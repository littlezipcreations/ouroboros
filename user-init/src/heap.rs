use core::alloc::{GlobalAlloc, Layout};
use core::cell::UnsafeCell;
use core::ptr::null_mut;

use crate::syscall::alloc_pages;

const PAGE_SIZE: usize = 4096;

struct HeapState {
    next: usize,
    end: usize,
}

pub struct BumpAllocator {
    state: UnsafeCell<HeapState>,
}

unsafe impl Sync for BumpAllocator {}

impl BumpAllocator {
    pub const fn new() -> Self {
        Self {
            state: UnsafeCell::new(HeapState {
                next: 0,
                end: 0,
            }),
        }
    }
}

unsafe impl GlobalAlloc for BumpAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let size = layout.size();
        let align = layout.align();

        if size == 0 {
            // Any non-null pointer satisfying the requested alignment
            // is valid for a zero-sized allocation.
            return align as *mut u8;
        }

        if size > isize::MAX as usize {
            return null_mut();
        }

        let state = unsafe {
            &mut *self.state.get()
        };

        // Try to satisfy the allocation from pages we already own.
        let aligned = match state.next.checked_add(align - 1) {
            Some(value) => value & !(align - 1),
            None => return null_mut(),
        };

        let end = match aligned.checked_add(size) {
            Some(value) => value,
            None => return null_mut(),
        };

        if state.next != 0 && end <= state.end {
            state.next = end;
            return aligned as *mut u8;
        }

        // Ask the kernel for enough new pages to satisfy this
        // allocation, including worst-case alignment padding.
        let required = match size.checked_add(align - 1) {
            Some(value) => value,
            None => return null_mut(),
        };

        let pages = match required.checked_add(PAGE_SIZE - 1) {
            Some(value) => value / PAGE_SIZE,
            None => return null_mut(),
        };

        let base = match alloc_pages(pages) {
            Some(address) => address as usize,
            None => return null_mut(),
        };

        let aligned = match base.checked_add(align - 1) {
            Some(value) => value & !(align - 1),
            None => return null_mut(),
        };

        let end = match aligned.checked_add(size) {
            Some(value) => value,
            None => return null_mut(),
        };

        state.next = end;
        state.end = match base.checked_add(pages * PAGE_SIZE) {
            Some(value) => value,
            None => return null_mut(),
        };

        aligned as *mut u8
    }

    unsafe fn dealloc(
        &self,
        _ptr: *mut u8,
        _layout: Layout,
    ) {
        // Deliberately a bump allocator for tonight.
        // Memory is reclaimed when the process is eventually replaced.
    }
}

#[global_allocator]
pub static ALLOCATOR: BumpAllocator = BumpAllocator::new();
