#![no_std]
#![no_main]

mod syscall;
use core::arch::asm;
use core::panic::PanicInfo;

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
	loop {
    match syscall::read_byte() {
        Some(byte) => {
            syscall::sys_write(&[byte]);
        }
        None => {
            syscall::yield_now();
        }
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

		// Exit this userspace task.
		asm!("svc #1", options(nostack));
	}

	loop {
		unsafe {
			asm!("svc #2", options(nomem, nostack));
		}
	}
}