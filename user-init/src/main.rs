#![no_std]
#![no_main]

mod syscall;
use core::arch::asm;
use core::panic::PanicInfo;

fn writeNumber(mut value: u64) {
    let mut buffer = [0u8; 20];
    let mut cursor = buffer.len();

    if value == 0 {
        syscall::sys_write(b"0");
        return;
    }

    while value != 0 {
        cursor -= 1;
        buffer[cursor] = b'0' + (value % 10) as u8;
        value /= 10;
    }

    syscall::sys_write(&buffer[cursor..]);
}
fn shell() -> ! {
    const FILE_BUFFER_PAGES: usize = 12;
    const FILE_BUFFER_SIZE: usize = FILE_BUFFER_PAGES * 4096;

    // Reuse this buffer for file operations instead of allocating
    // memory every time someone runs cat.
    let fileBuffer = syscall::alloc_pages(FILE_BUFFER_PAGES);

    if fileBuffer.is_none() {
        syscall::sys_write(
            b"Warning: filesystem buffer allocation failed.\r\n"
        );
    }

    let mut line = [0u8; 128];
    let mut ignoreNextLf = false;

    loop {
        syscall::sys_write(b"ouro> ");
        let mut length = 0;

        // Collect one line of input.
        loop {
            let byte = match syscall::read_byte() {
                Some(byte) => byte,
                None => {
                    syscall::yield_now();
                    continue;
                }
            };

            if ignoreNextLf {
                ignoreNextLf = false;

                if byte == b'\n' {
                    continue;
                }
            }

            match byte {
                b'\r' => {
                    ignoreNextLf = true;
                    syscall::sys_write(b"\r\n");
                    break;
                }

                b'\n' => {
                    syscall::sys_write(b"\r\n");
                    break;
                }

                8 | 127 => {
                    if length > 0 {
                        length -= 1;
                        syscall::sys_write(b"\x08 \x08");
                    }
                }

                0x20..=0x7e => {
                    if length < line.len() {
                        line[length] = byte;
                        length += 1;
                        syscall::sys_write(&[byte]);
                    }
                }

                _ => {}
            }
        }

        let command = &line[..length];

        if command.is_empty() {
            continue;
        }

        // ----------------------------------------------------
        // help
        // ----------------------------------------------------
        if command == b"help" {
            syscall::sys_write(
                b"Commands: help, stat, cat, touch, write,\r\n\
                  \x20         rm, mkdir, rmdir, run\r\n\
                  Paths must be absolute.\r\n"
            );
        }

        // ----------------------------------------------------
        // stat <path>
        // ----------------------------------------------------
        else if command.starts_with(b"stat ") {
            let path = &command[5..];

            match syscall::stat(path) {
                Some(info) => {
                    syscall::sys_write(b"inode: ");
                    writeNumber(info.inode);

                    syscall::sys_write(b"\r\nsize: ");
                    writeNumber(info.size);

                    syscall::sys_write(b"\r\ntype: ");

                    if info.kind == syscall::FILE_KIND_FILE {
                        syscall::sys_write(b"file");
                    } else if info.kind == syscall::FILE_KIND_DIRECTORY {
                        syscall::sys_write(b"directory");
                    } else {
                        syscall::sys_write(b"unknown");
                    }

                    syscall::sys_write(b"\r\nlinks: ");
                    writeNumber(info.links);
                    syscall::sys_write(b"\r\n");
                }

                None => {
                    syscall::sys_write(
                        b"stat: operation failed\r\n"
                    );
                }
            }
        }

        // ----------------------------------------------------
        // cat <path>
        // ----------------------------------------------------
        else if command.starts_with(b"cat ") {
            let path = &command[4..];

            let bufferPtr = match fileBuffer {
                Some(ptr) => ptr,
                None => {
                    syscall::sys_write(
                        b"cat: no filesystem buffer\r\n"
                    );
                    continue;
                }
            };

            let output = unsafe {
                core::slice::from_raw_parts_mut(
                    bufferPtr,
                    FILE_BUFFER_SIZE,
                )
            };

            match syscall::read_file(path, output) {
                Some(count) => {
                    if count > 0 {
                        syscall::sys_write(&output[..count]);

                        if output[count - 1] != b'\n' {
                            syscall::sys_write(b"\r\n");
                        }
                    } else {
                        syscall::sys_write(b"\r\n");
                    }
                }

                None => {
                    syscall::sys_write(
                        b"cat: read failed; file may be missing \
                          or too large\r\n"
                    );
                }
            }
        }

        // ----------------------------------------------------
        // touch <path>
        // ----------------------------------------------------
        else if command.starts_with(b"touch ") {
            let path = &command[6..];

            let bufferPtr = match fileBuffer {
                Some(ptr) => ptr,
                None => {
                    syscall::sys_write(
                        b"touch: no filesystem buffer\r\n"
                    );
                    continue;
                }
            };

            // The zero-length slice points into allocated
            // userspace memory, rather than kernel text.
            let emptyData = unsafe {
                core::slice::from_raw_parts(bufferPtr, 0)
            };

            match syscall::create_file(path, emptyData) {
                Some(inode) => {
                    syscall::sys_write(b"Created inode ");
                    writeNumber(inode as u64);
                    syscall::sys_write(b"\r\n");
                }

                None => {
                    syscall::sys_write(
                        b"touch: create failed; path may exist\r\n"
                    );
                }
            }
        }

        // ----------------------------------------------------
        // write <path> <contents>
        // Replaces the file's entire contents.
        // ----------------------------------------------------
        else if command.starts_with(b"write ") {
            let arguments = &command[6..];

            match arguments.iter().position(|&byte| byte == b' ') {
                Some(separator) if separator > 0 => {
                    let path = &arguments[..separator];
                    let contents = &arguments[separator + 1..];

                    match syscall::write_file(path, contents) {
                        Some(count) => {
                            syscall::sys_write(b"Wrote ");
                            writeNumber(count as u64);
                            syscall::sys_write(b" bytes\r\n");
                        }

                        None => {
                            syscall::sys_write(
                                b"write: operation failed\r\n"
                            );
                        }
                    }
                }

                _ => {
                    syscall::sys_write(
                        b"Usage: write /path contents\r\n"
                    );
                }
            }
        }

        // ----------------------------------------------------
        // rm <path>
        // ----------------------------------------------------
        else if command.starts_with(b"rm ") {
            let path = &command[3..];

            if syscall::delete_file(path) {
                syscall::sys_write(b"File deleted.\r\n");
            } else {
                syscall::sys_write(
                    b"rm: deletion failed\r\n"
                );
            }
        }

        // ----------------------------------------------------
        // mkdir <path>
        // ----------------------------------------------------
        else if command.starts_with(b"mkdir ") {
            let path = &command[6..];

            match syscall::create_dir(path) {
                Some(inode) => {
                    syscall::sys_write(b"Created directory inode ");
                    writeNumber(inode as u64);
                    syscall::sys_write(b"\r\n");
                }

                None => {
                    syscall::sys_write(
                        b"mkdir: operation failed\r\n"
                    );
                }
            }
        }

        // ----------------------------------------------------
        // rmdir <path>
        // ----------------------------------------------------
        else if command.starts_with(b"rmdir ") {
            let path = &command[6..];

            if syscall::remove_dir(path) {
                syscall::sys_write(b"Directory removed.\r\n");
            } else {
                syscall::sys_write(
                    b"rmdir: removal failed; directory may not be empty\r\n"
                );
            }
        }

        // ----------------------------------------------------
        // run <path>
        // ----------------------------------------------------
        else if command.starts_with(b"run ") {
            let path = &command[4..];

            match syscall::spawn(path) {
                Some(taskId) => {
                    syscall::sys_write(b"Started task ");
                    writeNumber(taskId as u64);
                    syscall::sys_write(b"\r\n");
                }

                None => {
                    syscall::sys_write(
                        b"run: executable could not be started\r\n"
                    );
                }
            }
        }

        // ----------------------------------------------------
        // ls <path>
        // ----------------------------------------------------
        else if command == b"ls" || command.starts_with(b"ls ") {
            let mut path = if command.len() == 2 {
                &b"/"[..]
            } else {
                &command[3..]
            };

            // Ignore extra spaces around the path.
            while path.first() == Some(&b' ') {
                path = &path[1..];
            }
            while path.last() == Some(&b' ') {
                path = &path[..path.len() - 1];
            }

            if path.is_empty() {
                path = b"/";
            }

            let buffer = match fileBuffer {
                Some(ptr) => unsafe {
                    core::slice::from_raw_parts_mut(ptr, 12 * 4096)
                },
                None => {
                    syscall::sys_write(b"ls: file buffer allocation failed\n");
                    continue;
                }
            };

            match syscall::list_directory(path, buffer) {
                Some(count) => {
                    syscall::sys_write(&buffer[..count]);
                }
                None => {
                    syscall::sys_write(
                        b"ls: cannot list directory (missing path or buffer too small)\n"
                    );
                }
            }
        }
        // ----------------------------------------------------
        // Unknown command
        // ----------------------------------------------------
        else {
            syscall::sys_write(b"Unknown command: ");
            syscall::sys_write(command);
            syscall::sys_write(b"\r\n");
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
	shell();
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