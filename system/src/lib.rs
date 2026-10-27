#![no_std]

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
pub mod console {
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
}