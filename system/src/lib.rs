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
	graphics_blit,
	graphics_present,
};

pub mod graphics;

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
	use core::sync::atomic::{AtomicUsize, Ordering};

	use crate::{Error, Result};

	const SCREEN_WIDTH: usize = 1280;
	const SCREEN_HEIGHT: usize = 800;

	const CELL_WIDTH: usize = 6;
	const CELL_HEIGHT: usize = 9;

	static CURSOR_X: AtomicUsize = AtomicUsize::new(0);
	static CURSOR_Y: AtomicUsize = AtomicUsize::new(0);

	/// Draws one character cell onto the framebuffer.
	fn draw_cell(x: usize, y: usize, character: char) -> Result<()> {
		let mut pixels = [0xFF00_0000u32; CELL_WIDTH * CELL_HEIGHT];

		{
			let mut surface = crate::graphics::Surface::new(
				CELL_WIDTH,
				CELL_HEIGHT,
				&mut pixels,
			).ok_or(Error::InvalidArgument)?;

			surface.fill_rect(
				0,
				0,
				CELL_WIDTH as i32,
				CELL_HEIGHT as i32,
				0xFF00_0000,
			);

			surface.draw_char(
				0,
				1,
				character,
				0xFFFF_FFFF,
			);
		}

		crate::syscall::graphics_blit(
			x,
			y,
			CELL_WIDTH,
			CELL_HEIGHT,
			&pixels,
		)
	}

	/// Writes bytes to the graphical console.
	pub fn write_bytes(bytes: &[u8]) -> Result<usize> {
		let mut cursor_x = CURSOR_X.load(Ordering::Relaxed);
		let mut cursor_y = CURSOR_Y.load(Ordering::Relaxed);

		let mut changed = false;

		for &byte in bytes {
			match byte {
				b'\r' => {
					cursor_x = 0;
				}

				b'\n' => {
					cursor_x = 0;
					cursor_y += CELL_HEIGHT;

					if cursor_y + CELL_HEIGHT > SCREEN_HEIGHT {
						cursor_y = 0;
					}
				}

				8 | 127 => {
					if cursor_x >= CELL_WIDTH {
						cursor_x -= CELL_WIDTH;
						draw_cell(cursor_x, cursor_y, ' ')?;
						changed = true;
					}
				}

				b' '..=b'~' => {
					if cursor_x + CELL_WIDTH > SCREEN_WIDTH {
						cursor_x = 0;
						cursor_y += CELL_HEIGHT;
					}

					if cursor_y + CELL_HEIGHT > SCREEN_HEIGHT {
						cursor_y = 0;
					}

					draw_cell(
						cursor_x,
						cursor_y,
						byte as char,
					)?;

					cursor_x += CELL_WIDTH;
					changed = true;
				}

				_ => {}
			}
		}

		CURSOR_X.store(cursor_x, Ordering::Relaxed);
		CURSOR_Y.store(cursor_y, Ordering::Relaxed);

		if changed {
			crate::syscall::graphics_present()?;
		}

		Ok(bytes.len())
	}

	/// Writes a string to the graphical console.
	pub fn write(text: &str) -> Result<()> {
		write_bytes(text.as_bytes()).map(|_| ())
	}

	/// Writes a string followed by a newline.
	pub fn write_line(text: &str) -> Result<()> {
		write(text)?;
		write("\n")
	}

	struct ConsoleWriter;

	impl fmt::Write for ConsoleWriter {
		fn write_str(&mut self, text: &str) -> fmt::Result {
			write(text).map_err(|_| fmt::Error)
		}
	}

	/// Formats and writes text to the graphical console.
	pub fn write_fmt(args: fmt::Arguments<'_>) -> Result<()> {
		fmt::write(&mut ConsoleWriter, args)
			.map_err(|_| Error::Unknown)
	}
}

#[macro_export]
macro_rules! print {
	($($arg:tt)*) => {
		$crate::console::write_fmt(
			core::format_args!($($arg)*)
		)
	};
}

#[macro_export]
macro_rules! println {
	() => {
		$crate::console::write_fmt(
			core::format_args!("\n")
		)
	};

	($($arg:tt)*) => {
		$crate::console::write_fmt(
			core::format_args!(
				"{}\n",
				core::format_args!($($arg)*)
			)
		)
	};
}

/// Reads a line from the input source.
///
/// Characters are echoed to the graphical console.
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
					let _ = print!("\x08");
				}
			}

			0x20..=0x7e => {
				if length < buffer.len() {
					buffer[length] = byte;
					length += 1;

					let _ = print!("{}", byte as char);
				}
			}

			_ => {}
		}
	}
}
