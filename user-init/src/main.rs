#![no_std]
#![no_main]

use core::arch::asm;
use core::fmt::{self, Write};

struct Stdout;

impl Write for Stdout {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        let result: isize;

        unsafe {
            asm!(
                "svc #3",
                in("x0") s.as_ptr(),
                in("x1") s.len(),
                lateout("x0") result,
            );
        }

        if result == s.len() as isize {
            Ok(())
        } else {
            Err(fmt::Error)
        }
    }
}

fn yield_now() {
    unsafe {
        asm!("svc #2");
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    let mut stdout = Stdout;

    writeln!(&mut stdout, "Hello from Ouroboros userspace!").unwrap();
    writeln!(&mut stdout, "The answer is {}", 42).unwrap();
    writeln!(&mut stdout, "Hex: {:#x}", 0xdeadbeef_u64).unwrap();
    writeln!(&mut stdout, "Formatting works.").unwrap();

    loop {
        yield_now();
    }
}
use core::panic::PanicInfo;

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {
        yield_now();
    }
}