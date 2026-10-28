#![no_std]
#![no_main]

use ouroboros_system::syscall;
use ouroboros_system::print;
use core::arch::asm;
use core::panic::PanicInfo;

fn write_number(value: u64) {
    let _ = print!("{}", value);
}

fn write_hex_byte(value: u8) {
    let _ = print!("{:02x}", value);
}

fn write_hex_u64(value: u64) {
    let _ = print!("{:016x}", value);
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

fn parse_usize(bytes: &[u8]) -> Option<usize> {
    if bytes.is_empty() {
        return None;
    }

    let mut value = 0usize;

    for &byte in bytes {
        if !(b'0'..=b'9').contains(&byte) {
            return None;
        }

        value = value.checked_mul(10)?.checked_add((byte - b'0') as usize)?;
    }

    Some(value)
}

fn replace_line(buffer: &mut [u8], text_length: &mut usize, line_number: usize, new_text: &[u8]) -> bool {
    if line_number == 0 {
        return false;
    }

    let mut line_index = 1usize;
    let mut line_start = 0usize;
    let mut line_end = 0usize;
    let mut match_start = 0usize;
    let mut match_end = 0usize;
    let mut found = false;

    while line_end <= *text_length {
        if line_end == *text_length || buffer[line_end] == b'\n' {
            if line_index == line_number {
                match_start = line_start;
                match_end = line_end;
                found = true;
                break;
            }

            line_start = line_end + 1;
            line_index += 1;
        }
        line_end += 1;
    }

    if !found {
        return false;
    }

    let kept_newline = if match_end < *text_length && buffer[match_end] == b'\n' { 1usize } else { 0usize };
    let old_line_length = match_end - match_start;
    let new_length = new_text.len();
    let tail_start = match_end + kept_newline;
    let tail_length = *text_length - tail_start;
    let after_replace_size = new_length + kept_newline;
    let removed_size = old_line_length + kept_newline;
    let new_total = *text_length - removed_size + after_replace_size;

    if new_total > buffer.len() {
        return false;
    }

    for index in (0..tail_length).rev() {
        buffer[match_start + after_replace_size + index] = buffer[tail_start + index];
    }

    buffer[match_start..match_start + new_length].copy_from_slice(new_text);
    if kept_newline == 1 {
        buffer[match_start + new_length] = b'\n';
    }

    *text_length = new_total;
    true
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

fn print_hex_dump(bytes: &[u8]) {
    if bytes.is_empty() {
        let _ = print!("(empty)\r\n");
        return;
    }

    for offset in (0..bytes.len()).step_by(16) {
        let line = &bytes[offset..bytes.len().min(offset + 16)];

        write_hex_u64(offset as u64);
        let _ = print!(": ");

        for (index, byte) in line.iter().enumerate() {
            if index == 8 {
                let _ = print!(" ");
            }
            write_hex_byte(*byte);
            let _ = print!(" ");
        }

        for _ in 0..(16 - line.len()) {
            let _ = print!("   ");
        }
        if line.len() <= 8 {
            let _ = print!(" ");
        }

        let _ = print!("|");
        for byte in line {
            let printable = *byte >= 0x20 && *byte <= 0x7e;
            let character = if printable { *byte as char } else { '.' };
            let _ = print!("{}", character);
        }
        let _ = print!("|\r\n");
    }
}

fn edit_file(path: &[u8], scratch: &mut [u8]) -> bool {
    let mut text_length = match syscall::read_file(path, scratch) {
        Some(count) => count,
        None => 0,
    };

    if text_length > 0 {
        let _ = print!("Current contents:\r\n");
        syscall::sys_write(&scratch[..text_length]);
        if !scratch[..text_length].ends_with(b"\n") {
            let _ = print!("\r\n");
        }
        let _ = print!("--- end of file ---\r\n");
    } else {
        let _ = print!("New file (empty)\r\n");
    }

    let mut line = [0u8; 128];

    loop {
        let _ = print!("edit> ");
        let line_length = read_line(&mut line);
        let command = strip_spaces(&line[..line_length]);

        if command.is_empty() {
            if text_length + 1 <= scratch.len() {
                scratch[text_length] = b'\n';
                text_length += 1;
            } else {
                let _ = print!("edit: buffer full\r\n");
            }
            continue;
        }

        match command {
            b":help" => {
                let _ = print!("Command summary: :view, :replace N text, :w save, :q quit, :wq save+quit\r\n");
            }
            b":view" | b":list" => {
                if text_length > 0 {
                    syscall::sys_write(&scratch[..text_length]);
                    if !scratch[..text_length].ends_with(b"\n") {
                        let _ = print!("\r\n");
                    }
                } else {
                    let _ = print!("(empty)\r\n");
                }
            }
            _ if command.starts_with(b":replace ") => {
                let rest = strip_spaces(&command[9..]);
                let separator = match rest.iter().position(|&byte| byte == b' ') {
                    Some(index) => index,
                    None => {
                        let _ = print!("Usage: :replace N text\r\n");
                        continue;
                    }
                };

                let line_number = match parse_usize(&rest[..separator]) {
                    Some(value) => value,
                    None => {
                        let _ = print!("replace: line number must be numeric\r\n");
                        continue;
                    }
                };

                let new_text = strip_spaces(&rest[separator + 1..]);

                if replace_line(scratch, &mut text_length, line_number, new_text) {
                    let _ = print!("Line updated.\r\n");
                } else {
                    let _ = print!("replace: bad line number or buffer too small\r\n");
                }
            }
            _ if command.starts_with(b":edit ") => {
                let rest = strip_spaces(&command[6..]);
                let separator = match rest.iter().position(|&byte| byte == b' ') {
                    Some(index) => index,
                    None => {
                        let _ = print!("Usage: :edit N text\r\n");
                        continue;
                    }
                };

                let line_number = match parse_usize(&rest[..separator]) {
                    Some(value) => value,
                    None => {
                        let _ = print!("edit: line number must be numeric\r\n");
                        continue;
                    }
                };

                let new_text = strip_spaces(&rest[separator + 1..]);

                if replace_line(scratch, &mut text_length, line_number, new_text) {
                    let _ = print!("Line updated.\r\n");
                } else {
                    let _ = print!("edit: bad line number or buffer too small\r\n");
                }
            }
            b":w" | b":write" => {
                match syscall::write_file(path, &scratch[..text_length]) {
                    Some(count) => {
                        let _ = print!("Saved ");
                        write_number(count as u64);
                        let _ = print!(" bytes to ");
                        syscall::sys_write(path);
                        let _ = print!("\r\n");
                    }
                    None => {
                        let _ = print!("edit: save failed\r\n");
                    }
                }
            }
            b":q" | b":quit" => {
                let _ = print!("Edit cancelled.\r\n");
                return false;
            }
            b":wq" | b":writequit" => {
                match syscall::write_file(path, &scratch[..text_length]) {
                    Some(count) => {
                        let _ = print!("Saved ");
                        write_number(count as u64);
                        let _ = print!(" bytes to ");
                        syscall::sys_write(path);
                        let _ = print!("\r\n");
                        return true;
                    }
                    None => {
                        let _ = print!("edit: save failed\r\n");
                    }
                }
            }
            _ => {
                if text_length + command.len() + 1 > scratch.len() {
                    let _ = print!("edit: buffer full\r\n");
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
    let _ = print!("Welcome to Ouroboros!\r\n");
    const FILE_BUFFER_PAGES: usize = 12;
    const FILE_BUFFER_SIZE: usize = FILE_BUFFER_PAGES * 4096;

    // Reuse this buffer for file operations instead of allocating
    // memory every time someone runs cat.
    let file_buffer = syscall::alloc_pages(FILE_BUFFER_PAGES);

    if file_buffer.is_none() {
        let _ = print!("Warning: filesystem buffer allocation failed.\r\n");
    }

    let mut line = [0u8; 128];
    let mut ignore_next_lf = false;

    loop {
        let _ = print!("ouro> ");
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
                    let _ = print!("\r\n");
                    break;
                }

                b'\n' => {
                    let _ = print!("\r\n");
                    break;
                }

                8 | 127 => {
                    if length > 0 {
                        length -= 1;
                        let _ = print!("\x08 \x08");
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
            let _ = print!("Commands: help, stat, cat, ls, df, touch, write,\r\n\
                  \x20         rm, mkdir, rmdir, run, edit, hex\r\n\
                  Paths must be absolute.\r\n");
        }

        // ----------------------------------------------------
        // df
        // ----------------------------------------------------
        else if command == b"df" || command.starts_with(b"df ") {
            let usage = match syscall::disk_usage() {
                Some(usage) => usage,
                None => {
                    let _ = print!("df: filesystem query failed\r\n");
                    continue;
                }
            };

            let total_kib = usage.total_blocks.saturating_mul(usage.block_size) / 1024;
            let used_kib = usage.used_blocks.saturating_mul(usage.block_size) / 1024;
            let available_kib = usage.available_blocks.saturating_mul(usage.block_size) / 1024;
            let used_percent = if total_kib == 0 {
                0
            } else {
                (used_kib * 100) / total_kib
            };

            let _ = print!("Filesystem     1K-blocks  Used Available Use% Mounted on\r\n");
            let _ = print!("rootfs         ");
            write_number(total_kib);
            let _ = print!("  ");
            write_number(used_kib);
            let _ = print!("  ");
            write_number(available_kib);
            let _ = print!("  ");
            write_number(used_percent);
            let _ = print!("% /\r\n");
        }

        // ----------------------------------------------------
        // hex <path>
        // ----------------------------------------------------
        else if command.starts_with(b"hex ") {
            let path = strip_spaces(&command[4..]);

            if path.is_empty() {
                let _ = print!("Usage: hex /path\r\n");
                continue;
            }

            let buffer = match file_buffer {
                Some(ptr) => unsafe {
                    core::slice::from_raw_parts_mut(ptr, FILE_BUFFER_SIZE)
                },
                None => {
                    let _ = print!("hex: buffer allocation failed\r\n");
                    continue;
                }
            };

            match syscall::read_file(path, buffer) {
                Some(count) => {
                    print_hex_dump(&buffer[..count]);
                }
                None => {
                    let _ = print!("hex: read failed\r\n");
                }
            }
        }

        // ----------------------------------------------------
        // edit <path>
        // ----------------------------------------------------
        else if command.starts_with(b"edit ") {
            let path = strip_spaces(&command[5..]);

            if path.is_empty() {
                let _ = print!("Usage: edit /path\r\n");
                continue;
            }

            let buffer = match file_buffer {
                Some(ptr) => unsafe {
                    core::slice::from_raw_parts_mut(ptr, FILE_BUFFER_SIZE)
                },
                None => {
                    let _ = print!("edit: buffer allocation failed\r\n");
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
                    let _ = print!("inode: ");
                    write_number(info.inode);

                    let _ = print!("\r\nsize: ");
                    write_number(info.size);

                    let _ = print!("\r\ntype: ");

                    if info.kind == syscall::FILE_KIND_FILE {
                        let _ = print!("file");
                    } else if info.kind == syscall::FILE_KIND_DIRECTORY {
                        let _ = print!("directory");
                    } else {
                        let _ = print!("unknown");
                    }

                    let _ = print!("\r\nlinks: ");
                    write_number(info.links);
                    let _ = print!("\r\n");
                }

                None => {
                    let _ = print!("stat: operation failed\r\n");
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
                    let _ = print!("cat: no filesystem buffer\r\n");
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
                            let _ = print!("\r\n");
                        }
                    } else {
                        let _ = print!("\r\n");
                    }
                }

                None => {
                    let _ = print!("cat: read failed; file may be missing \
                          or too large\r\n");
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
                    let _ = print!("touch: no filesystem buffer\r\n");
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
                    let _ = print!("Created inode ");
                    write_number(inode as u64);
                    let _ = print!("\r\n");
                }

                None => {
                    let _ = print!("touch: create failed; path may exist\r\n");
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
                            let _ = print!("Wrote ");
                            write_number(count as u64);
                            let _ = print!(" bytes\r\n");
                        }

                        None => {
                            let _ = print!("write: operation failed\r\n");
                        }
                    }
                }

                _ => {
                    let _ = print!("Usage: write /path contents\r\n");
                }
            }
        }

        // ----------------------------------------------------
        // rm <path>
        // ----------------------------------------------------
        else if command.starts_with(b"rm ") {
            let path = &command[3..];

            if syscall::delete_file(path) {
                let _ = print!("File deleted.\r\n");
            } else {
                let _ = print!("rm: deletion failed\r\n");
            }
        }

        // ----------------------------------------------------
        // mkdir <path>
        // ----------------------------------------------------
        else if command.starts_with(b"mkdir ") {
            let path = &command[6..];

            match syscall::create_dir(path) {
                Some(inode) => {
                    let _ = print!("Created directory inode ");
                    write_number(inode as u64);
                    let _ = print!("\r\n");
                }

                None => {
                    let _ = print!("mkdir: operation failed\r\n");
                }
            }
        }

        // ----------------------------------------------------
        // rmdir <path>
        // ----------------------------------------------------
        else if command.starts_with(b"rmdir ") {
            let path = &command[6..];

            if syscall::remove_dir(path) {
                let _ = print!("Directory removed.\r\n");
            } else {
                let _ = print!("rmdir: removal failed; directory may not be empty\r\n");
            }
        }

        // ----------------------------------------------------
        // run <path>
        // ----------------------------------------------------
        else if command.starts_with(b"run ") {
            let path = &command[4..];
            if path == b"/bin/user-init.run"{let _ = print!("You should NOT be trying to open a nested shell.\r\n"); continue;}
            match syscall::spawn(path) {
                Some(task_id) => {
                    let _ = print!("Completed task ");
                    write_number(task_id as u64);
                    let _ = print!("\r\n");
                }

                None => {
                    let _ = print!("run: executable could not be started\r\n");
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
                    let _ = print!("ls: file buffer allocation failed\n");
                    continue;
                }
            };

            match syscall::list_directory(path, buffer) {
                Some(count) => {
                    syscall::sys_write(&buffer[..count]);
                }
                None => {
                    let _ = print!("ls: cannot list directory (missing path or buffer too small)\n");
                }
            }
        }
        // ----------------------------------------------------
        // Unknown command
        // ----------------------------------------------------
        else {
            let _ = print!("Unknown command: ");
            syscall::sys_write(command);
            let _ = print!("\r\n");
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