#![no_std]
#![no_main]

extern crate alloc;

mod heap;
mod syscall;

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;

use core::fmt::{self, Write};
use core::panic::PanicInfo;

use syscall::sys_write;

struct Stdout;

impl Write for Stdout {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        let result = sys_write(s.as_bytes());

        if result == s.len() as isize {
            Ok(())
        } else {
            Err(fmt::Error)
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    let mut stdout = Stdout;

    writeln!(
        &mut stdout,
        "Hello from Ouroboros userspace!"
    ).unwrap();

    let message = String::from("Hello from a heap!");

    writeln!(
        &mut stdout,
        "String: {}",
        message
    ).unwrap();

    let boxed = Box::new(123456789_u64);

    writeln!(
        &mut stdout,
        "Box: {}",
        boxed
    ).unwrap();

    let mut numbers = Vec::new();

    for i in 0..1000_u64 {
        numbers.push(i);
    }

    writeln!(
        &mut stdout,
        "Vec length: {}",
        numbers.len()
    ).unwrap();

    writeln!(
        &mut stdout,
        "Vec[999]: {}",
        numbers[999]
    ).unwrap();

    writeln!(
        &mut stdout,
        "HEAP WORKS."
    ).unwrap();

    loop {
        syscall::yield_now();
    }
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {
        syscall::yield_now();
    }
}
