#![no_std]
#![no_main]
#![warn(non_snake_case)]

use ouroboros_system::syscall;
use ouroboros_system::print;
use core::panic::PanicInfo;
use core::arch::asm;

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

fn write_hex_byte(value: u8) {
    let _ = print!("{:02x}", value);
}

fn write_hex_u64(value: u64) {
    let _ = print!("{:016x}", value);
}

fn top() -> ! {
    let _ = print!("Top-like system monitor (press Ctrl+C to exit)\r\n");
    let _ = print!("Updating every 2 seconds...\r\n\r\n");

    // Helper to read the tick counter directly via SVC #16
    #[inline(always)]
    fn get_ticks() -> u64 {
        let result: u64;
        unsafe { asm!("svc #16", lateout("x0") result); }
        result
    }

    let mut last_ticks = get_ticks();

    loop {
        // Clear screen by printing many newlines (works in serial)
        for _ in 0..20 {
            let _ = print!("\r\n");
        }

        // Header
        let _ = print!("OUROBOROS SYSTEM MONITOR\r\n");
        let _ = print!("=======================\r\n\r\n");

        // Get current ticks for activity measurement
        let current_ticks = get_ticks();

        // Calculate system activity since last update
        let ticks_since_last = if current_ticks >= last_ticks {
            current_ticks - last_ticks
        } else {
            // Handle potential wrap-around (though unlikely in short time)
            0
        };
        last_ticks = current_ticks;

        // Display activity level and approximate CPU usage
        const MAX_TICKS_PER_INTERVAL: u64 = 100_000; // assumed max ticks in 2‑second window
        let cpu_percent = if ticks_since_last > MAX_TICKS_PER_INTERVAL {
            100
        } else {
            (ticks_since_last * 100) / MAX_TICKS_PER_INTERVAL
        };
        let _ = print!("System Activity: {} ticks since last update\r\n", ticks_since_last);
        let _ = print!("CPU Usage (approx): {}%\r\n", cpu_percent);
        if ticks_since_last > 10000 {
            let _ = print!("  High activity detected\r\n");
        } else if ticks_since_last > 1000 {
            let _ = print!("  Moderate activity\r\n");
        } else {
            let _ = print!("  Low activity/idle\r\n");
        }

        // Get file system information
        match syscall::disk_usage() {
            Ok(usage) => {
                let _ = print!("\r\nFilesystem:\r\n");
                let _ = print!("  Total blocks: {}\r\n", usage.total_blocks);
                let _ = print!("  Used blocks:  {}\r\n", usage.used_blocks);
                let _ = print!("  Free blocks:  {}\r\n", usage.available_blocks);
                let _ = print!("  Block size:   {} bytes\r\n", usage.block_size);
                
                let total_kib = usage.total_blocks.saturating_mul(usage.block_size) / 1024;
                let used_kib = usage.used_blocks.saturating_mul(usage.block_size) / 1024;
                let available_kib = usage.available_blocks.saturating_mul(usage.block_size) / 1024;
                let used_percent = if total_kib == 0 {
                    0
                } else {
                    (used_kib * 100) / total_kib
                };
                
                let _ = print!("  Usage: {}% ({} KiB/{} KiB)\r\n", used_percent, used_kib, total_kib);
            }
            Err(error) => {
                report_error(b"disk_usage", error);
            }
        }

        // We don't have syscalls for memory or process info, so we note that.
        let _ = print!("\r\nNote: Memory and process information not available (no syscalls).\r\n");

        // Sleep for 2 seconds by yielding multiple times.
        // We yield 200 times, each yield yields the CPU for a short time.
        for _ in 0..200 {
            let _ = syscall::yield_now();
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    top();
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    let message = b"\nUSERSPACE PANIC IN TOP\n";

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