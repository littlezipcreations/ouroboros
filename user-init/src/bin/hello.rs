#![no_std]
#![no_main]

#[path = "../syscall.rs"]
use ouroboros_system::syscall;

use core::panic::PanicInfo;

fn write(bytes: &[u8]) {
    let _ = syscall::sys_write(bytes);
}

fn write_number(mut value: u64) {
    let mut buffer = [0u8; 20];
    let mut index = buffer.len();

    if value == 0 {
        write(b"0");
        return;
    }

    while value > 0 {
        index -= 1;
        buffer[index] = b'0' + (value % 10) as u8;
        value /= 10;
    }

    write(&buffer[index..]);
}

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    write(b"Number of Fibonacci terms (0-94): ");

    let mut input = [0u8; 16];
    let mut inputLength = 0;

    // Read a line from the UART.
    loop {
        match syscall::read_byte() {
            None => core::hint::spin_loop(),

            Some(b'\r') | Some(b'\n') => {
                write(b"\r\n");
                break;
            }

            Some(8) | Some(127) => {
                if inputLength > 0 {
                    inputLength -= 1;
                    write(b"\x08 \x08");
                }
            }

            Some(byte) => {
                if inputLength < input.len() {
                    input[inputLength] = byte;
                    inputLength += 1;
                    write(&[byte]);
                }
            }
        }
    }

    // Parse the requested number of terms.
    if inputLength == 0 {
        write(b"Invalid number of terms.\r\n");
        syscall::exit();
    }

    let mut count = 0usize;

    for &digit in &input[..inputLength] {
        if !digit.is_ascii_digit() {
            write(b"Please enter a number from 0 to 94.\r\n");
            syscall::exit();
        }

        count = match count
            .checked_mul(10)
            .and_then(|n| n.checked_add((digit - b'0') as usize))
        {
            Some(number) => number,
            None => {
                write(b"Number too large.\r\n");
                syscall::exit();
            }
        };
    }

    if count > 94 {
        write(b"Maximum is 94 terms (u64 limit).\r\n");
        syscall::exit();
    }

    // Generate F(0), F(1), F(2), ...
    // A u64 can hold F(0) through F(93), but not F(94).
    let mut terms = [0u64; 94];

    if count > 1 {
        terms[1] = 1;
    }

    for index in 2..count {
        terms[index] = terms[index - 1] + terms[index - 2];
    }

    // Print one term per line.
    for term in &terms[..count] {
        write_number(*term);
        write(b"\r\n");
    }

    write(b"Done.\r\n");
    syscall::exit();
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    syscall::exit()
}