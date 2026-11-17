#![no_std]
#![no_main]

use core::panic::PanicInfo;
use core::arch::asm;

// Import Surface from your graphics library.
use ouroboros_system::graphics::{Surface, MouseCursor};

static mut PIXELS: [u32; 640 * 400] = [0; 640 * 400];

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {

    // Inside _start:
    let pixels = unsafe {
        core::slice::from_raw_parts_mut(
            core::ptr::addr_of_mut!(PIXELS).cast::<u32>(),
            640 * 400,
        )
    };

    let mut surface = Surface::new(640, 400, pixels).unwrap();

    surface.fill_rect(0, 0, 640, 400, 0xFF202830);

    surface.fill_rect(50, 50, 300, 200, 0xFF405060);
    surface.draw_rect(50, 50, 300, 200, 3, 0xFFFFFFFF);

    surface.draw_text(70, 70, "OUROBOROS", 0xFFFFFFFF);
    surface.draw_text(70, 90, "GRAPHICS TEST", 0xFF00FF00);

    let mut cursor = MouseCursor::new(200, 150);
    cursor.draw(&mut surface);

    loop {
        unsafe {
            asm!("svc #2", options(nomem, nostack));
        }
    }
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    let message = b"\nUSERSPACE PANIC\n";

    unsafe {
        asm!(
            "svc #3",
            inlateout("x0") message.as_ptr() as u64 => _,
            in("x1") message.len() as u64,
            options(nostack)
        );

        asm!("svc #1", options(nostack));
    }

    loop {
        unsafe {
            asm!("svc #2", options(nomem, nostack));
        }
    }
}