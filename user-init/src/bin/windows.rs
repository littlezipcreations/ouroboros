#![no_std]
#![no_main]
use core::panic::PanicInfo;
use core::arch::asm;

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

        // Exit this userspace task.
        asm!("svc #1", options(nostack));
    }

    loop {
        unsafe {
            asm!("svc #2", options(nomem, nostack));
        }
    }
}  