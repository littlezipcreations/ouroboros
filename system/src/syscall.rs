#[warn(dead_code)]
#[warn(non_snake_case)]
use core::arch::asm;

/// Gets the system tick count.
#[inline(always)]
pub fn tick_count() -> Result<u64> {
	let result: u64;

	unsafe {
		asm!(
			"svc #16",
			lateout("x0") result,
		);
	}

	super::syscall::decodeResult(result)
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FileStat {
	pub inode: u64,
	pub size: u64,
	pub kind: u8,
	pub links: u64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FilesystemUsage {
	pub total_blocks: u64,
	pub used_blocks: u64,
	pub available_blocks: u64,
	pub block_size: u64,
}

pub const FILE_KIND_FILE: u8 = 1;
pub const FILE_KIND_DIRECTORY: u8 = 2;

/// Errors returned by the Ouroboros kernel.
#[repr(u64)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
	NotFound = 1,
	Unsupported = 2,
	InvalidArgument = 3,
	ResourceUnavailable = 4,
	PermissionDenied = 5,
	AlreadyExists = 6,
	NotDirectory = 7,
	IsDirectory = 8,
	DirectoryNotEmpty = 9,
	BufferTooSmall = 10,
	InvalidAddr = 11,
	OoM = 12,
	IoErr = 13,
	WouldBlock = 14,
	Unknown = 255,
}

pub type Result<T> = core::result::Result<T, Error>;

/// Converts the kernel's syscall return value into a userspace result.
///
/// Successful return values are non-negative.
/// Errors are returned as negative error codes in x0.
#[inline(always)]
fn decodeResult(value: u64) -> Result<u64> {
	let signedValue = value as i64;

	if signedValue >= 0 {
		return Ok(value);
	}

	let code = signedValue.unsigned_abs();

	let error = match code {
		1 => Error::NotFound,
		2 => Error::Unsupported,
		3 => Error::InvalidArgument,
		4 => Error::ResourceUnavailable,
		5 => Error::PermissionDenied,
		6 => Error::AlreadyExists,
		7 => Error::NotDirectory,
		8 => Error::IsDirectory,
		9 => Error::DirectoryNotEmpty,
		10 => Error::BufferTooSmall,
		11 => Error::InvalidAddr,
		12 => Error::OoM,
		13 => Error::IoErr,
		14 => Error::WouldBlock,
		255 => Error::Unknown,
		_ => Error::Unknown,
	};

	Err(error)
}

#[inline(always)]
pub fn test() {
	unsafe {
		asm!("svc #42");
	}
}

#[inline(always)]
pub fn yield_now() {
	unsafe {
		asm!("svc #2");
	}
}

#[inline(always)]
pub fn exit() -> ! {
	unsafe {
		asm!("svc #1", options(noreturn));
	}
}

/// Writes bytes to the standard output.
#[inline(always)]
pub fn sys_write(buffer: &[u8]) -> Result<usize> {
	let result: u64;

	unsafe {
		asm!(
			"svc #3",
			in("x0") buffer.as_ptr(),
			in("x1") buffer.len(),
			lateout("x0") result,
		);
	}

	decodeResult(result).map(|bytesWritten| bytesWritten as usize)
}

/// Requests `pages` consecutive heap pages.
///
/// Returns the first userspace virtual address on success.
#[inline(always)]
pub fn alloc_pages(pages: usize) -> Result<*mut u8> {
	let mut address = pages as u64;

	unsafe {
		asm!(
			"svc #4",
			inout("x0") address,
		);
	}

	decodeResult(address).and_then(|address| {
		if address == 0 {
			Err(Error::ResourceUnavailable)
		} else {
			Ok(address as *mut u8)
		}
	})
}

/// Reads a byte from the kernel-provided input source.
#[inline(always)]
pub fn read_byte() -> Result<u8> {
	let result: u64;

	unsafe {
		asm!(
			"svc #5",
			lateout("x0") result,
		);
	}

	decodeResult(result).map(|value| value as u8)
}

/// Retrieves metadata about a filesystem path.
#[inline(always)]
pub fn stat(path: &[u8]) -> Result<FileStat> {
	let inode: u64;
	let size: u64;
	let kind: u64;
	let links: u64;

	unsafe {
		asm!(
			"svc #10",
			in("x0") path.as_ptr(),
			in("x1") path.len(),
			lateout("x0") inode,
			lateout("x1") size,
			lateout("x2") kind,
			lateout("x3") links,
		);
	}

	decodeResult(inode).map(|inode| FileStat {
		inode,
		size,
		kind: kind as u8,
		links,
	})
}

/// Creates a file and returns its inode.
#[inline(always)]
pub fn create_file(path: &[u8], data: &[u8]) -> Result<u32> {
	let inode: u64;

	unsafe {
		asm!(
			"svc #6",
			in("x0") path.as_ptr(),
			in("x1") path.len(),
			in("x2") data.as_ptr(),
			in("x3") data.len(),
			lateout("x0") inode,
		);
	}

	decodeResult(inode).map(|inode| inode as u32)
}

#[inline(always)]
pub fn create_file_path(path: &[u8], data: &[u8]) -> Result<u32> {
	create_file(path, data)
}

/// Reads a file into the supplied buffer.
///
/// Returns the number of bytes read.
#[inline(always)]
pub fn read_file(path: &[u8], buffer: &mut [u8]) -> Result<usize> {
	let count: u64;

	unsafe {
		asm!(
			"svc #7",
			in("x0") path.as_ptr(),
			in("x1") path.len(),
			in("x2") buffer.as_mut_ptr(),
			in("x3") buffer.len(),
			lateout("x0") count,
		);
	}

	decodeResult(count).map(|count| count as usize)
}

#[inline(always)]
pub fn read_file_path(path: &[u8], buffer: &mut [u8]) -> Result<usize> {
	read_file(path, buffer)
}

/// Writes data to a file.
///
/// Returns the number of bytes written.
#[inline(always)]
pub fn write_file(path: &[u8], data: &[u8]) -> Result<usize> {
	let count: u64;

	unsafe {
		asm!(
			"svc #8",
			in("x0") path.as_ptr(),
			in("x1") path.len(),
			in("x2") data.as_ptr(),
			in("x3") data.len(),
			lateout("x0") count,
		);
	}

	decodeResult(count).map(|count| count as usize)
}

#[inline(always)]
pub fn write_file_path(path: &[u8], data: &[u8]) -> Result<usize> {
	write_file(path, data)
}

/// Deletes a file.
#[inline(always)]
pub fn delete_file(path: &[u8]) -> Result<()> {
	let result: u64;

	unsafe {
		asm!(
			"svc #9",
			in("x0") path.as_ptr(),
			in("x1") path.len(),
			lateout("x0") result,
		);
	}

	decodeResult(result).map(|_| ())
}

#[inline(always)]
pub fn delete_file_path(path: &[u8]) -> Result<()> {
	delete_file(path)
}

/// Creates a directory and returns its inode.
#[inline(always)]
pub fn create_dir(path: &[u8]) -> Result<u32> {
	let inode: u64;

	unsafe {
		asm!(
			"svc #11",
			in("x0") path.as_ptr(),
			in("x1") path.len(),
			lateout("x0") inode,
		);
	}

	decodeResult(inode).map(|inode| inode as u32)
}

#[inline(always)]
pub fn mkdir(path: &[u8]) -> Result<u32> {
	create_dir(path)
}

/// Removes an empty directory.
#[inline(always)]
pub fn remove_dir(path: &[u8]) -> Result<()> {
	let result: u64;

	unsafe {
		asm!(
			"svc #12",
			in("x0") path.as_ptr(),
			in("x1") path.len(),
			lateout("x0") result,
		);
	}

	decodeResult(result).map(|_| ())
}

#[inline(always)]
pub fn rmdir(path: &[u8]) -> Result<()> {
	remove_dir(path)
}

/// Spawns a userspace task and returns its task ID.
#[inline(always)]
pub fn spawn(path: &[u8]) -> Result<usize> {
	let taskId: u64;

	unsafe {
		asm!(
			"svc #13",
			in("x0") path.as_ptr(),
			in("x1") path.len(),
			lateout("x0") taskId,
		);
	}

	decodeResult(taskId).map(|taskId| taskId as usize)
}

#[inline(always)]
pub fn create_task(path: &[u8]) -> Result<usize> {
	spawn(path)
}

/// Lists a directory into the supplied buffer.
///
/// Returns the number of bytes written to the buffer.
#[inline(always)]
pub fn list_directory(
	path: &[u8],
	buffer: &mut [u8],
) -> Result<usize> {
	let result: u64;

	unsafe {
		asm!(
			"svc #14",
			in("x0") path.as_ptr(),
			in("x1") path.len(),
			in("x2") buffer.as_mut_ptr(),
			in("x3") buffer.len(),
			lateout("x0") result,
		);
	}

	decodeResult(result).map(|count| count as usize)
}

/// Retrieves filesystem usage statistics.
#[inline(always)]
pub fn disk_usage() -> Result<FilesystemUsage> {
	let total: u64;
	let used: u64;
	let available: u64;
	let blockSize: u64;

	unsafe {
		asm!(
			"svc #15",
			lateout("x0") total,
			lateout("x1") used,
			lateout("x2") available,
			lateout("x3") blockSize,
		);
	}

	decodeResult(total).map(|total| FilesystemUsage {
		total_blocks: total,
		used_blocks: used,
		available_blocks: available,
		block_size: blockSize,
	})
}