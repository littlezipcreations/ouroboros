#![no_std]
#![no_main]

mod syscall;
use core::panic::PanicInfo;

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    syscall::test();
    for _ in 0..10{
        syscall::yield_now();
    }
    syscall::exit();
}
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    syscall::exit();
}