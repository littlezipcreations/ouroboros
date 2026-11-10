#![no_std]
#![warn(non_snake_case)]

pub mod syscall;

pub use syscall::{
	Error,
	Result,
	FileStat,
	FilesystemUsage,
	FILE_KIND_DIRECTORY,
	FILE_KIND_FILE,
	alloc_pages,
	create_dir,
	create_file,
	create_file_path,
	create_task,
	delete_file,
	delete_file_path,
	disk_usage,
	exit,
	list_directory,
	mkdir,
	read_byte,
	read_file,
	read_file_path,
	remove_dir,
	rmdir,
	spawn,
	stat,
	sys_write,
	tick_count,
	test,
	write_file,
	write_file_path,
	yield_now,
};
/// A result returned by the Ouroboros userspace library.
pub fn errorMessage(error: Error) -> &'static str {
	match error {
		Error::NotFound => "File not found",
		Error::Unsupported => "Operation not supported",
		Error::InvalidArgument => "Invalid argument",
		Error::ResourceUnavailable => "Resource unavailable",
		Error::PermissionDenied => "Permission denied",
		Error::AlreadyExists => "Already exists",
		Error::NotDirectory => "Not a directory",
		Error::IsDirectory => "Is a directory",
		Error::DirectoryNotEmpty => "Directory not empty",
		Error::BufferTooSmall => "Buffer too small",
		Error::InvalidAddr => "Invalid address",
		Error::OoM => "Out of memory",
		Error::IoErr => "I/O error",
		Error::WouldBlock => "Would block",
		Error::Unknown => "Unknown error",
	}
}
pub mod console {
    use core::fmt;
    use crate::{Error, Result};

    /// Writes all bytes of a string to the system console.
    pub fn write(text: &str) -> Result<()> {
        let bytes = text.as_bytes();
        let mut written = 0;

        while written < bytes.len() {
            let count = match super::syscall::sys_write(&bytes[written..]) {
                Ok(count) => count,
                Err(error) => return Err(error),
            };

            if count == 0 || count > bytes.len() - written {
                return Err(Error::IoErr);
            }

            written += count;
        }

        Ok(())
    }
    /// Writes a string followed by a newline.
    pub fn write_line(text: &str) -> Result<()> {
        write(text)?;
        write("\n")
    }
    struct ConsoleWriter;
    impl fmt::Write for ConsoleWriter{
        fn write_str(&mut self, text: &str) -> fmt::Result{
            write(text).map_err(|_| fmt::Error)
        }
    }
    pub fn write_fmt(args: fmt::Arguments<'_>) -> Result<()>{
        fmt::write(&mut ConsoleWriter, args).map_err(|_| crate::Error::Unknown)
    }

}

#[macro_export]
macro_rules! print{ //Thank you Google. I'll be damned if anyone can write a macro off the top of their head.
    ($($arg:tt)*) => {
        $crate::console::write_fmt(core::format_args!($($arg)*))
    };
}
#[macro_export]
macro_rules! println {
    () => {
        $crate::console::write_fmt(core::format_args!("\n"))
    };
    ($($arg:tt)*) => {
        $crate::console::write_fmt(core::format_args!("{}\n", core::format_args!($($arg)*)))
    };
}
pub fn read_line(buffer: &mut [u8]) -> usize {
    let mut length = 0usize;

    loop {
        let byte = match syscall::read_byte() {
            Ok(byte) => byte,
            Err(syscall::Error::WouldBlock) => {
                syscall::yield_now();
                continue;
            }
            Err(error) => {
                println!("{:?}", error);
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