//! Minimal polling driver for a VirtIO MMIO mouse.
//!
//! This first version consumes queue 0 (the VirtIO input event queue).
//! It supports relative X/Y movement and mouse buttons. It intentionally
//! polls rather than using interrupts, matching Ouroboros's current drivers.

use core::ptr::{read_volatile, write_volatile};
use core::sync::atomic::{fence, Ordering};

// With the current QEMU device ordering:
//   block = 0x0A00_3E00
//   GPU   = 0x0A00_3C00
// the mouse added after the GPU is expected at 0x0A00_3A00.
// init() probes the device before writing any registers at this address.
const VIRTIO_INPUT_BASE: usize = 0x0A00_3A00;

const MAGIC_VALUE: usize = 0x000;
const VERSION: usize = 0x004;
const DEVICE_ID: usize = 0x008;
const DEVICE_FEATURES: usize = 0x010;
const DEVICE_FEATURES_SEL: usize = 0x014;
const DRIVER_FEATURES: usize = 0x020;
const DRIVER_FEATURES_SEL: usize = 0x024;
const QUEUE_SEL: usize = 0x030;
const QUEUE_NUM_MAX: usize = 0x034;
const QUEUE_NUM: usize = 0x038;
const QUEUE_READY: usize = 0x044;
const QUEUE_NOTIFY: usize = 0x050;
const STATUS: usize = 0x070;
const QUEUE_DESC_LOW: usize = 0x080;
const QUEUE_DESC_HIGH: usize = 0x084;
const QUEUE_DRIVER_LOW: usize = 0x090;
const QUEUE_DRIVER_HIGH: usize = 0x094;
const QUEUE_DEVICE_LOW: usize = 0x0A0;
const QUEUE_DEVICE_HIGH: usize = 0x0A4;

const STATUS_ACKNOWLEDGE: u32 = 1 << 0;
const STATUS_DRIVER: u32 = 1 << 1;
const STATUS_DRIVER_OK: u32 = 1 << 2;
const STATUS_FEATURES_OK: u32 = 1 << 3;
const FEATURE_VERSION_1: u64 = 1 << 32;

const DESC_F_WRITE: u16 = 2;
const DEVICE_ID_INPUT: u32 = 18;

const QUEUE_SIZE: usize = 8;
const DESC_OFFSET: usize = 0;
const AVAIL_OFFSET: usize = 128;
const USED_OFFSET: usize = 148;
const AVAIL_RING_OFFSET: usize = AVAIL_OFFSET + 4;
const USED_RING_OFFSET: usize = USED_OFFSET + 4;

pub const EV_SYN: u16 = 0x00;
pub const EV_KEY: u16 = 0x01;
pub const EV_REL: u16 = 0x02;
pub const REL_X: u16 = 0x00;
pub const REL_Y: u16 = 0x01;
pub const BTN_LEFT: u16 = 0x110;
pub const BTN_RIGHT: u16 = 0x111;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct VirtioInputEvent {
    pub event_type: u16,
    pub code: u16,
    /// Relative motion is signed: interpret this as `value as i32` for EV_REL.
    pub value: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct VirtqDesc {
    addr: u64,
    len: u32,
    flags: u16,
    next: u16,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct UsedElement {
    id: u32,
    len: u32,
}

#[repr(C, align(4096))]
struct QueueMemory {
    bytes: [u8; 4096],
}

#[repr(C, align(16))]
struct EventBuffers([VirtioInputEvent; QUEUE_SIZE]);

static mut QUEUE: QueueMemory = QueueMemory { bytes: [0; 4096] };
static mut EVENT_BUFFERS: EventBuffers = EventBuffers([
    VirtioInputEvent { event_type: 0, code: 0, value: 0 };
    QUEUE_SIZE
]);

#[derive(Debug, Clone, Copy)]
pub enum VirtioInputError {
    WrongDevice,
    VersionFeatureMissing,
    FeaturesRejected,
    NoEventQueue,
    QueueNotReady,
}

/// One driver instance owns the module's static queue. Do not create multiple
/// active VirtioInput objects.
pub struct VirtioInput {
    base: usize,
    last_used_index: u16,
    ready: bool,
}

impl VirtioInput {
    pub const fn new() -> Self {
        Self {
            base: VIRTIO_INPUT_BASE,
            last_used_index: 0,
            ready: false,
        }
    }

    #[inline(always)]
    fn read32(&self, offset: usize) -> u32 {
        unsafe { read_volatile((self.base + offset) as *const u32) }
    }

    #[inline(always)]
    fn write32(&self, offset: usize, value: u32) {
        unsafe { write_volatile((self.base + offset) as *mut u32, value) }
    }

    /// Read-only probe. Safe to call before init; it does not change device state.
    pub fn probe(&self) -> (u32, u32, u32) {
        (
            self.read32(MAGIC_VALUE),
            self.read32(VERSION),
            self.read32(DEVICE_ID),
        )
    }

    pub fn init(&mut self) -> Result<(), VirtioInputError> {
        // Do this check before writing STATUS. If the presumed MMIO address is
        // wrong, do not accidentally reset a different VirtIO device.
        let (magic, version, device_id) = self.probe();
        if magic != 0x7472_6976 || version != 2 || device_id != DEVICE_ID_INPUT {
            return Err(VirtioInputError::WrongDevice);
        }

        self.write32(STATUS, 0);
        self.write32(STATUS, STATUS_ACKNOWLEDGE);
        self.write32(STATUS, STATUS_ACKNOWLEDGE | STATUS_DRIVER);

        self.write32(DEVICE_FEATURES_SEL, 0);
        let features_low = self.read32(DEVICE_FEATURES) as u64;
        self.write32(DEVICE_FEATURES_SEL, 1);
        let features_high = self.read32(DEVICE_FEATURES) as u64;
        let offered_features = features_low | (features_high << 32);

        if offered_features & FEATURE_VERSION_1 == 0 {
            return Err(VirtioInputError::VersionFeatureMissing);
        }

        // VirtIO input has no additional device-specific feature bits needed by
        // this driver. Negotiate only the modern VERSION_1 transport feature.
        self.write32(DRIVER_FEATURES_SEL, 0);
        self.write32(DRIVER_FEATURES, 0);
        self.write32(DRIVER_FEATURES_SEL, 1);
        self.write32(DRIVER_FEATURES, 1);

        let status = STATUS_ACKNOWLEDGE | STATUS_DRIVER | STATUS_FEATURES_OK;
        self.write32(STATUS, status);
        if self.read32(STATUS) & STATUS_FEATURES_OK == 0 {
            return Err(VirtioInputError::FeaturesRejected);
        }

        self.write32(QUEUE_SEL, 0); // queue 0 is the input event queue
        if self.read32(QUEUE_NUM_MAX) < QUEUE_SIZE as u32 {
            return Err(VirtioInputError::NoEventQueue);
        }
        self.write32(QUEUE_NUM, QUEUE_SIZE as u32);

        unsafe {
            let queue_base = core::ptr::addr_of_mut!(QUEUE.bytes) as *mut u8;
            core::ptr::write_bytes(queue_base, 0, 4096);

            let event_buffers = core::ptr::addr_of_mut!(EVENT_BUFFERS.0)
                as *mut VirtioInputEvent;
            let descriptors = queue_base.add(DESC_OFFSET) as *mut VirtqDesc;

            for index in 0..QUEUE_SIZE {
                write_volatile(
                    event_buffers.add(index),
                    VirtioInputEvent { event_type: 0, code: 0, value: 0 },
                );
                write_volatile(
                    descriptors.add(index),
                    VirtqDesc {
                        addr: event_buffers.add(index) as u64,
                        len: core::mem::size_of::<VirtioInputEvent>() as u32,
                        flags: DESC_F_WRITE,
                        next: 0,
                    },
                );
                write_volatile(
                    queue_base.add(AVAIL_RING_OFFSET + index * 2) as *mut u16,
                    index as u16,
                );
            }

            // The device may write an event into any of the eight buffers.
            // We poll the used ring, so ask the device not to interrupt us.
            write_volatile(queue_base.add(AVAIL_OFFSET) as *mut u16, 1);
            write_volatile(
                queue_base.add(AVAIL_OFFSET + 2) as *mut u16,
                QUEUE_SIZE as u16,
            );

            let desc_addr = queue_base.add(DESC_OFFSET) as u64;
            let avail_addr = queue_base.add(AVAIL_OFFSET) as u64;
            let used_addr = queue_base.add(USED_OFFSET) as u64;

            self.write32(QUEUE_DESC_LOW, desc_addr as u32);
            self.write32(QUEUE_DESC_HIGH, (desc_addr >> 32) as u32);
            self.write32(QUEUE_DRIVER_LOW, avail_addr as u32);
            self.write32(QUEUE_DRIVER_HIGH, (avail_addr >> 32) as u32);
            self.write32(QUEUE_DEVICE_LOW, used_addr as u32);
            self.write32(QUEUE_DEVICE_HIGH, (used_addr >> 32) as u32);
            fence(Ordering::SeqCst);
            self.write32(QUEUE_READY, 1);
            fence(Ordering::SeqCst);
        }

        self.write32(STATUS, status | STATUS_DRIVER_OK);
        if self.read32(STATUS) & STATUS_DRIVER_OK == 0 {
            return Err(VirtioInputError::QueueNotReady);
        }

        self.last_used_index = 0;
        self.ready = true;
        // Notify the device that receive buffers are available.
        self.write32(QUEUE_NOTIFY, 0);
        Ok(())
    }

    /// Return one available mouse/input event, or None if there is no event.
    /// Call repeatedly in the kernel's input-polling loop.
    pub fn poll_event(&mut self) -> Option<VirtioInputEvent> {
        if !self.ready {
            return None;
        }

        unsafe {
            let queue_base = core::ptr::addr_of_mut!(QUEUE.bytes) as *mut u8;
            let used_index_ptr = queue_base.add(USED_OFFSET + 2) as *const u16;
            let used_index = read_volatile(used_index_ptr);

            if used_index == self.last_used_index {
                return None;
            }

            fence(Ordering::Acquire);

            let used_slot = self.last_used_index as usize & (QUEUE_SIZE - 1);
            let used_element = read_volatile(
                queue_base.add(USED_RING_OFFSET + used_slot * 8) as *const UsedElement,
            );
            self.last_used_index = self.last_used_index.wrapping_add(1);

            let descriptor_id = used_element.id as usize;
            if descriptor_id >= QUEUE_SIZE {
                return None;
            }

            let event = if used_element.len as usize
                >= core::mem::size_of::<VirtioInputEvent>()
            {
                let event_ptr = core::ptr::addr_of!(EVENT_BUFFERS.0)
                    as *const VirtioInputEvent;
                let mut event = read_volatile(event_ptr.add(descriptor_id));
                // VirtIO input fields are little-endian.
                event.event_type = u16::from_le(event.event_type);
                event.code = u16::from_le(event.code);
                event.value = u32::from_le(event.value);
                Some(event)
            } else {
                None
            };

            // Return this descriptor to the available ring so the device can
            // use its buffer again for the next event.
            let avail_index_ptr = queue_base.add(AVAIL_OFFSET + 2) as *mut u16;
            let avail_index = read_volatile(avail_index_ptr);
            let avail_slot = avail_index as usize & (QUEUE_SIZE - 1);
            write_volatile(
                queue_base.add(AVAIL_RING_OFFSET + avail_slot * 2) as *mut u16,
                descriptor_id as u16,
            );
            fence(Ordering::Release);
            write_volatile(avail_index_ptr, avail_index.wrapping_add(1));
            fence(Ordering::SeqCst);
            self.write32(QUEUE_NOTIFY, 0);

            event
        }
    }
}
