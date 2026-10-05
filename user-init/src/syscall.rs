use core::arch::asm;

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
