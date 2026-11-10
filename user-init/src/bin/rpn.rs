#![no_std]
#![no_main]

use ouroboros_system::syscall;
use ouroboros_system::print;
use core::arch::asm;
use core::panic::PanicInfo;

fn write_number(value: u64) {
    let _ = print!("{}", value);
}

fn error_message(error: syscall::Error) -> &'static str {
    match error {
        syscall::Error::NotFound => "not found",
        syscall::Error::Unsupported => "operation not supported",
        syscall::Error::InvalidArgument => "invalid argument",
        syscall::Error::ResourceUnavailable => "resource unavailable",
        syscall::Error::PermissionDenied => "permission denied",
        syscall::Error::AlreadyExists => "already exists",
        syscall::Error::NotDirectory => "not a directory",
        syscall::Error::IsDirectory => "is a directory",
        syscall::Error::DirectoryNotEmpty => "directory not empty",
        syscall::Error::BufferTooSmall => "buffer too small",
        syscall::Error::InvalidAddr => "invalid address",
        syscall::Error::OoM => "out of memory",
        syscall::Error::IoErr => "I/O error",
        syscall::Error::WouldBlock => "would block",
        syscall::Error::Unknown => "unknown error",
    }
}

fn report_error(command: &[u8], error: syscall::Error) {
    let _ = syscall::sys_write(command);
    let _ = print!(": {}\r\n", error_message(error));
}

fn strip_spaces(bytes: &[u8]) -> &[u8] {
    let start = bytes.iter().position(|b| !b.is_ascii_whitespace()).unwrap_or(bytes.len());
    let end = bytes
        .iter()
        .rposition(|b| !b.is_ascii_whitespace())
        .map(|i| i + 1)
        .unwrap_or(start);
    &bytes[start..end]
}

fn parse_i64(bytes: &[u8]) -> Option<i64> {
    if bytes.is_empty() {
        return None;
    }
    let mut neg = false;
    let mut idx = 0usize;
    if bytes[0] == b'-' {
        neg = true;
        idx = 1;
    }
    let mut value: i64 = 0;
    while idx < bytes.len() {
        let b = bytes[idx];
        if !(b'0'..=b'9').contains(&b) {
            return None;
        }
        value = value.checked_mul(10)? + ((b - b'0') as i64);
        idx += 1;
    }
    if neg { Some(-value) } else { Some(value) }
}

fn read_line(buffer: &mut [u8]) -> usize {
    let mut length = 0usize;
    loop {
        let byte = match syscall::read_byte() {
            Ok(b) => b,
            Err(syscall::Error::WouldBlock) => {
                syscall::yield_now();
                continue;
            }
            Err(e) => {
                report_error(b"input", e);
                syscall::yield_now();
                continue;
            }
        };
        match byte {
            b'\r' | b'\n' => {
                let _ = print!("\r\n");
                return length;
            }
            8 | 127 => {
                if length > 0 {
                    length -= 1;
                    let _ = print!("\x08 \x08");
                }
            }
            0x20..=0x7e => {
                if length < buffer.len() {
                    buffer[length] = byte;
                    length += 1;
                    syscall::sys_write(&[byte]);
                }
            }
            _ => {}
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    let _ = print!("RPN Calculator\r\n");
    const BUF_SIZE: usize = 256;
    let mut line = [0u8; BUF_SIZE];
    let mut stack: [i64; 64] = [0; 64];
    let mut sp: usize = 0; // stack pointer
    loop {
        let _ = print!("rpn> ");
        let len = read_line(&mut line);
        let input = strip_spaces(&line[..len]);
        if input.is_empty() {
            continue;
        }
        // split by whitespace
        let mut start = 0usize;
            while start < input.len() {
                // find next space to isolate a token
                let mut end = start;
                while end < input.len() && !input[end].is_ascii_whitespace() {
                    end += 1;
                }
                let token = &input[start..end];

                // First handle known operators before attempting number parsing.
                let is_operator = matches!(token, b"+" | b"-" | b"*" | b"/");
                if is_operator {
                    if sp < 2 {
                        let _ = print!("Not enough operands\r\n");
                    } else {
                        let b = stack[sp - 1];
                        let a = stack[sp - 2];
                        sp -= 2;
                        let res = match token {
                            b"+" => a.wrapping_add(b),
                            b"-" => a.wrapping_sub(b),
                            b"*" => a.wrapping_mul(b),
                            b"/" => {
                                if b == 0 {
                                    let _ = print!("Division by zero\r\n");
                                    0
                                } else {
                                    a.wrapping_div(b)
                                }
                            }
                            _ => 0, // unreachable due to is_operator check
                        };
                        stack[sp] = res;
                        sp += 1;
                    }
                } else if let Some(num) = parse_i64(token) {
                    // treat token as a number
                    if sp >= stack.len() {
                        let _ = print!("Stack overflow\r\n");
                    } else {
                        stack[sp] = num;
                        sp += 1;
                    }
                } else {
                    let _ = print!("Unknown token\r\n");
                }

                // advance to next token, skipping any whitespace
                start = end;
                while start < input.len() && input[start].is_ascii_whitespace() {
                    start += 1;
                }
        }
        // after processing line, print top of stack if any
        if sp > 0 {
            let _ = print!("= ");
            write_number(stack[sp - 1] as u64);
            let _ = print!("\r\n");
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
        unsafe { asm!("svc #2", options(nomem, nostack)); }
    }
}
