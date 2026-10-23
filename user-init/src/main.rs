#![no_std]
#![no_main]

mod syscall;
use core::arch::asm;
use core::panic::PanicInfo;

fn write_number(mut value: u64) {
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

fn write_hex_byte(value: u8) {
    const TABLE: &[u8; 16] = b"0123456789abcdef";
    let low = TABLE[(value & 0x0f) as usize];
    let high = TABLE[((value >> 4) & 0x0f) as usize];
    syscall::sys_write(&[high, low]);
}

fn write_hex_u64(mut value: u64) {
    let mut buffer = [0u8; 16];
    let mut index = buffer.len();

    while index > 0 {
        index -= 1;
        buffer[index] = b"0123456789abcdef"[(value & 0xf) as usize];
        value >>= 4;
    }

    syscall::sys_write(&buffer);
}

fn strip_spaces(bytes: &[u8]) -> &[u8] {
    let start = bytes.iter().position(|byte| !byte.is_ascii_whitespace()).unwrap_or(bytes.len());
    let end = bytes
        .iter()
        .rposition(|byte| !byte.is_ascii_whitespace())
        .map(|index| index + 1)
        .unwrap_or(start);

    &bytes[start..end]
}

fn read_line(buffer: &mut [u8]) -> usize {
    let mut length = 0usize;

    loop {
        let byte = match syscall::read_byte() {
            Some(byte) => byte,
            None => {
                syscall::yield_now();
                continue;
            }
        };

        match byte {
            b'\r' | b'\n' => {
                syscall::sys_write(b"\r\n");
                return length;
            }
            8 | 127 => {
                if length > 0 {
                    length -= 1;
                    syscall::sys_write(b"\x08 \x08");
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

fn print_hex_dump(bytes: &[u8]) {
    if bytes.is_empty() {
        syscall::sys_write(b"(empty)\r\n");
        return;
    }

    for offset in (0..bytes.len()).step_by(16) {
        let line = &bytes[offset..bytes.len().min(offset + 16)];

        write_hex_u64(offset as u64);
        syscall::sys_write(b": ");

        for (index, byte) in line.iter().enumerate() {
            if index == 8 {
                syscall::sys_write(b" ");
            }
            write_hex_byte(*byte);
            syscall::sys_write(b" ");
        }

        for _ in 0..(16 - line.len()) {
            syscall::sys_write(b"   ");
        }
        if line.len() <= 8 {
            syscall::sys_write(b" ");
        }

        syscall::sys_write(b"|");
        for byte in line {
            let printable = *byte >= 0x20 && *byte <= 0x7e;
            syscall::sys_write(&[if printable { *byte } else { b'.' }]);
        }
        syscall::sys_write(b"|\r\n");
    }
}

fn edit_file(path: &[u8], scratch: &mut [u8]) -> bool {
    let mut text_length = match syscall::read_file(path, scratch) {
        Some(count) => count,
        None => 0,
    };

    if text_length > 0 {
        syscall::sys_write(b"Current contents:\r\n");
        syscall::sys_write(&scratch[..text_length]);
        if !scratch[..text_length].ends_with(b"\n") {
            syscall::sys_write(b"\r\n");
        }
        syscall::sys_write(b"--- end of file ---\r\n");
    } else {
        syscall::sys_write(b"New file (empty)\r\n");
    }

    let mut line = [0u8; 128];

    loop {
        syscall::sys_write(b"edit> ");
        let line_length = read_line(&mut line);
        let command = strip_spaces(&line[..line_length]);

        if command.is_empty() {
            if text_length + 1 <= scratch.len() {
                scratch[text_length] = b'\n';
                text_length += 1;
            } else {
                syscall::sys_write(b"edit: buffer full\r\n");
            }
            continue;
        }

        match command {
            b":help" => {
                syscall::sys_write(
                    b"Command summary: :view, :w save, :q quit, :wq save+quit\r\n"
                );
            }
            b":view" | b":list" => {
                if text_length > 0 {
                    syscall::sys_write(&scratch[..text_length]);
                    if !scratch[..text_length].ends_with(b"\n") {
                        syscall::sys_write(b"\r\n");
                    }
                } else {
                    syscall::sys_write(b"(empty)\r\n");
                }
            }
            b":w" | b":write" => {
                match syscall::write_file(path, &scratch[..text_length]) {
                    Some(count) => {
                        syscall::sys_write(b"Saved ");
                        write_number(count as u64);
                        syscall::sys_write(b" bytes to ");
                        syscall::sys_write(path);
                        syscall::sys_write(b"\r\n");
                    }
                    None => {
                        syscall::sys_write(b"edit: save failed\r\n");
                    }
                }
            }
            b":q" | b":quit" => {
                syscall::sys_write(b"Edit cancelled.\r\n");
                return false;
            }
            b":wq" | b":writequit" => {
                match syscall::write_file(path, &scratch[..text_length]) {
                    Some(count) => {
                        syscall::sys_write(b"Saved ");
                        write_number(count as u64);
                        syscall::sys_write(b" bytes to ");
                        syscall::sys_write(path);
                        syscall::sys_write(b"\r\n");
                        return true;
                    }
                    None => {
                        syscall::sys_write(b"edit: save failed\r\n");
                    }
                }
            }
            _ => {
                if text_length + command.len() + 1 > scratch.len() {
                    syscall::sys_write(b"edit: buffer full\r\n");
                    continue;
                }

                scratch[text_length..text_length + command.len()].copy_from_slice(command);
                text_length += command.len();
                scratch[text_length] = b'\n';
                text_length += 1;
            }
        }
    }
}

fn shell() -> ! {
    const FILE_BUFFER_PAGES: usize = 12;
    const FILE_BUFFER_SIZE: usize = FILE_BUFFER_PAGES * 4096;

    // Reuse this buffer for file operations instead of allocating
    // memory every time someone runs cat.
    let file_buffer = syscall::alloc_pages(FILE_BUFFER_PAGES);

    if file_buffer.is_none() {
        syscall::sys_write(
            b"Warning: filesystem buffer allocation failed.\r\n"
        );
    }

    let mut line = [0u8; 128];
    let mut ignore_next_lf = false;

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

            if ignore_next_lf {
                ignore_next_lf = false;

                if byte == b'\n' {
                    continue;
                }
            }

            match byte {
                b'\r' => {
                    ignore_next_lf = true;
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
                b"Commands: help, stat, cat, ls, touch, write,\r\n\
                  \x20         rm, mkdir, rmdir, run, edit, hex\r\n\
                  Paths must be absolute.\r\n"
            );
        }

        // ----------------------------------------------------
        // hex <path>
        // ----------------------------------------------------
        else if command.starts_with(b"hex ") {
            let path = strip_spaces(&command[4..]);

            if path.is_empty() {
                syscall::sys_write(b"Usage: hex /path\r\n");
                continue;
            }

            let buffer = match file_buffer {
                Some(ptr) => unsafe {
                    core::slice::from_raw_parts_mut(ptr, FILE_BUFFER_SIZE)
                },
                None => {
                    syscall::sys_write(b"hex: buffer allocation failed\r\n");
                    continue;
                }
            };

            match syscall::read_file(path, buffer) {
                Some(count) => {
                    print_hex_dump(&buffer[..count]);
                }
                None => {
                    syscall::sys_write(b"hex: read failed\r\n");
                }
            }
        }

        // ----------------------------------------------------
        // edit <path>
        // ----------------------------------------------------
        else if command.starts_with(b"edit ") {
            let path = strip_spaces(&command[5..]);

            if path.is_empty() {
                syscall::sys_write(b"Usage: edit /path\r\n");
                continue;
            }

            let buffer = match file_buffer {
                Some(ptr) => unsafe {
                    core::slice::from_raw_parts_mut(ptr, FILE_BUFFER_SIZE)
                },
                None => {
                    syscall::sys_write(b"edit: buffer allocation failed\r\n");
                    continue;
                }
            };

            let _ = edit_file(path, buffer);
        }

        // ----------------------------------------------------
        // stat <path>
        // ----------------------------------------------------
        else if command.starts_with(b"stat ") {
            let path = &command[5..];

            match syscall::stat(path) {
                Some(info) => {
                    syscall::sys_write(b"inode: ");
                    write_number(info.inode);

                    syscall::sys_write(b"\r\nsize: ");
                    write_number(info.size);

                    syscall::sys_write(b"\r\ntype: ");

                    if info.kind == syscall::FILE_KIND_FILE {
                        syscall::sys_write(b"file");
                    } else if info.kind == syscall::FILE_KIND_DIRECTORY {
                        syscall::sys_write(b"directory");
                    } else {
                        syscall::sys_write(b"unknown");
                    }

                    syscall::sys_write(b"\r\nlinks: ");
                    write_number(info.links);
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

            let buffer_ptr = match file_buffer {
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
                    buffer_ptr,
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

            let buffer_ptr = match file_buffer {
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
            let empty_data = unsafe {
                core::slice::from_raw_parts(buffer_ptr, 0)
            };

            match syscall::create_file(path, empty_data) {
                Some(inode) => {
                    syscall::sys_write(b"Created inode ");
                    write_number(inode as u64);
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
                            write_number(count as u64);
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
                    write_number(inode as u64);
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
            if path == b"/bin/user-init.run"{syscall::sys_write(b"You should NOT be trying to open a nested shell.\r\n"); continue;}
            match syscall::spawn(path) {
                Some(task_id) => {
                    syscall::sys_write(b"Started task ");
                    write_number(task_id as u64);
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

            let buffer = match file_buffer {
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