use core::ptr::{read_volatile, write_volatile};
use core::sync::atomic::{fence, Ordering};

const VIRTIO_BASE: usize = 0x0A00_3E00;

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
const INTERRUPT_STATUS: usize = 0x060;
const STATUS: usize = 0x070;

const QUEUE_DESC_LOW: usize = 0x080;
const QUEUE_DESC_HIGH: usize = 0x084;
const QUEUE_DRIVER_LOW: usize = 0x090;
const QUEUE_DRIVER_HIGH: usize = 0x094;
const QUEUE_DEVICE_LOW: usize = 0x0A0;
const QUEUE_DEVICE_HIGH: usize = 0x0A4;

const CONFIG: usize = 0x100;

const STATUS_ACKNOWLEDGE: u32 = 1 << 0;
const STATUS_DRIVER: u32 = 1 << 1;
const STATUS_DRIVER_OK: u32 = 1 << 2;
const STATUS_FEATURES_OK: u32 = 1 << 3;

const FEATURE_VERSION_1: u64 = 1 << 32;

const DESC_F_NEXT: u16 = 1;
const DESC_F_WRITE: u16 = 2;

const QUEUE_SIZE: usize = 8;

const DESC_OFFSET: usize = 0;
const AVAIL_OFFSET: usize = 128;
const USED_OFFSET: usize = 148;
const AVAIL_RING_OFFSET: usize = AVAIL_OFFSET + 4;
const USED_RING_OFFSET: usize = USED_OFFSET + 4;

#[repr(C, align(4096))]
struct QueueMemory{
    bytes: [u8; 4096],
}
static mut QUEUE: QueueMemory = QueueMemory { bytes: [0; 4096] };

#[repr(C, align(16))]
struct BlockRequest{
    request_type: u32,
    reserved: u32,
    sector: u64,
}
static mut REQUEST: BlockRequest = BlockRequest { request_type: 0, reserved: 0, sector: 0 };

#[repr(C, align(4096))]
struct BlockBuffer{
    bytes: [u8; 4096],
}
static mut BUFFER: BlockBuffer = BlockBuffer { bytes: [0; 4096] };

static mut STATUS_BYTE: u8 = 0xFF;
static mut LAST_USED_INDEX: u16 = 0;

#[derive(Debug, Clone, Copy)]
pub enum VirtioError {
    NotPresent,
    WrongDevice,
    NoQueue,
    FeaturesRejected,
    IoFailed,
    BadBlock,
}
pub struct VirtioBlk;

impl VirtioBlk {
    pub const fn new() -> Self {Self}
    pub fn probe(&self) -> (u32, u32, u32) {
        unsafe {
            (
                Self::read32(MAGIC_VALUE),
                Self::read32(VERSION),
                Self::read32(DEVICE_ID),
            )
        }
    }

    #[inline(always)]
    unsafe fn read32(offset: usize) -> u32{
        read_volatile((VIRTIO_BASE + offset) as *const u32)
    }
    #[inline(always)]
    unsafe fn write32(offset: usize, value: u32){
        write_volatile((VIRTIO_BASE + offset) as *mut u32, value);
    }
    #[inline(always)]
    unsafe fn read_config32(offset: usize) -> u32{
        read_volatile((VIRTIO_BASE + CONFIG + offset) as *const u32)
    }

    fn status (&self) -> u32 {
        unsafe { Self::read32(STATUS) }
    }
    fn set_status (&self, value: u32) {
        unsafe { Self::write32(STATUS, value); }
    }
    pub fn init(&self) -> Result<(), VirtioError> {
        unsafe{
            Self::write32(STATUS, 0);
            if Self::read32(MAGIC_VALUE) != 0x74726976 {
                return Err(VirtioError::NotPresent);
            }
            if Self::read32(DEVICE_ID) != 2 {
                return Err(VirtioError::WrongDevice);
            }
            if Self::read32(VERSION) != 2 {
                return Err(VirtioError::WrongDevice);
            }
            Self::write32(STATUS, STATUS_ACKNOWLEDGE);
            Self::write32(STATUS, STATUS_ACKNOWLEDGE | STATUS_DRIVER);

            Self::write32(DRIVER_FEATURES_SEL, 0);
            let _features_low = Self::read32(DEVICE_FEATURES);
            Self::write32(DRIVER_FEATURES_SEL, 1);
            let _features_high = Self::read32(DEVICE_FEATURES);
            let device_features = (_features_low as u64) | ((_features_high as u64) << 32);
            let negotiated = device_features & FEATURE_VERSION_1 as u64;
            Self::write32(DRIVER_FEATURES_SEL, 0);
            Self::write32(DRIVER_FEATURES, (negotiated as u32));
            Self::write32(DRIVER_FEATURES_SEL, 1);
            Self::write32(DRIVER_FEATURES, (negotiated >> 32) as u32);
            let status = STATUS_ACKNOWLEDGE | STATUS_DRIVER | STATUS_FEATURES_OK;
            Self::write32(STATUS, status);

            if Self::read32(STATUS) & STATUS_FEATURES_OK == 0 {
                return Err(VirtioError::FeaturesRejected);
            }

            Self::write32(QUEUE_SEL, 0);
            let max_queue = Self::read32(QUEUE_NUM_MAX) as usize;

            if max_queue < QUEUE_SIZE {
                return Err(VirtioError::NoQueue);
            }
            
            Self::write32(QUEUE_NUM, QUEUE_SIZE as u32);
            let queue_base = core::ptr::addr_of!(QUEUE) as u64;
            let desc = queue_base + DESC_OFFSET as u64;
            let avail = queue_base + AVAIL_OFFSET as u64;
            let used = queue_base + USED_OFFSET as u64;
            Self::write32(QUEUE_DESC_LOW, desc as u32);
            Self::write32(QUEUE_DESC_HIGH, (desc >> 32) as u32);
            Self::write32(QUEUE_DRIVER_LOW, avail as u32);
            Self::write32(QUEUE_DRIVER_HIGH, (avail >> 32) as u32);
            Self::write32(QUEUE_DEVICE_LOW, used as u32);
            Self::write32(QUEUE_DEVICE_HIGH, (used >> 32) as u32);
            Self::write32(QUEUE_READY, 1);
            fence(Ordering::SeqCst);

            Self::write32(STATUS, status | STATUS_DRIVER_OK);

            if Self::read32(STATUS) != (status | STATUS_DRIVER_OK) {
                return Err(VirtioError::WrongDevice);
            }
            LAST_USED_INDEX = 0;
            Ok(())
        }
    }
    pub fn capacity_sectors(&self) -> u64 {
        unsafe { Self::read_config32(0) as u64 | ((Self::read_config32(4) as u64) << 32) }
    }
    pub fn capacity_blocks(&self) -> u64 {
        self.capacity_sectors() / 8
    }

    fn submit(&self, write: bool, block: u64) -> Result<(), VirtioError>{
        if block >= self.capacity_blocks() {
            return Err(VirtioError::BadBlock);
        }
        unsafe{
            let queue_base = core::ptr::addr_of_mut!(QUEUE) as *mut u8;
            let desc = queue_base as *mut VirtqDesc;

            REQUEST.request_type = if write {1} else {0};
            REQUEST.sector = block * 8;
            REQUEST.reserved = 0;
            let request_addr = core::ptr::addr_of!(REQUEST) as u64;
            let buffer_addr = core::ptr::addr_of!(BUFFER) as u64;
            let status_addr = core::ptr::addr_of!(STATUS_BYTE) as u64;

            STATUS_BYTE = 0xFF;
            (*desc.add(0)).addr = request_addr;
            (*desc.add(0)).len = core::mem::size_of::<BlockRequest>() as u32;
            (*desc.add(0)).flags = DESC_F_NEXT;
            (*desc.add(0)).next = 1;

            (*desc.add(1)).addr = buffer_addr;
            (*desc.add(1)).len = core::mem::size_of::<BlockBuffer>() as u32;
            (*desc.add(1)).flags = if write {DESC_F_NEXT} else {DESC_F_NEXT | DESC_F_WRITE};
            (*desc.add(1)).next = 2;

            (*desc.add(2)).addr = status_addr;
            (*desc.add(2)).len = 1;
            (*desc.add(2)).flags = DESC_F_WRITE;
            (*desc.add(2)).next = 0;

            let avail_idx = (queue_base.add(AVAIL_OFFSET + 2) as *const u16).read_volatile();
            let ring_index = (avail_idx as usize) & (QUEUE_SIZE - 1);
            (queue_base.add(AVAIL_RING_OFFSET + ring_index * 2) as *mut u16).write_volatile(0);
            fence(Ordering::SeqCst);
            (queue_base.add(AVAIL_OFFSET + 2) as *mut u16).write_volatile(avail_idx.wrapping_add(1));
            fence(Ordering::SeqCst);
            Self::write32(QUEUE_NOTIFY, 0);

            loop{
                fence(Ordering::SeqCst);
                let used_idx = (queue_base.add(USED_OFFSET + 2) as *const u16).read_volatile();
                if used_idx != LAST_USED_INDEX {
                    let used_slot = (LAST_USED_INDEX as usize) & (QUEUE_SIZE - 1);
                    let used_id = (queue_base.add(USED_RING_OFFSET + used_slot * 8) as *const u32).read_volatile();
                    if used_id != 0 {
                        return Err(VirtioError::IoFailed);
                    }
                    LAST_USED_INDEX = LAST_USED_INDEX.wrapping_add(1);
                    if STATUS_BYTE != 0 {
                        return Err(VirtioError::IoFailed);
                    }
                    break;
                }
                core::hint::spin_loop();
            }
            Ok(())
        }
    }
    pub fn read_block(&self, block:u64, output: &mut[u8; 4096]) -> Result<(), VirtioError>{
        unsafe {
            core::ptr::copy_nonoverlapping(
                core::ptr::addr_of!(BUFFER.bytes) as *const u8,
                output.as_mut_ptr(),
                4096,
            );
        }
        Ok(())
    }
    pub fn write_block(&self, block: u64, input: &[u8; 4096]) -> Result<(), VirtioError>{
        unsafe {
        core::ptr::copy_nonoverlapping(
            input.as_ptr(),
            core::ptr::addr_of_mut!(BUFFER.bytes) as *mut u8,
            4096,
        );
    }
        self.submit(true, block)
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
struct VirtqDesc{
    addr: u64,
    len: u32,
    flags: u16,
    next: u16,
}