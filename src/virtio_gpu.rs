use core::ptr::{read_volatile, write_volatile};
use core::sync::atomic::{fence, Ordering};

const GPU_BASE: usize = 0x0A00_3C00;
const MAGIC: usize = 0x000;
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

const ACKNOWLEDGE: u32 = 1;
const DRIVER: u32 = 2;
const DRIVER_OK: u32 = 4;
const FEATURES_OK: u32 = 8;
const VERSION_1: u64 = 1 << 32;
const QUEUE_SIZE: usize = 8;
const CMD_RESOURCE_CREATE_2D: u32 = 0x0101;
const CMD_RESOURCE_UNREF: u32 = 0x0102;
const CMD_SET_SCANOUT: u32 = 0x0103;
const CMD_RESOURCE_FLUSH: u32 = 0x0104;
const CMD_TRANSFER_TO_HOST_2D: u32 = 0x0105;
const CMD_RESOURCE_ATTACH_BACKING: u32 = 0x0106;

const RESP_OK_NODATA: u32 = 0x1100;

const GPU_WIDTH: usize = 1280;
const GPU_HEIGHT: usize = 800;
const GPU_RESOURCE_ID: u32 = 1;

// VirtIO GPU format B8G8R8A8_UNORM.
const GPU_FORMAT_BGRA8: u32 = 1;

#[repr(C)]
#[derive(Clone, Copy)]
struct GpuResourceCreate2d {
    header: GpuCtrlHeader,
    resource_id: u32,
    format: u32,
    width: u32,
    height: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct GpuResourceAttachBacking {
    header: GpuCtrlHeader,
    resource_id: u32,
    entry_count: u32,
    address: u64,
    length: u32,
    padding: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct GpuSetScanout {
    header: GpuCtrlHeader,
    rect: GpuRect,
    scanout_id: u32,
    resource_id: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct GpuTransferToHost2d {
    header: GpuCtrlHeader,
    rect: GpuRect,
    offset: u64,
    resource_id: u32,
    padding: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct GpuResourceFlush {
    header: GpuCtrlHeader,
    rect: GpuRect,
    resource_id: u32,
    padding: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct GpuMemEntry {
    address: u64,
    length: u32,
    padding: u32,
}

#[repr(C)]
struct GpuAttachBackingRequest {
    header: GpuCtrlHeader,
    resource_id: u32,
    entry_count: u32,
    entry: GpuMemEntry,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct Descriptor{
    addr: u64,
    len: u32,
    flags: u16,
    next: u16,
}
impl Descriptor{
    const EMPTY: Self = Self{addr: 0, len: 0, flags: 0, next: 0};
}
#[repr(C, align(16))]
struct DescriptorTable([Descriptor; QUEUE_SIZE]);
#[repr(C, align(2))]
struct AvailableRing{
    flags: u16,
    index: u16,
    ring: [u16; QUEUE_SIZE],
    used_event: u16,
}
#[repr(C)]
#[derive(Clone, Copy)]
struct UsedElement{id: u32, len: u32}
#[repr(C, align(4))]
struct UsedRing{
    flags: u16, 
    index: u16,
    ring: [UsedElement; QUEUE_SIZE],
    available_event: u16,
}
#[repr(C)]
#[derive(Clone, Copy)]
struct GpuCtrlHeader {
    command_type: u32,
    flags: u32,
    fence_id: u64,
    context_id: u32,
    padding: u32,
}
#[repr(C)]
#[derive(Clone, Copy)]
struct GpuRect {
    x: u32,
    y: u32,
    width: u32,
    height: u32,
}
#[repr(C)]
#[derive(Clone, Copy)]
struct GpuDisplayOne{
    rect: GpuRect,
    enabled: u32,
    flags: u32,
}
#[repr(C)]
#[derive(Clone, Copy)]
struct GpuDisplayInfo{
    header: GpuCtrlHeader,
    displays: [GpuDisplayOne; 16],
}
static mut DESCRIPTORS: DescriptorTable = DescriptorTable([Descriptor::EMPTY; QUEUE_SIZE]);
static mut AVAILABLE: AvailableRing = AvailableRing{ flags: 0, index: 0, ring: [0; QUEUE_SIZE], used_event: 0,}; 
static mut USED: UsedRing = UsedRing {flags: 0, index: 0, ring: [UsedElement{id:0,len:0}; QUEUE_SIZE], available_event: 0};
static mut DISPLAY_REQUEST: GpuCtrlHeader = GpuCtrlHeader{command_type: 0x0100, flags: 0, fence_id: 0, context_id: 0, padding: 0};
static mut DISPLAY_RESPONSE: GpuDisplayInfo = GpuDisplayInfo {
    header: GpuCtrlHeader {
        command_type: 0,
        flags: 0,
        fence_id: 0,
        context_id: 0,
        padding: 0,
    },
    displays: [GpuDisplayOne {
        rect: GpuRect {
            x: 0,
            y: 0,
            width: 0,
            height: 0,
        },
        enabled: 0,
        flags: 0,
    }; 16],
};
static mut LAST_USED_INDEX: u16 = 0;
#[repr(C, align(4096))]
struct Framebuffer([u32; GPU_WIDTH * GPU_HEIGHT]);

static mut FRAMEBUFFER: Framebuffer =
    Framebuffer([0; GPU_WIDTH * GPU_HEIGHT]);
#[derive(Debug, Clone, Copy)]
pub enum GpuError{
    NotPresent,
    WrongDevice,
    FeaturesRejected,
    NoQueue,
    QueueFailed,
    BadResponse,
    NoDisplay,
}
pub struct VirtioGpu;
impl VirtioGpu{
    pub const fn new() -> Self{Self}
    #[inline(always)]
    unsafe fn read32(offset:usize) -> u32{read_volatile((GPU_BASE + offset) as *const u32)}
    #[inline(always)]
    unsafe fn write32(offset:usize, value:u32){write_volatile((GPU_BASE + offset) as *mut u32, value);}
    pub fn probe(&self) -> (u32, u32, u32){
        unsafe{
            (Self::read32(MAGIC), Self::read32(VERSION), Self::read32(DEVICE_ID))
        }
    }
    pub fn init(&self) -> Result<(), GpuError>{
        unsafe {
            Self::write32(STATUS, 0);

            if Self::read32(MAGIC) != 0x7472_6976 {
                return Err(GpuError::NotPresent);
            }

            if Self::read32(VERSION) != 2 || Self::read32(DEVICE_ID) != 16 {
                return Err(GpuError::WrongDevice);
            }

            Self::write32(STATUS, ACKNOWLEDGE);
            Self::write32(STATUS, ACKNOWLEDGE | DRIVER);

            // Read both 32-bit halves of the feature bitmap.
            Self::write32(DEVICE_FEATURES_SEL, 0);
            let low = Self::read32(DEVICE_FEATURES);

            Self::write32(DEVICE_FEATURES_SEL, 1);
            let high = Self::read32(DEVICE_FEATURES);

            let features = (low as u64) | ((high as u64) << 32);
            let negotiated = features & VERSION_1;

            // Advertise only VIRTIO_F_VERSION_1.
            Self::write32(DRIVER_FEATURES_SEL, 0);
            Self::write32(DRIVER_FEATURES, negotiated as u32);

            Self::write32(DRIVER_FEATURES_SEL, 1);
            Self::write32(DRIVER_FEATURES, (negotiated >> 32) as u32);

            let status = ACKNOWLEDGE | DRIVER | FEATURES_OK;
            Self::write32(STATUS, status);

            if Self::read32(STATUS) & FEATURES_OK == 0 {
                return Err(GpuError::FeaturesRejected);
            }

            // Select queue 0: the GPU control queue.
            Self::write32(QUEUE_SEL, 0);

            let max_size = Self::read32(QUEUE_NUM_MAX) as usize;
            if max_size < QUEUE_SIZE {
                return Err(GpuError::NoQueue);
            }

            Self::write32(QUEUE_NUM, QUEUE_SIZE as u32);

            let descriptors = core::ptr::addr_of!(DESCRIPTORS) as u64;
            let available = core::ptr::addr_of!(AVAILABLE) as u64;
            let used = core::ptr::addr_of!(USED) as u64;

            Self::write32(QUEUE_DESC_LOW, descriptors as u32);
            Self::write32(QUEUE_DESC_HIGH, (descriptors >> 32) as u32);

            Self::write32(QUEUE_DRIVER_LOW, available as u32);
            Self::write32(QUEUE_DRIVER_HIGH, (available >> 32) as u32);

            Self::write32(QUEUE_DEVICE_LOW, used as u32);
            Self::write32(QUEUE_DEVICE_HIGH, (used >> 32) as u32);

            fence(Ordering::SeqCst);
            Self::write32(QUEUE_READY, 1);

            Self::write32(STATUS, status | DRIVER_OK);

            if Self::read32(STATUS) & DRIVER_OK == 0 {
                return Err(GpuError::WrongDevice);
            }

            Ok(())
        }
    }
    pub fn get_display_info(&self) -> Result<(u32, u32), GpuError>{
        unsafe{
            let descriptors = core::ptr::addr_of_mut!(DESCRIPTORS) as *mut Descriptor;
            let request_addr = core::ptr::addr_of!(DISPLAY_REQUEST) as u64;
            let response_addr= core::ptr::addr_of_mut!(DISPLAY_RESPONSE) as u64;
            descriptors.add(0).write_volatile(Descriptor {
                addr: request_addr,
                len: core::mem::size_of::<GpuCtrlHeader>() as u32,
                flags: 1, // NEXT
                next: 1,
            });

            descriptors.add(1).write_volatile(Descriptor {
                addr: response_addr,
                len: core::mem::size_of::<GpuDisplayInfo>() as u32,
                flags: 2, // WRITE
                next: 0,
            });

            // Publish descriptor 0 as the next available request.
            let available =
                core::ptr::addr_of_mut!(AVAILABLE);

            let available_index_ptr =
                core::ptr::addr_of_mut!((*available).index);

            let available_index =
                read_volatile(available_index_ptr);

            let slot =
                (available_index as usize) & (QUEUE_SIZE - 1);

            write_volatile(
                core::ptr::addr_of_mut!((*available).ring[slot]),
                0,
            );

            // Make the descriptor and ring writes visible before
            // publishing the updated available index.
            fence(Ordering::SeqCst);

            write_volatile(
                available_index_ptr,
                available_index.wrapping_add(1),
            );

            fence(Ordering::SeqCst);

            // Notify the GPU that queue 0 contains a request.
            Self::write32(QUEUE_NOTIFY, 0);

            // Wait for the GPU to complete the request.
            let used = core::ptr::addr_of!(USED);

            let used_index_ptr =
                core::ptr::addr_of!((*used).index);

            let mut spins = 0usize;

            loop {
                let used_index = read_volatile(used_index_ptr);

                if used_index != LAST_USED_INDEX {
                    break;
                }

                spins += 1;

                if spins >= 10_000_000 {
                    return Err(GpuError::QueueFailed);
                }

                core::hint::spin_loop();
            }

            // The GPU has completed the request. Read its completion
            // record only after observing the updated used index.
            fence(Ordering::SeqCst);

            let used_slot =
                (LAST_USED_INDEX as usize) & (QUEUE_SIZE - 1);

            let used_element =
                core::ptr::addr_of!((*used).ring[used_slot]);

            let descriptor_id =
                read_volatile(core::ptr::addr_of!((*used_element).id));

            let response_len =
                read_volatile(core::ptr::addr_of!((*used_element).len));

            LAST_USED_INDEX = LAST_USED_INDEX.wrapping_add(1);

            // Our request chain begins with descriptor 0.
            if descriptor_id != 0 {
                return Err(GpuError::QueueFailed);
            }

            // The GPU should have returned a complete display-info
            // response, not just a partial header.
            if response_len < core::mem::size_of::<GpuDisplayInfo>() as u32 {
                return Err(GpuError::BadResponse);
            }

            // 0x1101 = VIRTIO_GPU_RESP_OK_DISPLAY_INFO.
            let response_type = read_volatile(
                core::ptr::addr_of!(DISPLAY_RESPONSE.header.command_type),
            );

            if response_type != 0x1101 {
                return Err(GpuError::BadResponse);
            }

            // Return the dimensions of the first enabled display.
            let display_count = 16;

            for i in 0..display_count {
                let display =
                    core::ptr::addr_of!(DISPLAY_RESPONSE.displays[i]);

                let enabled =
                    read_volatile(core::ptr::addr_of!((*display).enabled));

                if enabled == 0 {
                    continue;
                }

                let width = read_volatile(
                    core::ptr::addr_of!((*display).rect.width),
                );

                let height = read_volatile(
                    core::ptr::addr_of!((*display).rect.height),
                );

                if width != 0 && height != 0 {
                    return Ok((width, height));
                }
            }

            Err(GpuError::NoDisplay)
        }

    }
    unsafe fn submit_command(&self, request: *const u8, request_len: usize, response: *mut u8, response_len: usize,) -> Option<u32> {
        use core::ptr::{addr_of, addr_of_mut, read_volatile, write_volatile};
        use core::sync::atomic::{fence, Ordering};

        write_volatile(
            addr_of_mut!(DESCRIPTORS.0[0]),
            Descriptor {
                addr: request as u64,
                len: request_len as u32,
                flags: 1,
                next: 1,
            },
        );

        write_volatile(
            addr_of_mut!(DESCRIPTORS.0[1]),
            Descriptor {
                addr: response as u64,
                len: response_len as u32,
                flags: 2,
                next: 0,
            },
        );

        let available_index_ptr = addr_of_mut!(AVAILABLE.index);
        let available_index = read_volatile(available_index_ptr);

        write_volatile(
            addr_of_mut!(
                AVAILABLE.ring[(available_index as usize) % QUEUE_SIZE]
            ),
            0,
        );

        fence(Ordering::Release);
        write_volatile(available_index_ptr, available_index.wrapping_add(1));
        fence(Ordering::SeqCst);
        write_volatile((GPU_BASE + 0x050) as *mut u32, 0);

        let last_used_ptr = addr_of_mut!(LAST_USED_INDEX);
        let last_used = read_volatile(last_used_ptr);
        let used_index_ptr = addr_of!(USED.index);

        let mut completed = false;

        for _ in 0..10_000_000 {
            if read_volatile(used_index_ptr) != last_used {
                completed = true;
                break;
            }
        }

        if !completed {
            return None;
        }

        fence(Ordering::Acquire);

        let used_element =
            read_volatile(addr_of!(USED.ring[(last_used as usize) % QUEUE_SIZE]));

        if used_element.id != 0
            || (used_element.len as usize)
                < core::mem::size_of::<GpuCtrlHeader>()
        {
            return None;
        }

        write_volatile(last_used_ptr, last_used.wrapping_add(1));

        Some(read_volatile(response.cast::<u32>()))
    }
    pub fn show_background(&self, pixel:u32) -> bool {
        use core::ptr::addr_of_mut;

        unsafe {
            let framebuffer =
                addr_of_mut!(FRAMEBUFFER.0) as *mut u32;

            for i in 0..(GPU_WIDTH * GPU_HEIGHT) {
                framebuffer.add(i).write_volatile(pixel);
            }

            let rect = GpuRect {
                x: 0,
                y: 0,
                width: GPU_WIDTH as u32,
                height: GPU_HEIGHT as u32,
            };

            // 1. Create a 1280 x 800 GPU resource.
            let create = GpuResourceCreate2d {
                header: GpuCtrlHeader {
                    command_type: CMD_RESOURCE_CREATE_2D,
                    flags: 0,
                    fence_id: 0,
                    context_id: 0,
                    padding: 0,
                },
                resource_id: GPU_RESOURCE_ID,
                format: GPU_FORMAT_BGRA8,
                width: GPU_WIDTH as u32,
                height: GPU_HEIGHT as u32,
            };

            let mut response = GpuCtrlHeader {
                command_type: 0,
                flags: 0,
                fence_id: 0,
                context_id: 0,
                padding: 0,
            };

            if self.submit_command(
                &create as *const _ as *const u8,
                core::mem::size_of_val(&create),
                &mut response as *mut _ as *mut u8,
                core::mem::size_of_val(&response),
            ) != Some(RESP_OK_NODATA) {
                return false;
            }

            // 2. Attach the framebuffer memory to the GPU resource.
            let attach = GpuAttachBackingRequest {
                header: GpuCtrlHeader {
                    command_type: CMD_RESOURCE_ATTACH_BACKING,
                    flags: 0,
                    fence_id: 0,
                    context_id: 0,
                    padding: 0,
                },
                resource_id: GPU_RESOURCE_ID,
                entry_count: 1,
                entry: GpuMemEntry {
                    address: framebuffer as u64,
                    length: (GPU_WIDTH * GPU_HEIGHT * 4) as u32,
                    padding: 0,
                },
            };

            if self.submit_command(
                &attach as *const _ as *const u8,
                core::mem::size_of_val(&attach),
                &mut response as *mut _ as *mut u8,
                core::mem::size_of_val(&response),
            ) != Some(RESP_OK_NODATA) {
                return false;
            }

            // 3. Assign the resource to scanout 0.
            let scanout = GpuSetScanout {
                header: GpuCtrlHeader {
                    command_type: CMD_SET_SCANOUT,
                    flags: 0,
                    fence_id: 0,
                    context_id: 0,
                    padding: 0,
                },
                rect,
                scanout_id: 0,
                resource_id: GPU_RESOURCE_ID,
            };

            if self.submit_command(
                &scanout as *const _ as *const u8,
                core::mem::size_of_val(&scanout),
                &mut response as *mut _ as *mut u8,
                core::mem::size_of_val(&response),
            ) != Some(RESP_OK_NODATA) {
                return false;
            }

            // 4. Transfer the framebuffer pixels to the host GPU.
            let transfer = GpuTransferToHost2d {
                header: GpuCtrlHeader {
                    command_type: CMD_TRANSFER_TO_HOST_2D,
                    flags: 0,
                    fence_id: 0,
                    context_id: 0,
                    padding: 0,
                },
                rect,
                offset: 0,
                resource_id: GPU_RESOURCE_ID,
                padding: 0,
            };

            if self.submit_command(
                &transfer as *const _ as *const u8,
                core::mem::size_of_val(&transfer),
                &mut response as *mut _ as *mut u8,
                core::mem::size_of_val(&response),
            ) != Some(RESP_OK_NODATA) {
                return false;
            }

            // 5. Flush the updated rectangle to the display.
            let flush = GpuResourceFlush {
                header: GpuCtrlHeader {
                    command_type: CMD_RESOURCE_FLUSH,
                    flags: 0,
                    fence_id: 0,
                    context_id: 0,
                    padding: 0,
                },
                rect,
                resource_id: GPU_RESOURCE_ID,
                padding: 0,
            };

            if self.submit_command(
                &flush as *const _ as *const u8,
                core::mem::size_of_val(&flush),
                &mut response as *mut _ as *mut u8,
                core::mem::size_of_val(&response),
            ) != Some(RESP_OK_NODATA) {
                return false;
            }
        }

        true
    }
    pub fn present_frame(&self) -> bool {
        use core::mem::size_of_val;

        let rect = GpuRect {
            x: 0,
            y: 0,
            width: GPU_WIDTH as u32,
            height: GPU_HEIGHT as u32,
        };

        let mut response = GpuCtrlHeader {
            command_type: 0,
            flags: 0,
            fence_id: 0,
            context_id: 0,
            padding: 0,
        };

        let transfer = GpuTransferToHost2d {
            header: GpuCtrlHeader {
                command_type: CMD_TRANSFER_TO_HOST_2D,
                flags: 0,
                fence_id: 0,
                context_id: 0,
                padding: 0,
            },
            rect,
            offset: 0,
            resource_id: GPU_RESOURCE_ID,
            padding: 0,
        };

        let flush = GpuResourceFlush {
            header: GpuCtrlHeader {
                command_type: CMD_RESOURCE_FLUSH,
                flags: 0,
                fence_id: 0,
                context_id: 0,
                padding: 0,
            },
            rect,
            resource_id: GPU_RESOURCE_ID,
            padding: 0,
        };

        unsafe {
            if self.submit_command(
                &transfer as *const _ as *const u8,
                size_of_val(&transfer),
                &mut response as *mut _ as *mut u8,
                size_of_val(&response),
            ) != Some(RESP_OK_NODATA) {
                return false;
            }

            if self.submit_command(
                &flush as *const _ as *const u8,
                size_of_val(&flush),
                &mut response as *mut _ as *mut u8,
                size_of_val(&response),
            ) != Some(RESP_OK_NODATA) {
                return false;
            }
        }

        true
    }
    pub fn put_pixel(x: i32, y: i32, colour: u32) {
        // Ignore coordinates outside the display.
        if x < 0
            || y < 0
            || x >= GPU_WIDTH as i32
            || y >= GPU_HEIGHT as i32
        {
            return;
        }

        unsafe {
            let framebuffer =
                core::ptr::addr_of_mut!(FRAMEBUFFER.0) as *mut u32;

            let index = y as usize * GPU_WIDTH + x as usize;
            framebuffer.add(index).write_volatile(colour);
        }
    }
    pub fn fill_rect(
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        colour: u32,
    ) {
        if width <= 0 || height <= 0 {
            return;
        }

        // Clip the rectangle against the framebuffer boundaries.
        let left = x.max(0);
        let top = y.max(0);
        let right = x.saturating_add(width).min(GPU_WIDTH as i32);
        let bottom = y.saturating_add(height).min(GPU_HEIGHT as i32);

        if left >= right || top >= bottom {
            return;
        }

        unsafe {
            let framebuffer =
                core::ptr::addr_of_mut!(FRAMEBUFFER.0) as *mut u32;

            for row in top..bottom {
                let start = row as usize * GPU_WIDTH + left as usize;
                let end = row as usize * GPU_WIDTH + right as usize;

                for index in start..end {
                    framebuffer.add(index).write_volatile(colour);
                }
            }
        }
    }
    pub fn draw_rect(
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        thickness: i32,
        colour: u32,
    ) {
        if width <= 0 || height <= 0 || thickness <= 0 {
            return;
        }

        // Four filled rectangles make the border.
        Self::fill_rect(x, y, width, thickness, colour);
        Self::fill_rect(
            x,
            y.saturating_add(height).saturating_sub(thickness),
            width,
            thickness,
            colour,
        );
        Self::fill_rect(x, y, thickness, height, colour);
        Self::fill_rect(
            x.saturating_add(width).saturating_sub(thickness),
            y,
            thickness,
            height,
            colour,
        );
    }
    fn glyph_rows(character: char) -> [u8; 7] {
        match character.to_ascii_uppercase() {
            'A' => [14, 17, 17, 31, 17, 17, 17],
            'B' => [30, 17, 17, 30, 17, 17, 30],
            'C' => [14, 17, 16, 16, 16, 17, 14],
            'D' => [30, 17, 17, 17, 17, 17, 30],
            'E' => [31, 16, 16, 30, 16, 16, 31],
            'F' => [31, 16, 16, 30, 16, 16, 16],
            'G' => [14, 17, 16, 23, 17, 17, 15],
            'H' => [17, 17, 17, 31, 17, 17, 17],
            'I' => [14, 4, 4, 4, 4, 4, 14],
            'J' => [7, 2, 2, 2, 18, 18, 12],
            'K' => [17, 18, 20, 24, 20, 18, 17],
            'L' => [16, 16, 16, 16, 16, 16, 31],
            'M' => [17, 27, 21, 21, 17, 17, 17],
            'N' => [17, 25, 21, 19, 17, 17, 17],
            'O' => [14, 17, 17, 17, 17, 17, 14],
            'P' => [30, 17, 17, 30, 16, 16, 16],
            'Q' => [14, 17, 17, 17, 21, 18, 13],
            'R' => [30, 17, 17, 30, 20, 18, 17],
            'S' => [15, 16, 16, 14, 1, 1, 30],
            'T' => [31, 4, 4, 4, 4, 4, 4],
            'U' => [17, 17, 17, 17, 17, 17, 14],
            'V' => [17, 17, 17, 17, 17, 10, 4],
            'W' => [17, 17, 17, 21, 21, 21, 10],
            'X' => [17, 17, 10, 4, 10, 17, 17],
            'Y' => [17, 17, 10, 4, 4, 4, 4],
            'Z' => [31, 1, 2, 4, 8, 16, 31],

            '0' => [14, 17, 19, 21, 25, 17, 14],
            '1' => [4, 12, 4, 4, 4, 4, 14],
            '2' => [14, 17, 1, 2, 4, 8, 31],
            '3' => [30, 1, 1, 14, 1, 1, 30],
            '4' => [2, 6, 10, 18, 31, 2, 2],
            '5' => [31, 16, 16, 30, 1, 1, 30],
            '6' => [14, 16, 16, 30, 17, 17, 14],
            '7' => [31, 1, 2, 4, 8, 8, 8],
            '8' => [14, 17, 17, 14, 17, 17, 14],
            '9' => [14, 17, 17, 15, 1, 1, 14],

            ' ' => [0; 7],
            '.' => [0, 0, 0, 0, 0, 4, 4],
            ',' => [0, 0, 0, 0, 4, 4, 8],
            ':' => [0, 4, 4, 0, 4, 4, 0],
            '!' => [4, 4, 4, 4, 4, 0, 4],
            '?' => [14, 17, 1, 2, 4, 0, 4],
            '-' => [0, 0, 0, 31, 0, 0, 0],
            '_' => [0, 0, 0, 0, 0, 0, 31],
            '/' => [1, 2, 2, 4, 8, 8, 16],
            '+' => [0, 4, 4, 31, 4, 4, 0],
            '=' => [0, 31, 0, 31, 0, 0, 0],
            '(' => [2, 4, 8, 8, 8, 4, 2],
            ')' => [8, 4, 2, 2, 2, 4, 8],
            '\'' => [4, 4, 8, 0, 0, 0, 0],

            _ => [14, 17, 1, 2, 4, 0, 4], // Unknown character: ?
        }
    }
    pub fn draw_char(x: i32, y: i32, character: char, colour: u32) {
        let rows = Self::glyph_rows(character);

        for (row_index, bits) in rows.iter().enumerate() {
            for column in 0..5 {
                // The top bit of each 5-bit row is the leftmost pixel.
                if bits & (1u8 << (4 - column)) != 0 {
                    Self::put_pixel(
                        x.saturating_add(column as i32),
                        y.saturating_add(row_index as i32),
                        colour,
                    );
                }
            }
        }
    }

    pub fn draw_text(
        mut x: i32,
        mut y: i32,
        text: &str,
        colour: u32,
    ) {
        let line_start = x;

        for character in text.chars() {
            if character == '\n' {
                x = line_start;
                y = y.saturating_add(9);
                continue;
            }

            // Move to a new line if the next character would exceed the screen.
            if x.saturating_add(5) > GPU_WIDTH as i32 {
                x = line_start;
                y = y.saturating_add(9);
            }

            if y >= GPU_HEIGHT as i32 {
                break;
            }

            Self::draw_char(x, y, character, colour);
            x = x.saturating_add(6);
        }
    }
    pub fn show_gui_demo(&self) -> bool {
        // Create and initialise the GPU resource, with a teal background.
        if !self.show_background(008083u32) {
            return false;
        }

        const TEAL: u32 = 0xFF00_8080;
        const BLACK: u32 = 0xFF00_0000;
        const WHITE: u32 = 0xFFFF_FFFF;
        const GREY: u32 = 0xFFC0_C0C0;
        const DARK_GREY: u32 = 0xFF80_8080;
        const NAVY: u32 = 0xFF00_0080;

        // Desktop taskbar.
        Self::fill_rect(
            0,
            GPU_HEIGHT as i32 - 42,
            GPU_WIDTH as i32,
            42,
            GREY,
        );
        Self::fill_rect(
            0,
            GPU_HEIGHT as i32 - 42,
            GPU_WIDTH as i32,
            2,
            WHITE,
        );

        // Start button.
        Self::fill_rect(8, 766, 88, 27, GREY);
        Self::draw_rect(8, 766, 88, 27, 1, BLACK);
        Self::fill_rect(10, 768, 84, 1, WHITE);
        Self::fill_rect(10, 768, 1, 23, WHITE);
        Self::fill_rect(10, 791, 84, 1, DARK_GREY);
        Self::fill_rect(93, 768, 1, 23, DARK_GREY);
        Self::draw_text(24, 776, "START", BLACK);

        // Main window.
        Self::fill_rect(260, 160, 640, 390, GREY);
        Self::draw_rect(260, 160, 640, 390, 2, BLACK);

        // Title bar.
        Self::fill_rect(263, 163, 634, 28, NAVY);
        Self::draw_text(274, 173, "OUROBOROS - FIRST GUI", WHITE);

        // Close button.
        Self::fill_rect(869, 167, 21, 20, GREY);
        Self::draw_rect(869, 167, 21, 20, 1, BLACK);
        Self::draw_text(876, 173, "X", BLACK);

        // Window content.
        Self::draw_text(285, 215, "WELCOME TO OUROBOROS.", BLACK);
        Self::draw_text(285, 240, "THE FRAMEBUFFER CAN Self::draw SHAPES", BLACK);
        Self::draw_text(285, 251, "AND RENDER BITMAP TEXT.", BLACK);

        Self::draw_text(285, 290, "THIS IS THE BEGINNING OF THE GUI.", BLACK);

        // A raised-looking button.
        Self::fill_rect(285, 340, 100, 30, GREY);
        Self::draw_rect(285, 340, 100, 30, 1, BLACK);
        Self::fill_rect(286, 341, 98, 1, WHITE);
        Self::fill_rect(286, 341, 1, 28, WHITE);
        Self::fill_rect(286, 369, 98, 1, DARK_GREY);
        Self::fill_rect(384, 341, 1, 28, DARK_GREY);
        Self::draw_text(319, 351, "OK", BLACK);

        // Show the complete frame.
        let _ = TEAL; // Background colour is set by show_teal_background().
        self.present_frame()
    }

}
const CURSOR_WIDTH: usize = 16;
const CURSOR_HEIGHT: usize = 16;

const CURSOR_PATTERN: [&str; CURSOR_HEIGHT] = [
    "#...............",
    "##..............",
    "#W#.............",
    "#WW#............",
    "#WWW#...........",
    "#WWWW#..........",
    "#WWWWW#.........",
    "#WWWWWW#........",
    "#WWWWWWW#.......",
    "#WWWW#####......",
    "#WWW#...........",
    "#WW#.#..........",
    "#W#..#..........",
    "##....#.........",
    "#.....#.........",
    "......##........",
];

pub struct MouseCursor {
    x: i32,
    y: i32,
    saved: [u32; CURSOR_WIDTH * CURSOR_HEIGHT],
    drawn: bool,
}

impl MouseCursor {
    pub const fn new(x: i32, y: i32) -> Self {
        Self {
            x,
            y,
            saved: [0; CURSOR_WIDTH * CURSOR_HEIGHT],
            drawn: false,
        }
    }

    pub fn draw(&mut self) {
        if self.drawn {
            self.erase();
        }

        self.x = self.x.clamp(
            0,
            GPU_WIDTH as i32 - CURSOR_WIDTH as i32,
        );
        self.y = self.y.clamp(
            0,
            GPU_HEIGHT as i32 - CURSOR_HEIGHT as i32,
        );

        self.paint();
    }

    pub fn move_by(&mut self, dx: i32, dy: i32) -> bool {
        if !self.drawn {
            self.draw();
        }

        let new_x = self.x.saturating_add(dx).clamp(
            0,
            GPU_WIDTH as i32 - CURSOR_WIDTH as i32,
        );
        let new_y = self.y.saturating_add(dy).clamp(
            0,
            GPU_HEIGHT as i32 - CURSOR_HEIGHT as i32,
        );

        if new_x == self.x && new_y == self.y {
            return false;
        }

        self.erase();

        self.x = new_x;
        self.y = new_y;

        self.paint();
        true
    }

    fn erase(&mut self) {
        use core::ptr::write_volatile;

        unsafe {
            let framebuffer =
                core::ptr::addr_of_mut!(FRAMEBUFFER.0) as *mut u32;

            for row in 0..CURSOR_HEIGHT {
                for col in 0..CURSOR_WIDTH {
                    let x = self.x + col as i32;
                    let y = self.y + row as i32;
                    let saved_index = row * CURSOR_WIDTH + col;

                    if x >= 0
                        && y >= 0
                        && x < GPU_WIDTH as i32
                        && y < GPU_HEIGHT as i32
                    {
                        let index =
                            y as usize * GPU_WIDTH + x as usize;

                        write_volatile(
                            framebuffer.add(index),
                            self.saved[saved_index],
                        );
                    }
                }
            }
        }

        self.drawn = false;
    }

    fn paint(&mut self) {
        use core::ptr::{read_volatile, write_volatile};

        unsafe {
            let framebuffer =
                core::ptr::addr_of_mut!(FRAMEBUFFER.0) as *mut u32;

            // Save the pixels beneath the cursor before drawing it.
            for row in 0..CURSOR_HEIGHT {
                for col in 0..CURSOR_WIDTH {
                    let x = self.x + col as i32;
                    let y = self.y + row as i32;
                    let saved_index = row * CURSOR_WIDTH + col;

                    self.saved[saved_index] =
                        if x >= 0
                            && y >= 0
                            && x < GPU_WIDTH as i32
                            && y < GPU_HEIGHT as i32
                        {
                            let index =
                                y as usize * GPU_WIDTH + x as usize;

                            read_volatile(framebuffer.add(index))
                        } else {
                            0
                        };
                }
            }

            // Draw the black outline and white interior.
            for row in 0..CURSOR_HEIGHT {
                let pattern = CURSOR_PATTERN[row].as_bytes();

                for col in 0..CURSOR_WIDTH {
                    let colour = match pattern[col] {
                        b'#' => 0xFF00_0000,
                        b'W' => 0xFFFF_FFFF,
                        _ => continue,
                    };

                    let x = self.x + col as i32;
                    let y = self.y + row as i32;

                    if x >= 0
                        && y >= 0
                        && x < GPU_WIDTH as i32
                        && y < GPU_HEIGHT as i32
                    {
                        let index =
                            y as usize * GPU_WIDTH + x as usize;

                        write_volatile(
                            framebuffer.add(index),
                            colour,
                        );
                    }
                }
            }
        }

        self.drawn = true;
        
    }
}