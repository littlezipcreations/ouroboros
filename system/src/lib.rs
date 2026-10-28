#![no_std]
use core::fmt;

pub mod syscall;

pub use syscall::{
    FileStat,
    FILE_KIND_DIRECTORY,
    FILE_KIND_FILE,
    alloc_pages,
    create_dir,
    create_file,
    create_file_path,
    create_task,
    delete_file,
    delete_file_path,
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
    test,
    write_file,
    write_file_path,
    yield_now,
};
/// Errors returned by the Ouroboros userspace library.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    /// The requested file or resource does not exist.
    NotFound,

    /// The operation is not supported.
    Unsupported,

    /// The supplied arguments are invalid.
    InvalidArgument,

    /// There is insufficient memory or another resource is unavailable.
    ResourceUnavailable,

    /// The operation failed for another reason.
    Unknown,
}

/// A result returned by the Ouroboros userspace library.
pub type Result<T> = core::result::Result<T, Error>;
mod console {
    use crate::{Error, Result};

    /// Writes all bytes of a string to the system console.
    pub fn write(text: &str) -> Result<()> {
        let bytes = text.as_bytes();
        let mut written = 0;

        while written < bytes.len() {
            let count = super::syscall::sys_write(&bytes[written..]);

            if count <= 0 || count as usize > bytes.len() - written {
                return Err(Error::Unknown);
            }

            written += count as usize;
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
        fmt::write(&mut ConsoleWriter, args).map_err(|_| Error::Unknown)
    }

}

#[macro_export]
macro_rules! print{ //Thank you Google. I'll be damned if anyone can write a macro off the top of their head.
    ($($arg:tt)*) => {
        $crate::console::write_fmt(core::format_arges!($($arg)*))
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