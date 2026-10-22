#![warn(dead_code)]
use core::arch::asm;

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FileStat {
    pub inode: u64,
    pub size: u64,
    pub kind: u8,
    pub links: u64,
}

pub const FILE_KIND_FILE: u8 = 1;
pub const FILE_KIND_DIRECTORY: u8 = 2;

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

#[inline(always)]
pub fn sys_write(buffer: &[u8]) -> isize {
    let result: isize;

    unsafe {
        asm!(
            "svc #3",
            in("x0") buffer.as_ptr(),
            in("x1") buffer.len(),
            lateout("x0") result,
        );
    }

    result
}

/// Ask the kernel for `pages` consecutive heap pages.
///
/// Returns the first userspace virtual address, or `None` if
/// the kernel could not satisfy the request.
#[inline(always)]
pub fn alloc_pages(pages: usize) -> Option<*mut u8> {
    let mut address = pages as u64;

    unsafe {
        asm!(
            "svc #4",
            inout("x0") address,
        );
    }

    if address == 0 {
        None
    } else {
        Some(address as *mut u8)
    }
}

pub fn read_byte() -> Option<u8>{
    let result: u64;
    unsafe {
        asm!("svc #5", lateout("x0") result,);
    }
    if result == u64::MAX{None} else{Some(result as u8)}
}

#[inline(always)]
pub fn stat(path: &[u8]) -> Option<FileStat> {
    let result: u64;
    let size: u64;
    let kind: u64;
    let links: u64;

    unsafe {
        asm!(
            "svc #10",
            in("x0") path.as_ptr(),
            in("x1") path.len(),
            lateout("x0") result,
            lateout("x1") size,
            lateout("x2") kind,
            lateout("x3") links,
        );
    }

    if result == u64::MAX {
        None
    } else {
        Some(FileStat {
            inode: result,
            size,
            kind: kind as u8,
            links,
        })
    }
}

#[inline(always)]
pub fn create_file(path: &[u8], data: &[u8]) -> Option<u32> {
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

    if inode == u64::MAX { None } else { Some(inode as u32) }
}

#[inline(always)]
pub fn create_file_path(path: &[u8], data: &[u8]) -> Option<u32> {
    create_file(path, data)
}

#[inline(always)]
pub fn read_file(path: &[u8], buffer: &mut [u8]) -> Option<usize> {
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

    if count == u64::MAX { None } else { Some(count as usize) }
}

#[inline(always)]
pub fn read_file_path(path: &[u8], buffer: &mut [u8]) -> Option<usize> {
    read_file(path, buffer)
}

#[inline(always)]
pub fn write_file(path: &[u8], data: &[u8]) -> Option<usize> {
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

    if count == u64::MAX { None } else { Some(count as usize) }
}

#[inline(always)]
pub fn write_file_path(path: &[u8], data: &[u8]) -> Option<usize> {
    write_file(path, data)
}

#[inline(always)]
pub fn delete_file(path: &[u8]) -> bool {
    let result: u64;

    unsafe {
        asm!(
            "svc #9",
            in("x0") path.as_ptr(),
            in("x1") path.len(),
            lateout("x0") result,
        );
    }

    result != u64::MAX
}

#[inline(always)]
pub fn delete_file_path(path: &[u8]) -> bool {
    delete_file(path)
}

#[inline(always)]
pub fn create_dir(path: &[u8]) -> Option<u32> {
    let inode: u64;

    unsafe {
        asm!(
            "svc #11",
            in("x0") path.as_ptr(),
            in("x1") path.len(),
            lateout("x0") inode,
        );
    }

    if inode == u64::MAX { None } else { Some(inode as u32) }
}

#[inline(always)]
pub fn mkdir(path: &[u8]) -> Option<u32> {
    create_dir(path)
}

#[inline(always)]
pub fn remove_dir(path: &[u8]) -> bool {
    let result: u64;

    unsafe {
        asm!(
            "svc #12",
            in("x0") path.as_ptr(),
            in("x1") path.len(),
            lateout("x0") result,
        );
    }

    result != u64::MAX
}

#[inline(always)]
pub fn rmdir(path: &[u8]) -> bool {
    remove_dir(path)
}

#[inline(always)]
pub fn spawn(path: &[u8]) -> Option<usize> {
    let task_id: u64;

    unsafe {
        asm!(
            "svc #13",
            in("x0") path.as_ptr(),
            in("x1") path.len(),
            lateout("x0") task_id,
        );
    }

    if task_id == u64::MAX {
        None
    } else {
        Some(task_id as usize)
    }
}

#[inline(always)]
pub fn create_task(path: &[u8]) -> Option<usize> {
    spawn(path)
}

#[inline(always)]
pub fn list_directory(
    path: &[u8],
    buffer: &mut [u8],
) -> Option<usize> {
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

    if result == u64::MAX {
        None
    } else {
        Some(result as usize)
    }
}

