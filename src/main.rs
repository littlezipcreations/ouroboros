#![no_std]
#![no_main]
#![allow(warnings)]
mod exception;
use exception::ExceptionFrame;
mod virtio;
use virtio::VirtioBlk;
mod virtio_gpu;
use virtio_gpu::VirtioGpu;
mod virtio_input;
use virtio_input::{
    VirtioInput,
    EV_KEY,
    EV_REL,
    REL_X,
    REL_Y,
    BTN_LEFT,
    BTN_RIGHT,
};
mod filesystem;
core::arch::global_asm!(include_str!("boot.S"));
core::arch::global_asm!(include_str!("exceptions.S"));
use core::fmt::{self, Write};
use core::ops::Add;
use core::panic::PanicInfo;
use core::sync::atomic::{AtomicU64, Ordering};
static TICKS: AtomicU64 = AtomicU64::new(0);
// ============================================================
// UART
// ============================================================
struct Uart {
    base: usize,
}
impl Uart {
    const fn new(base: usize) -> Self {
        Self { base }
    }
    fn put_byte(&self, byte: u8) {
        unsafe {
            let base = self.base as *mut u32;

            // Wait while the transmit FIFO is full.
            while base.add(0x18 / 4).read_volatile() & (1 << 5) != 0 {}

            base.write_volatile(byte as u32);
        }
    }
    fn write_bytes(&self, s: &str) {
        for &byte in s.as_bytes() {
            self.put_byte(byte);
        }
    }
    fn try_get_byte(&self) -> Option<u8>{
        unsafe{
            let base = self.base as *mut u32;
            let flags = base.add(0x18 / 4).read_volatile();
            if flags & (1 << 4) != 0{
                None
            } else {
                Some(base.read_volatile() as u8)
            }
        }
    }
}
impl Write for Uart {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        self.write_bytes(s);
        Ok(())
    }
}
// ============================================================
// Exception vector
// ============================================================
unsafe extern "C" {
    fn exception_vector();
}
// ============================================================
// Kernel stack
// ============================================================
#[repr(align(16))]
struct KernelStack([u8; 16 * 1024]);
#[unsafe(no_mangle)]
#[unsafe(link_section = ".kernel_stack")]
static mut STACK: KernelStack = KernelStack([0; 16 * 1024]);
// ============================================================
// Tasks
// ============================================================
const TASK_STACK_SIZE: usize = 16 * 1024;
const MAX_TASKS: usize = 64;
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq)]
enum TaskState {
    Ready,
    Running,
    Dead,
    Unused,
    Blocked,
}
#[repr(C)]
struct CpuContext {
    x: [u64; 31],
    sp: u64,
    pc: u64,
    spsr: u64,
    kernel_sp: u64,
}
#[repr(C, align(4096))]
#[derive(Clone, Copy)]
struct AddressSpace {
    l0: PageTable,
    l1: PageTable,
    l2_device: PageTable,
    l2_ram: PageTable,
    l2_vmap: PageTable,
    l3_ram: PageTable,
    l3_vmap: PageTable,
}
impl AddressSpace {
    const fn new() -> Self {
        Self{ 
            l0: PageTable::new(),
            l1: PageTable::new(),
            l2_device: PageTable::new(),
            l2_ram: PageTable::new(),
            l2_vmap: PageTable::new(),
            l3_ram: PageTable::new(),
            l3_vmap: PageTable::new(),
        }
    }
    unsafe fn init(&mut self){
        self.l0.entries[0] = PageTableEntry::new_table(&raw const self.l1);
        self.l1.entries[0] = PageTableEntry::new_table(&raw const self.l2_device);
        self.l1.entries[1] = PageTableEntry::new_table(&raw const self.l2_ram);
        self.l1.entries[2] = PageTableEntry::new_table(&raw const self.l2_vmap);
        self.l2_device.entries[64] = PageTableEntry::new_block(0x0800_0000, ATTR_DEVICE, 0b00, 0b10, true);
        self.l2_device.entries[72] = PageTableEntry::new_block(0x0900_0000, ATTR_DEVICE, 0b00, 0b10, true);
        self.l2_device.entries[80] = PageTableEntry::new_block(0x0A00_0000, ATTR_DEVICE, 0b00, 0b10, true); 
        self.l2_ram.entries[0] = PageTableEntry::new_table(&raw const self.l3_ram);
        for i in 1..256{
            self.l2_ram.entries[i] = PageTableEntry::new_block(0x4000_0000 + i * 0x20_0000, ATTR_NORMAL, 0b00, 0b11, false);
        }
        self.l2_vmap.entries[0] = PageTableEntry::new_table(&raw const self.l3_vmap);
        for i in 0..512{
            self.l3_ram.entries[i] = PageTableEntry::new_page(0x4000_0000 + i * PAGE_SIZE, ATTR_NORMAL, 0b00, 0b11, false);
        }
    }

}
#[unsafe(no_mangle)]
#[unsafe(link_section = ".address_spaces")]
static mut ADDRESS_SPACES: [AddressSpace; MAX_TASKS] = [const { AddressSpace::new()}; MAX_TASKS];
static mut CURRENT_ASID: usize = 0;
#[repr(C)]
struct Task {
    id: usize,
    state: TaskState,
    context: CpuContext,
    address_space_id: usize,
}
const SPSR_EL0T: u64 = 0b0000;
const SPSR_EL1H: u64 = 0b0101;
#[repr(align(16))]
#[derive(Copy, Clone)]
struct TaskStack([u8; TASK_STACK_SIZE]);
#[unsafe(no_mangle)]
#[unsafe(link_section = ".task_stacks")]
static mut TASK_STACKS: [TaskStack; MAX_TASKS] =
    [TaskStack([0; TASK_STACK_SIZE]); MAX_TASKS];
const USER_STACK_TOP: u64 = 0x0000_0000_8020_0000;
const USER_STACK_SIZE: usize = 16 * 1024;
const USER_CODE_BASE: usize = 0x8000_0000;
const USER_HEAP_START: usize = 0x8001_0000;
const USER_HEAP_END: usize = USER_STACK_TOP as usize - USER_STACK_SIZE;
static mut USER_HEAP_NEXT: [usize; MAX_TASKS] = [USER_HEAP_START; MAX_TASKS];

// OuroFS currently supports files using 12 direct data blocks.
const MAX_RUN_SIZE: usize = 12 * 4096;

// Static storage keeps the executable buffer off the kernel stack.
static mut RUN_LOAD_BUFFER: [u8; MAX_RUN_SIZE] = [0; MAX_RUN_SIZE];

// ============================================================
// Ouroboros .run executable format
// ============================================================
const RUN_MAGIC: &[u8; 4] = b"RUN1";
const RUN_VERSION: u16 = 1;
const RUN_ARCH_AARCH64: u16 = 1;
const RUN_HEADER_SIZE: usize = 32;
const RUN_SEGMENT_SIZE: usize = 48;
const RUN_MAX_SEGMENTS: usize = 16;

const RUN_R: u32 = 1;
const RUN_W: u32 = 2;
const RUN_X: u32 = 4;

fn run_read_u16(data: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([
        data[offset],
        data[offset + 1],
    ])
}

fn run_read_u32(data: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([
        data[offset],
        data[offset + 1],
        data[offset + 2],
        data[offset + 3],
    ])
}

fn run_read_u64(data: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes([
        data[offset],
        data[offset + 1],
        data[offset + 2],
        data[offset + 3],
        data[offset + 4],
        data[offset + 5],
        data[offset + 6],
        data[offset + 7],
    ])
}

fn run_align_up(value: usize) -> usize {
    (value + PAGE_SIZE - 1) & !(PAGE_SIZE - 1)
}
fn validate_run(data: &[u8]) -> bool {
    if data.len() < RUN_HEADER_SIZE || &data[0..4] != RUN_MAGIC {
        return false;
    }

    if run_read_u16(data, 4) != RUN_VERSION
        || run_read_u16(data, 6) != RUN_ARCH_AARCH64
        || run_read_u16(data, 8) as usize != RUN_HEADER_SIZE
    {
        return false;
    }

    let segmentCount = run_read_u16(data, 10) as usize;
    let entry = run_read_u64(data, 12);
    let imageSize = run_read_u64(data, 24);

    if segmentCount == 0
        || segmentCount > RUN_MAX_SEGMENTS
        || imageSize != data.len() as u64
        || entry < VMAP_START as u64
        || entry >= VMAP_END as u64
    {
        return false;
    }

    let tableEnd = match RUN_HEADER_SIZE
        .checked_add(segmentCount * RUN_SEGMENT_SIZE)
    {
        Some(end) => end,
        None => return false,
    };

    if tableEnd > data.len() {
        return false;
    }

    let mut ranges = [(0usize, 0usize); RUN_MAX_SEGMENTS];
    let mut maxEnd = VMAP_START;
    let mut entryIsExecutable = false;

    for i in 0..segmentCount {
        let base = RUN_HEADER_SIZE + i * RUN_SEGMENT_SIZE;

        let virtualAddress = run_read_u64(data, base) as usize;
        let fileOffset = run_read_u64(data, base + 8) as usize;
        let fileSize = run_read_u64(data, base + 16) as usize;
        let memorySize = run_read_u64(data, base + 24) as usize;
        let flags = run_read_u32(data, base + 32);

        if memorySize == 0
            || fileSize > memorySize
            || virtualAddress % PAGE_SIZE != 0
        {
            return false;
        }

        let segmentEnd = match virtualAddress.checked_add(memorySize) {
            Some(end) => end,
            None => return false,
        };

        if virtualAddress < VMAP_START || segmentEnd > VMAP_END {
            return false;
        }

        let fileEnd = match fileOffset.checked_add(fileSize) {
            Some(end) => end,
            None => return false,
        };

        if fileEnd > data.len() {
            return false;
        }

        for j in 0..i {
            let (otherStart, otherEnd) = ranges[j];

            if !(segmentEnd <= otherStart || virtualAddress >= otherEnd) {
                return false;
            }
        }

        ranges[i] = (virtualAddress, segmentEnd);
        maxEnd = maxEnd.max(segmentEnd);

        if (flags & RUN_X) != 0
            && entry >= virtualAddress as u64
            && entry < segmentEnd as u64
        {
            entryIsExecutable = true;
        }
    }

    if !entryIsExecutable {
        return false;
    }

    let heapStart = run_align_up(maxEnd.max(USER_HEAP_START));
    heapStart < USER_HEAP_END
}
fn load_run(address_space_id: usize, data: &[u8],) -> (u64, usize) {
    assert!(data.len() >= RUN_HEADER_SIZE, ".run header too small");
    assert!(&data[0..4] == RUN_MAGIC, "invalid .run magic");

    let version = run_read_u16(data, 4);
    let architecture = run_read_u16(data, 6);
    let header_size = run_read_u16(data, 8) as usize;
    let segment_count = run_read_u16(data, 10) as usize;
    let entry = run_read_u64(data, 12);
    let image_size = run_read_u64(data, 24) as usize;

    assert!(version == RUN_VERSION, "unsupported .run version");
    assert!(architecture == RUN_ARCH_AARCH64, "unsupported .run architecture");
    assert!(header_size == RUN_HEADER_SIZE, "invalid .run header size");
    assert!(segment_count <= RUN_MAX_SEGMENTS, "too many .run segments");
    assert!(image_size == data.len(), ".run image size mismatch");

    let table_end = header_size
        .checked_add(segment_count * RUN_SEGMENT_SIZE)
        .expect(".run segment table overflow");
    assert!(table_end <= data.len(), ".run segment table outside file");

    assert!(entry >= VMAP_START as u64);
    assert!(entry < VMAP_END as u64);

    // Keep segment ranges around so overlapping segments are rejected.
    let mut ranges = [(0usize, 0usize); RUN_MAX_SEGMENTS];
    let mut range_count = 0usize;
    let mut max_end = VMAP_START;

    for i in 0..segment_count {
        let base = header_size + i * RUN_SEGMENT_SIZE;

        let virtual_address = run_read_u64(data, base) as usize;
        let file_offset = run_read_u64(data, base + 8) as usize;
        let file_size = run_read_u64(data, base + 16) as usize;
        let memory_size = run_read_u64(data, base + 24) as usize;
        let flags = run_read_u32(data, base + 32);
        let _alignment = run_read_u32(data, base + 36);

        assert!(memory_size != 0, ".run segment has zero memory size");
        assert!(file_size <= memory_size, ".run file size exceeds memory size");
        assert!(virtual_address % PAGE_SIZE == 0, ".run segment is not page-aligned");

        let segment_end = virtual_address
            .checked_add(memory_size)
            .expect(".run segment address overflow");

        assert!(virtual_address >= VMAP_START);
        assert!(segment_end <= VMAP_END);

        let file_end = file_offset
            .checked_add(file_size)
            .expect(".run file offset overflow");

        assert!(file_end <= data.len(), ".run segment data outside file");

        for j in 0..range_count {
            let (other_start, other_end) = ranges[j];
            assert!(
                segment_end <= other_start || virtual_address >= other_end,
                ".run segments overlap"
            );
        }

        ranges[range_count] = (virtual_address, segment_end);
        range_count += 1;

        if segment_end > max_end {
            max_end = segment_end;
        }

        let pages = run_align_up(memory_size) / PAGE_SIZE;
        let writable = (flags & RUN_W) != 0;
        let executable = (flags & RUN_X) != 0;

        for page in 0..pages {
            let virtual_page = virtual_address + page * PAGE_SIZE;
            let physical = alloc_page().expect("out of physical memory loading .run");

            unsafe {
                core::ptr::write_bytes(
                    physical as *mut u8,
                    0,
                    PAGE_SIZE,
                );

                let remaining_file = file_size.saturating_sub(page * PAGE_SIZE);
                let copy_len = remaining_file.min(PAGE_SIZE);

                if copy_len != 0 {
                    core::ptr::copy_nonoverlapping(
                        data.as_ptr().add(file_offset + page * PAGE_SIZE),
                        physical as *mut u8,
                        copy_len,
                    );
                }

                map_run_page(
                    &mut ADDRESS_SPACES[address_space_id],
                    virtual_page,
                    physical,
                    writable,
                    executable,
                );
            }
        }
    }

    assert!(range_count != 0, ".run contains no loadable segments");

    let heap_start = run_align_up(max_end.max(USER_HEAP_START));
    assert!(heap_start < USER_HEAP_END, ".run leaves no room for user heap/stack");

    unsafe {
        core::arch::asm!("dsb sy", "isb");
    }

    (entry, heap_start)
}

impl Task {
    fn new(id: usize, entry: fn()) -> Self {
        unsafe {
            ADDRESS_SPACES[id].init();
        }

        let stack_bottom;
        let stack_top;

        unsafe {
            stack_bottom = TASK_STACKS[id].0.as_ptr() as u64;
            stack_top = stack_bottom + TASK_STACK_SIZE as u64;
        }

        let stack_top = stack_top & !0xF;
        let initial_sp = stack_top - 16;

        let mut task = Self {
            id,
            state: TaskState::Ready,
            address_space_id: id,
            context: CpuContext {
                x: [0; 31],
                sp: initial_sp,
                pc: task_trampoline as usize as u64,
                spsr: SPSR_EL1H,
                kernel_sp: initial_sp
            },
        };

        task.context.x[0] = entry as usize as u64;
        task.context.x[30] = task_exit as usize as u64;
        task.context.x[1..].fill(0);

        task
    }
    fn new_user(id: usize, run_data: &[u8]) -> Self {
        unsafe {
            ADDRESS_SPACES[id].init();
            USER_HEAP_NEXT[id] = USER_HEAP_START;
        }

        let (entry, heap_start) = load_run(id, run_data);

        unsafe {
            USER_HEAP_NEXT[id] = heap_start;
        }

        let stack_top = USER_STACK_TOP;

        for i in 0..(USER_STACK_SIZE / PAGE_SIZE) {
            let physical = alloc_page()
                .expect("Failed to allocate page for user stack");

            let virtual_address =
                USER_STACK_TOP as usize - USER_STACK_SIZE + i * PAGE_SIZE;

            unsafe {
                core::ptr::write_bytes(
                    physical as *mut u8,
                    0,
                    PAGE_SIZE,
                );
                map_user_page(
                    &mut ADDRESS_SPACES[id],
                    virtual_address,
                    physical,
                    false,
                );
            }
        }

        let kernel_stack_top = unsafe {
            TASK_STACKS[id].0.as_ptr() as u64 + TASK_STACK_SIZE as u64
        };

        Self {
            id,
            state: TaskState::Ready,
            address_space_id: id,
            context: CpuContext {
                x: [0; 31],
                sp: stack_top,
                pc: entry,
                spsr: SPSR_EL0T,
                kernel_sp: kernel_stack_top,
            },
        }
    }
    fn save_context(&mut self, frame: &ExceptionFrame) {
        self.context.x.copy_from_slice(&frame.x);
        self.context.sp = frame.sp;
        self.context.pc = frame.elr;
        self.context.spsr = frame.spsr;
    }
    fn load_context(&self, frame: &mut ExceptionFrame) {
        frame.x.copy_from_slice(&self.context.x);
        frame.sp = self.context.sp;
        frame.elr = self.context.pc;
        frame.spsr = self.context.spsr;
    }
}
struct TaskTable {
    tasks: [Option<Task>; MAX_TASKS],
    count: usize,
}
impl TaskTable {
    fn new() -> Self {
        Self {
            tasks: core::array::from_fn(|_| None),
            count: 0,
        }
    }
    fn create(&mut self, entry: fn()) -> usize {
        if self.count >= MAX_TASKS {
            panic!("No free task slots");
        }
        let id = self.count;
        self.tasks[id] = Some(Task::new(id, entry));
        self.count += 1;
        id
    }      
    fn available_slot(&self) -> Option<usize> {
        if self.count == 0 {
            return None;
        }

        for id in 1..self.count {
            let reusable = self.tasks[id].as_ref().map_or(true, |task| {
                matches!(task.state, TaskState::Dead | TaskState::Unused)
            });

            if reusable {
                return Some(id);
            }
        }

        if self.count < MAX_TASKS {
            Some(self.count)
        } else {
            None
        }
    }
    fn create_user(&mut self, runData: &[u8]) -> usize {
        let id = self
            .available_slot()
            .expect("No free task slots");

        self.tasks[id] = Some(Task::new_user(id, runData));

        if id == self.count {
            self.count += 1;
        }

        id
    }

}
// ============================================================
// Task trampoline / termination
// ============================================================
fn task_trampoline(entry: fn()) -> ! {
    entry();
    task_exit()
}
fn task_exit() -> ! {
    unsafe {
        core::arch::asm!("svc #1");
    }
    loop {
        core::hint::spin_loop();
    }
}
fn yield_now() {
    unsafe {
        core::arch::asm!("svc #2", options(nomem, nostack));
    }
}
// ============================================================
// Test tasks
// ============================================================
fn task_a() { //RR test 1
    let mut uart = Uart::new(0x0900_0000);
    let mut a = 0u64;
    loop {
        for _ in 0..500_000 {
            core::hint::spin_loop();
        }
        a = a.wrapping_add(1);
        writeln!(uart, "A value = {:#x}", a).ok();
        //if a == 100 {
            //task_exit();
        //}
    }
}
fn task_b() { //RR test 2
    let mut uart = Uart::new(0x0900_0000);
    let mut b = 1000u64;
    loop {
        for _ in 0..500_000 {
            core::hint::spin_loop();
        }
        b = b.wrapping_add(1);
        writeln!(uart, "B value = {:#x}", b).ok();
        //if b == 2000 {
           // task_exit();
        //}
    }
}
fn task_c() { // RAM / page allocator test
    unsafe {
        let mut uart = Uart::new(0x0900_0000);

        writeln!(uart, "================================").unwrap();
        writeln!(uart, "        RAM / PAGE TEST         ").unwrap();
        writeln!(uart, "================================").unwrap();

        let ram_end = core::ptr::addr_of!(RAM_END).read();
        let kernel_end = core::ptr::addr_of!(KERNEL_END).read();
        let first_free_page = core::ptr::addr_of!(FIRST_FREE_PAGE).read();
        let page_count = core::ptr::addr_of!(PAGE_COUNT).read();
        let bitmap_size = core::ptr::addr_of!(BITMAP_SIZE).read();

        writeln!(uart, "PAGE_SIZE: {}", PAGE_SIZE).unwrap();
        writeln!(uart, "RAM_START: {:#x}", RAM_START).unwrap();
        writeln!(uart, "RAM_END: {:#x}", ram_end).unwrap();
        writeln!(uart, "KERNEL_END: {:#x}", kernel_end).unwrap();
        writeln!(uart, "FIRST_FREE_PAGE: {:#x}", first_free_page).unwrap();
        writeln!(uart, "PAGE_COUNT: {}", page_count).unwrap();
        writeln!(uart, "BITMAP_SIZE: {}", bitmap_size).unwrap();

        // ========================================================
        // 1. Bitmap operations
        //
        // Allocate a real page first, then test its bitmap bit.
        // This avoids making assumptions about which pages are
        // already owned by the kernel or other tasks.
        // ========================================================

        writeln!(uart, "").unwrap();
        writeln!(uart, "Testing page bitmap...").unwrap();

        let bitmap_test_page = match alloc_page() {
            Some(addr) => addr,
            None => {
                writeln!(uart, "FAIL: could not allocate bitmap test page").unwrap();
                loop {
                    yield_now();
                }
            }
        };

        let bitmap_page =
            (bitmap_test_page - FIRST_FREE_PAGE) / PAGE_SIZE;

        if is_page_used(bitmap_page) {
            writeln!(
                uart,
                "PASS: allocated page is marked used"
            ).unwrap();
        } else {
            writeln!(
                uart,
                "FAIL: allocated page is not marked used"
            ).unwrap();
        }

        // Test clearing and restoring the exact bit belonging to
        // our allocated page. Do not allocate anything while the
        // bit is temporarily cleared.
        mark_page_free(bitmap_page);

        if !is_page_used(bitmap_page) {
            writeln!(
                uart,
                "PASS: bitmap bit can be cleared"
            ).unwrap();
        } else {
            writeln!(
                uart,
                "FAIL: bitmap bit could not be cleared"
            ).unwrap();
        }

        mark_page_used(bitmap_page);

        if is_page_used(bitmap_page) {
            writeln!(
                uart,
                "PASS: bitmap bit can be restored"
            ).unwrap();
        } else {
            writeln!(
                uart,
                "FAIL: bitmap bit could not be restored"
            ).unwrap();
        }

        free_page(bitmap_test_page);

        if !is_page_used(bitmap_page) {
            writeln!(
                uart,
                "PASS: free_page clears allocation bit"
            ).unwrap();
        } else {
            writeln!(
                uart,
                "FAIL: free_page left allocation bit set"
            ).unwrap();
        }

        writeln!(uart, "PASS: page bitmap").unwrap();

        // ========================================================
        // 2. Single-page allocation
        // ========================================================

        writeln!(uart, "").unwrap();
        writeln!(uart, "Testing single page allocation...").unwrap();

        let page = match alloc_page() {
            Some(addr) => addr,
            None => {
                writeln!(uart, "FAIL: could not allocate page").unwrap();
                loop {
                    yield_now();
                }
            }
        };

        let page_index =
            (page - FIRST_FREE_PAGE) / PAGE_SIZE;

        writeln!(uart, "Allocated: {:#x}", page).unwrap();

        if page % PAGE_SIZE == 0 {
            writeln!(uart, "PASS: page is aligned").unwrap();
        } else {
            writeln!(uart, "FAIL: page is not aligned").unwrap();
        }

        if page >= FIRST_FREE_PAGE && page < RAM_END {
            writeln!(uart, "PASS: page is inside usable RAM").unwrap();
        } else {
            writeln!(uart, "FAIL: page outside usable RAM").unwrap();
        }

        if is_page_used(page_index) {
            writeln!(uart, "PASS: page marked used").unwrap();
        } else {
            writeln!(uart, "FAIL: page not marked used").unwrap();
        }

        free_page(page);

        if !is_page_used(page_index) {
            writeln!(uart, "PASS: page marked free after free_page").unwrap();
        } else {
            writeln!(uart, "FAIL: page still marked used").unwrap();
        }

        // ========================================================
        // 3. Multiple-page uniqueness
        // ========================================================

        writeln!(uart, "").unwrap();
        writeln!(uart, "Testing multiple allocations...").unwrap();

        const TEST_PAGES: usize = 8;
        let mut pages = [0usize; TEST_PAGES];
        let mut allocated = 0usize;
        let mut passed = true;

        while allocated < TEST_PAGES {
            match alloc_page() {
                Some(addr) => {
                    pages[allocated] = addr;
                    allocated += 1;

                    writeln!(
                        uart,
                        "Page {}: {:#x}",
                        allocated,
                        addr
                    ).unwrap();
                }

                None => {
                    writeln!(
                        uart,
                        "FAIL: allocator ran out after {} pages",
                        allocated
                    ).unwrap();

                    passed = false;
                    break;
                }
            }
        }

        // Check alignment, range and uniqueness.
        for i in 0..allocated {
            if pages[i] % PAGE_SIZE != 0 {
                writeln!(
                    uart,
                    "FAIL: page {} is misaligned",
                    i
                ).unwrap();
                passed = false;
            }

            if pages[i] < FIRST_FREE_PAGE || pages[i] >= RAM_END {
                writeln!(
                    uart,
                    "FAIL: page {} is outside usable RAM",
                    i
                ).unwrap();
                passed = false;
            }

            let index =
                (pages[i] - FIRST_FREE_PAGE) / PAGE_SIZE;

            if !is_page_used(index) {
                writeln!(
                    uart,
                    "FAIL: page {} is not marked used",
                    i
                ).unwrap();
                passed = false;
            }

            for j in 0..i {
                if pages[i] == pages[j] {
                    writeln!(
                        uart,
                        "FAIL: duplicate allocation {:#x}",
                        pages[i]
                    ).unwrap();
                    passed = false;
                }
            }
        }

        if passed && allocated == TEST_PAGES {
            writeln!(
                uart,
                "PASS: {} unique pages allocated",
                TEST_PAGES
            ).unwrap();
        }

        for i in 0..allocated {
            free_page(pages[i]);
        }

        writeln!(uart, "Freed all test pages").unwrap();

        // ========================================================
        // 4. Page reuse
        // ========================================================

        writeln!(uart, "").unwrap();
        writeln!(uart, "Testing page reuse...").unwrap();

        let first = match alloc_page() {
            Some(addr) => addr,
            None => {
                writeln!(uart, "FAIL: first allocation failed").unwrap();
                loop {
                    yield_now();
                }
            }
        };

        free_page(first);

        let second = match alloc_page() {
            Some(addr) => addr,
            None => {
                writeln!(uart, "FAIL: second allocation failed").unwrap();
                loop {
                    yield_now();
                }
            }
        };

        if first == second {
            writeln!(
                uart,
                "PASS: freed page was reused ({:#x})",
                first
            ).unwrap();
        } else {
            writeln!(
                uart,
                "FAIL: expected {:#x}, got {:#x}",
                first,
                second
            ).unwrap();
        }

        free_page(second);

        // ========================================================
        // 5. Physical RAM read/write test
        //
        // Only touches a page owned by this test.
        // ========================================================

        writeln!(uart, "").unwrap();
        writeln!(uart, "Testing physical RAM access...").unwrap();

        let test_page = match alloc_page() {
            Some(addr) => addr,
            None => {
                writeln!(uart, "FAIL: could not allocate RAM test page").unwrap();
                loop {
                    yield_now();
                }
            }
        };

        let ptr = test_page as *mut u8;

        // Pattern 1
        for i in 0..PAGE_SIZE {
            ptr.add(i).write_volatile(0xAA);
        }

        let mut passed = true;

        for i in 0..PAGE_SIZE {
            let value = ptr.add(i).read_volatile();

            if value != 0xAA {
                writeln!(
                    uart,
                    "FAIL: 0xAA pattern mismatch at byte {}: {:#x}",
                    i,
                    value
                ).unwrap();

                passed = false;
                break;
            }
        }

        if passed {
            writeln!(
                uart,
                "PASS: 0xAA pattern"
            ).unwrap();
        }

        // Pattern 2
        for i in 0..PAGE_SIZE {
            ptr.add(i).write_volatile(0x55);
        }

        passed = true;

        for i in 0..PAGE_SIZE {
            let value = ptr.add(i).read_volatile();

            if value != 0x55 {
                writeln!(
                    uart,
                    "FAIL: 0x55 pattern mismatch at byte {}: {:#x}",
                    i,
                    value
                ).unwrap();

                passed = false;
                break;
            }
        }

        if passed {
            writeln!(
                uart,
                "PASS: 0x55 pattern"
            ).unwrap();
        }

        // Deterministic word-level pattern.
        let words = PAGE_SIZE / 8;
        let word_ptr = test_page as *mut u64;

        for i in 0..words {
            let pattern =
                0x1000_0000_0000_0000u64 | i as u64;

            word_ptr.add(i).write_volatile(pattern);
        }

        passed = true;

        for i in 0..words {
            let expected =
                0x1000_0000_0000_0000u64 | i as u64;

            let value = word_ptr.add(i).read_volatile();

            if value != expected {
                writeln!(
                    uart,
                    "FAIL: word {}: expected {:#018x}, got {:#018x}",
                    i,
                    expected,
                    value
                ).unwrap();

                passed = false;
                break;
            }
        }

        if passed {
            writeln!(
                uart,
                "PASS: word-addressable RAM test"
            ).unwrap();
        }

        free_page(test_page);

        // ========================================================
        // 6. VM allocator test
        //
        // Use the CURRENT address space rather than hardcoding an
        // ASID, and only touch virtual pages returned by the VM
        // allocator.
        // ========================================================

        writeln!(uart, "").unwrap();
        writeln!(uart, "Testing VM allocator...").unwrap();

        let asid = CURRENT_ASID;

        let va_a = vm_alloc_page(asid);
        let va_b = vm_alloc_page(asid);
        let va_c = vm_alloc_page(asid);

        match (va_a, va_b, va_c) {
            (Some(a), Some(b), Some(c)) => {
                writeln!(uart, "VA A: {:#x}", a).unwrap();
                writeln!(uart, "VA B: {:#x}", b).unwrap();
                writeln!(uart, "VA C: {:#x}", c).unwrap();

                if a == b || a == c || b == c {
                    writeln!(
                        uart,
                        "FAIL: VM allocator returned duplicate VAs"
                    ).unwrap();
                } else {
                    writeln!(
                        uart,
                        "PASS: VM virtual addresses are unique"
                    ).unwrap();
                }

                (a as *mut u64)
                    .write_volatile(0xAAAAAAAAAAAAAAAA);

                (b as *mut u64)
                    .write_volatile(0xBBBBBBBBBBBBBBBB);

                (c as *mut u64)
                    .write_volatile(0xCCCCCCCCCCCCCCCC);

                let a_value = (a as *const u64).read_volatile();
                let b_value = (b as *const u64).read_volatile();
                let c_value = (c as *const u64).read_volatile();

                if a_value == 0xAAAAAAAAAAAAAAAA {
                    writeln!(uart, "PASS: VA A read/write").unwrap();
                } else {
                    writeln!(
                        uart,
                        "FAIL: VA A readback = {:#018x}",
                        a_value
                    ).unwrap();
                }

                if b_value == 0xBBBBBBBBBBBBBBBB {
                    writeln!(uart, "PASS: VA B read/write").unwrap();
                } else {
                    writeln!(
                        uart,
                        "FAIL: VA B readback = {:#018x}",
                        b_value
                    ).unwrap();
                }

                if c_value == 0xCCCCCCCCCCCCCCCC {
                    writeln!(uart, "PASS: VA C read/write").unwrap();
                } else {
                    writeln!(
                        uart,
                        "FAIL: VA C readback = {:#018x}",
                        c_value
                    ).unwrap();
                }

                vm_free_page(a);
                vm_free_page(b);
                vm_free_page(c);

                writeln!(
                    uart,
                    "PASS: VM pages freed"
                ).unwrap();
            }

            _ => {
                writeln!(
                    uart,
                    "FAIL: VM allocation failed"
                ).unwrap();

                if let Some(a) = va_a {
                    vm_free_page(a);
                }

                if let Some(b) = va_b {
                    vm_free_page(b);
                }

                if let Some(c) = va_c {
                    vm_free_page(c);
                }
            }
        }

        // ========================================================
        // 7. Page-table inspection
        // ========================================================

        writeln!(uart, "").unwrap();
        writeln!(uart, "Page-table information:").unwrap();

        writeln!(
            uart,
            "L0[0]   = {:#018x}",
            PAGE_TABLE_L0.entries[0].0
        ).unwrap();

        writeln!(
            uart,
            "L2 RAM[0] = {:#018x}",
            PAGE_TABLE_L2_RAM.entries[0].0
        ).unwrap();

        writeln!(
            uart,
            "L2 RAM[1] = {:#018x}",
            PAGE_TABLE_L2_RAM.entries[1].0
        ).unwrap();

        // ========================================================
        // Done
        // ========================================================

        writeln!(uart, "").unwrap();
        writeln!(uart, "================================").unwrap();
        writeln!(uart, "       ALL RAM TESTS DONE       ").unwrap();
        writeln!(uart, "================================").unwrap();

        loop {
            yield_now();
        }
    }
}
// ============================================================
// Idle
// ============================================================
fn idle_task() {
    loop {
        unsafe {
            core::arch::asm!("wfe");
        }
    }
}

// ============================================================
// User test tasks
// ============================================================
extern "C" fn user_test() -> ! {
    unsafe {
        core::arch::asm!("svc #42");
    }

    loop {
        yield_now();
    }
}
// ============================================================
// Scheduler
// ============================================================
struct Scheduler {
    tasks: TaskTable,
    current: usize,
    started: bool,
    foreground: Option<(usize, usize)>,
}
impl Scheduler {
    fn new() -> Self {
        Self {
            tasks: TaskTable::new(),
            current: 0,
            started: false,
            foreground: None,
        }
    }
    fn add_task(&mut self, entry: fn()) -> usize {
        self.tasks.create(entry)
    }
    fn add_user_task(&mut self, runData: &[u8]) -> usize {
        self.tasks.create_user(runData)
    }
    fn next_ready(&self) -> Option<usize> {
        if self.tasks.count <= 1 {
            return None;
        }
        let start = self.current;
        for offset in 1..self.tasks.count {
            let id = (start + offset) % self.tasks.count;
            if id == 0 {
                continue;
            }
            match &self.tasks.tasks[id] {
                Some(task) if task.state == TaskState::Ready => {
                    return Some(id);
                }
                _ => {}
            }
        }
        None
    }
    fn start(&mut self, frame: &mut ExceptionFrame) {
        if self.started {
            return;
        }
        self.started = true;
        if self.tasks.count > 1 {
            self.current = 1;
        } else {
            self.current = 0;
        }
        let id = self.current;
        self.switch_to_task(self.current, frame);
    }
    fn tick(&mut self, frame: &mut ExceptionFrame) {
        if self.tasks.count == 0 {
            return;
        }
        if !self.started {
            self.start(frame);
            return;
        }
        if self.current != 0 {
            if let Some(task) = &mut self.tasks.tasks[self.current] {
                if task.state == TaskState::Running {
                    task.save_context(frame);
                    task.state = TaskState::Ready;
                }
            }
        }
        let next = match self.next_ready() {
            Some(id) => id,
            None => {
                if self.current != 0 {
                    if let Some(task) = &mut self.tasks.tasks[self.current] {
                        if task.state == TaskState::Ready {
                            task.state = TaskState::Running;
                            task.load_context(frame);
                            return;
                        }
                    }
                }
                0
            }
        };
        self.switch_to_task(next, frame);
    }
    fn yield_current(&mut self, frame: &mut ExceptionFrame) {
        if self.tasks.count <= 1 {
            return;
        }
        if self.current != 0 {
            if let Some(task) = &mut self.tasks.tasks[self.current] {
                task.save_context(frame);
                task.state = TaskState::Ready;
            }
        }
        let next = match self.next_ready() {
            Some(id) => id,
            None => {
                if self.current != 0 {
                    if let Some(task) = &mut self.tasks.tasks[self.current] {
                        task.state = TaskState::Running;
                        task.load_context(frame);
                    }
                }
                return;
            }
        };
        self.switch_to_task(next, frame);
    }
    
    fn exit_current(&mut self, frame: &mut ExceptionFrame) {
        if self.tasks.count == 0 {
            panic!("No tasks");
        }

        if self.current == 0 {
            panic!("Idle task cannot exit");
        }

        let dead = self.current;

        // Release this process's user mappings and physical pages.
        reclaim_user_pages(dead);

        if let Some(task) = &mut self.tasks.tasks[dead] {
            task.state = TaskState::Dead;
        }

        // Did the foreground child just exit?
        let parentToResume = match self.foreground {
            Some((parentId, childId)) if childId == dead => {
                self.foreground = None;
                Some(parentId)
            }
            _ => None,
        };

        if let Some(parentId) = parentToResume {
            let parentReady =
                if let Some(parent) = self.tasks.tasks[parentId].as_mut() {
                    if parent.state == TaskState::Blocked {
                        parent.state = TaskState::Ready;
                        true
                    } else {
                        false
                    }
                } else {
                    false
                };

            if parentReady {
                self.switch_to_task(parentId, frame);
                return;
            }
        }

        // Preserve the ordinary exit path for tasks that have no
        // blocked foreground parent.
        for offset in 1..self.tasks.count {
            let id = (dead + offset) % self.tasks.count;

            if id == 0 {
                continue;
            }

            if let Some(task) = &self.tasks.tasks[id] {
                if task.state == TaskState::Ready {
                    self.switch_to_task(id, frame);
                    return;
                }
            }
        }

        self.switch_to_task(0, frame);
    }

    fn switch_to_task(
        &mut self,
        id: usize,
        frame: &mut ExceptionFrame,
    ) {
        self.current = id;

        let address_space_id = self.tasks.tasks[id]
            .as_ref()
            .unwrap()
            .address_space_id;
         unsafe {
            CURRENT_ASID = address_space_id;
        }   

        if let Some(task) = &mut self.tasks.tasks[id] {
            task.state = TaskState::Running;
            task.load_context(frame);
        }

        unsafe {
            switch_address_space(address_space_id);
        }
    }
    fn spawn_foreground_user(
        &mut self,
        runData: &[u8],
        frame: &mut ExceptionFrame,
    ) -> bool {
        // Only one foreground child may own the terminal at a time.
        if self.current == 0
            || self.foreground.is_some()
            || self.tasks.available_slot().is_none()
        {
            return false;
        }

        let parentId = self.current;
        let childId = self.add_user_task(runData);

        // When the child exits, the parent will return from its
        // spawn syscall with this task ID in x0.
        frame.x[0] = childId as u64;

        {
            let parent = self.tasks.tasks[parentId]
                .as_mut()
                .expect("Current task missing");

            parent.save_context(frame);
            parent.state = TaskState::Blocked;
        }

        self.foreground = Some((parentId, childId));

        // Save the child as the current context and start it.
        self.switch_to_task(childId, frame);

        true
    }

}

// ============================================================
// Scheduler storage
// ============================================================
#[unsafe(no_mangle)]
#[unsafe(link_section = ".scheduler")]
static mut SCHEDULER: Option<Scheduler> = None;
static mut FILESYSTEM_PTR: *mut filesystem::FileSystem =
    core::ptr::null_mut();
// ============================================================
// GIC
// ============================================================
struct Gic {
    dist: usize,
    cpu: usize,
}
impl Gic {
    const fn new() -> Self {
        Self {
            dist: 0x0800_0000,
            cpu: 0x0801_0000,
        }
    }
    fn write_dist(&self, offset: usize, value: u32) {
        unsafe {
            ((self.dist + offset) as *mut u32).write_volatile(value);
        }
    }
    fn write_cpu(&self, offset: usize, value: u32) {
        unsafe {
            ((self.cpu + offset) as *mut u32).write_volatile(value);
        }
    }
    fn enable(&self) {
        let mut uart = Uart::new(0x0900_0000);
        writeln!(uart, "Enabling GIC distributor...").ok();
        self.write_dist(0x000, 1);
        writeln!(uart, "Enabling timer PPI...").ok();
        self.write_dist(0x100, 1 << 30);
        writeln!(uart, "Configuring CPU priority...").ok();
        self.write_cpu(0x004, 0xFF);
        writeln!(uart, "Enabling GIC CPU interface...").ok();
        self.write_cpu(0x000, 1);
    }
}
// ============================================================
// Timer
// ============================================================
struct Timer;
impl Timer {
    fn frequency() -> u64 {
        let freq: u64;
        unsafe {
            core::arch::asm!("mrs {0}, cntfrq_el0", out(reg) freq);
        }
        freq
    }
    fn counter() -> u64 {
        let counter: u64;
        unsafe {
            core::arch::asm!("mrs {0}, cntpct_el0", out(reg) counter);
        }
        counter
    }
    fn set_timeout(ticks: u64) {
        unsafe {
            core::arch::asm!("msr cntp_tval_el0, {0}", in(reg) ticks);
        }
    }
    fn enable() {
        unsafe {
            core::arch::asm!("msr cntp_ctl_el0, {0}", "isb", in(reg) 1u64);
        }
    }
}
// ============================================================
// RAM / Physical Page Allocator
// ============================================================
const PAGE_SIZE: usize = 4096;
const RAM_START: usize = 0x4000_0000;
const MAX_RAM_END: usize = 0x6000_0000;
const MAX_PAGE_COUNT: usize = (MAX_RAM_END - RAM_START) / PAGE_SIZE;
const MAX_BITMAP_SIZE: usize = (MAX_PAGE_COUNT + 7) / 8;
#[unsafe(link_section = ".page_bitmap")]
static mut PAGE_BITMAP: [u8; MAX_BITMAP_SIZE] = [0; MAX_BITMAP_SIZE];
static mut RAM_END: usize = MAX_RAM_END;
static mut KERNEL_END: usize = 0;
static mut FIRST_FREE_PAGE: usize = 0;
static mut PAGE_COUNT: usize = 0;
static mut BITMAP_SIZE: usize = 0;
const VMAP_START: usize = 0x8000_0000;
const VMAP_SIZE: usize = 0x20_0000;
const VMAP_END: usize = VMAP_START + VMAP_SIZE;
const VMAP_PAGE_COUNT: usize = VMAP_SIZE / PAGE_SIZE;
const VMAP_BITMAP_SIZE: usize = (VMAP_PAGE_COUNT + 7) /8;
static mut VMAP_BITMAP: [u8; VMAP_BITMAP_SIZE] = [0; VMAP_BITMAP_SIZE];
unsafe extern "C" {
    static _kernel_start: u8;
    static _kernel_end: u8;
}
// ------------------------------------------------------------
// Initialisation
// ------------------------------------------------------------
fn init_memory() {
    unsafe {
        KERNEL_END = &_kernel_end as *const u8 as usize;
        FIRST_FREE_PAGE = (KERNEL_END + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);
        PAGE_COUNT = (RAM_END - FIRST_FREE_PAGE) / PAGE_SIZE;
        BITMAP_SIZE = (PAGE_COUNT + 7) / 8;
        let bitmap_ptr = core::ptr::addr_of_mut!(PAGE_BITMAP) as *mut u8;
        for i in 0..BITMAP_SIZE {
            bitmap_ptr.add(i).write(0);
        }
    }
}
// ------------------------------------------------------------
// Bitmap operations
// ------------------------------------------------------------
fn is_page_used(page: usize) -> bool {
    unsafe {
        assert!(page < PAGE_COUNT);
        let byte = page / 8;
        let bit = page % 8;
        let mask = 1u8 << bit;
        let bitmap_ptr = core::ptr::addr_of!(PAGE_BITMAP) as *const u8;
        (bitmap_ptr.add(byte).read() & mask) != 0
    }
}
fn is_virtual_page_used(page: usize) -> bool{
    unsafe{
        assert!(page < VMAP_PAGE_COUNT);
        let byte = page / 8;
        let bit = page % 8;
        let mask = 1u8 << bit;
        (VMAP_BITMAP[byte] & mask) != 0
    }
}
fn mark_page_used(page: usize) {
    unsafe {
        assert!(page < PAGE_COUNT);
        let byte = page / 8;
        let bit = page % 8;
        let mask = 1u8 << bit;
        let bitmap_ptr = core::ptr::addr_of_mut!(PAGE_BITMAP) as *mut u8;
        let ptr = bitmap_ptr.add(byte);
        *ptr |= mask;
    }
}
fn mark_virtual_page_used(page: usize){
    unsafe{
        assert!(page < VMAP_PAGE_COUNT);
        let byte = page /8;
        let bit = page % 8;
        let mask = 1u8 << bit;
        VMAP_BITMAP[byte] |= mask;
    }
}
fn mark_page_free(page: usize) {
    unsafe {
        assert!(page < PAGE_COUNT);
        let byte = page / 8;
        let bit = page % 8;
        let mask = 1u8 << bit;
        let bitmap_ptr = core::ptr::addr_of_mut!(PAGE_BITMAP) as *mut u8;
        let ptr = bitmap_ptr.add(byte);
        *ptr &= !mask;
    }
}
fn mark_virtual_page_free(page: usize){
    unsafe{
        assert!(page < VMAP_PAGE_COUNT);
        let byte = page /8;
        let bit = page % 8;
        let mask = 1u8 << bit;
        VMAP_BITMAP[byte] &= !mask;
    }
}
fn alloc_page() -> Option<usize> {
    unsafe {
        for page in 0..PAGE_COUNT {
            if !is_page_used(page) {
                mark_page_used(page);
                let address = FIRST_FREE_PAGE + page * PAGE_SIZE;
                return Some(address);
            }
        }
    }
    None
}
fn alloc_virtual_page() -> Option<usize> {
    for page in 0..VMAP_PAGE_COUNT{
        if !is_virtual_page_used(page){
            mark_virtual_page_used(page);
            let address = VMAP_START + page * PAGE_SIZE;
            return Some(address);
        }
    }
    None
}
fn free_page(address: usize) {
    unsafe {
        assert!(address >= FIRST_FREE_PAGE);
        assert!(address < RAM_END);
        assert!(address % PAGE_SIZE == 0);
        let page = (address - FIRST_FREE_PAGE) / PAGE_SIZE;
        assert!(page < PAGE_COUNT);
        assert!(is_page_used(page));
        mark_page_free(page);
    }
}
fn free_virtual_page(address: usize){
    assert!(address >= VMAP_START);
    assert!(address < VMAP_END);
    assert!(address % PAGE_SIZE == 0);
    let page = (address - VMAP_START) / PAGE_SIZE;
    assert!(page < VMAP_PAGE_COUNT);
    assert!(is_virtual_page_used(page));
    mark_virtual_page_free(page);
}
unsafe fn map_page(address_space: &mut AddressSpace, virt_addr: usize, phys_addr: usize){
    assert!(virt_addr % PAGE_SIZE == 0);
    assert!(phys_addr % PAGE_SIZE == 0);
    assert!(virt_addr >= VMAP_START);
    assert!(virt_addr < VMAP_END);

    let index = (virt_addr >> 12) & 0x1FF;
    assert!(!address_space.l3_vmap.entries[index].is_valid());
    address_space.l3_vmap.entries[index] = 
        PageTableEntry::new_page(
            phys_addr,
            ATTR_NORMAL,
            0b00,
            0b11,
            false
        )
}
unsafe fn map_user_page(address_space: &mut AddressSpace, virt_addr: usize, phys_addr: usize, executable: bool){
    assert!(virt_addr % PAGE_SIZE == 0);
    assert!(phys_addr % PAGE_SIZE == 0);
    assert!(virt_addr >= VMAP_START);
    assert!(virt_addr < VMAP_END);
    let index = (virt_addr >> 12) & 0x1FF;
    assert!(!address_space.l3_vmap.entries[index].is_valid());
    address_space.l3_vmap.entries[index] =
        PageTableEntry::new_page(
            phys_addr, ATTR_NORMAL, 0b01, 0b11, !executable
        );
}
unsafe fn map_run_page(
    address_space: &mut AddressSpace,
    virt_addr: usize,
    phys_addr: usize,
    writable: bool,
    executable: bool,
) {
    assert!(virt_addr % PAGE_SIZE == 0);
    assert!(phys_addr % PAGE_SIZE == 0);
    assert!(virt_addr >= VMAP_START);
    assert!(virt_addr < VMAP_END);

    let index = (virt_addr >> 12) & 0x1FF;
    assert!(!address_space.l3_vmap.entries[index].is_valid());

    // AP=01: EL0 read/write.
    // AP=11: EL0 read-only.
    let ap = if writable { 0b01 } else { 0b11 };

    address_space.l3_vmap.entries[index] =
        PageTableEntry::new_page(
            phys_addr,
            ATTR_NORMAL,
            ap,
            0b11,
            !executable,
        );
}
unsafe fn map_user_stack(address_space: &mut AddressSpace) -> usize {
    let stack_bottom = USER_STACK_TOP as usize - USER_STACK_SIZE as usize;

    for i in 0..(USER_STACK_SIZE / PAGE_SIZE) {
        let phys_addr = alloc_page().expect("Out of physical memory");
        let virt_addr = stack_bottom + i * PAGE_SIZE;

        map_user_page(
            address_space,
            virt_addr,
            phys_addr,
            false,
        );
    }

    USER_STACK_TOP as usize
}
unsafe fn unmap_page(address_space: &mut AddressSpace, virt_addr: usize) {
    assert!(virt_addr % PAGE_SIZE == 0);
    assert!(virt_addr >= VMAP_START);
    assert!(virt_addr < VMAP_END);
    let index = (virt_addr >> 12) & 0x1FF;
    assert!(address_space.l3_vmap.entries[index].is_valid());
    address_space.l3_vmap.entries[index] = PageTableEntry::invalid();
}

fn reclaim_user_pages(addressSpaceId: usize) {
    unsafe{
        if addressSpaceId >= MAX_TASKS {
            return;
        }

        // Each entry corresponds to one possible 4 KiB user page.
        let mut physicalPages = [0usize; 512];
        let mut pageCount = 0usize;

        unsafe {
            let addressSpaces = &raw mut ADDRESS_SPACES;
            let entries =
                &mut (*addressSpaces)[addressSpaceId].l3_vmap.entries;

            for entry in entries.iter_mut() {
                if entry.is_valid() {
                    let physical =
                        (entry.0 & 0x0000_FFFF_FFFF_F000) as usize;

                    *entry = PageTableEntry::invalid();

                    physicalPages[pageCount] = physical;
                    pageCount += 1;
                }
            }

            let heapNext = &raw mut USER_HEAP_NEXT;
            (*heapNext)[addressSpaceId] = USER_HEAP_START;

            // Remove stale translations before releasing physical pages.
            core::arch::asm!(
                "dsb ishst",
                "tlbi vmalle1",
                "dsb ish",
                "isb",
                options(nostack)
            );
        }

        for physical in physicalPages[..pageCount].iter().copied() {
            if physical >= FIRST_FREE_PAGE
                && physical < RAM_END
                && physical % PAGE_SIZE == 0
            {
                free_page(physical);
            }
        }
    }   
}

// ============================================================
// Page tables
// ============================================================
#[repr(transparent)]
#[derive(Clone, Copy)]
struct PageTableEntry(u64);
impl PageTableEntry {
    const fn invalid() -> Self {
        Self(0)
    }
    const fn new(value: u64) -> Self {
        Self(value)
    }
    fn is_valid(&self) -> bool {
        self.0 & 1 != 0
    }
    fn new_table(table: *const PageTable) -> Self {
        let address = table as u64;
        Self(address | 0b11)
    }
    const fn new_block(
        address: usize,
        attr_index: u64,
        ap: u64,
        sh: u64,
        xn: bool,
    ) -> Self {
        let mut value =
            ((address as u64) & 0x0000_0000_FFE0_0000)
            | 0b01                         // block
            | (attr_index << 2)            // AttrIndx
            | (ap << 6)                    // AP
            | (sh << 8)                    // SH
            | (1 << 10);                   // AF

        if xn {
            value |= 1 << 54;              // UXN
            value |= 1 << 53;              // PXN
        }

        Self(value)
    }
    const fn new_page(address: usize, attr_index: u64, ap: u64, sh: u64, xn: bool) -> Self{
        let mut value = ((address as u64) & 0x0000_FFFF_FFFF_F000) | 0b11 | (attr_index << 2) | (ap << 6) | (sh << 8) | (1 << 10);
        if xn {
            value |= 1 << 54;
            value |= 1 << 53;
        }
        Self(value)
    }
}
#[repr(C, align(4096))]
#[derive(Clone, Copy)]
struct PageTable {
    entries: [PageTableEntry; 512],
}
impl PageTable {
    const fn new() -> Self {
        Self {
            entries: [PageTableEntry::invalid(); 512],
        }
    }
}
#[unsafe(no_mangle)]
#[unsafe(link_section = ".page_table_l0")]
static mut PAGE_TABLE_L0: PageTable = PageTable::new();
#[unsafe(no_mangle)]
#[unsafe(link_section = ".page_table_l1_low")]
static mut PAGE_TABLE_L1_LOW: PageTable = PageTable::new();
#[unsafe(no_mangle)]
#[unsafe(link_section = ".page_table_l1_ram")]
static mut PAGE_TABLE_L1_RAM: PageTable = PageTable::new();
#[unsafe(no_mangle)]
#[unsafe(link_section = ".page_table_l2_device")]
static mut PAGE_TABLE_L2_DEVICE: PageTable = PageTable::new();
#[unsafe(no_mangle)]
#[unsafe(link_section = ".page_table_l2_ram")]
static mut PAGE_TABLE_L2_RAM: PageTable = PageTable::new();
#[unsafe(no_mangle)]
#[unsafe(link_section = ".page_table_l2_vmap")]
static mut PAGE_TABLE_L2_VMAP: PageTable = PageTable::new();
#[unsafe(no_mangle)]
#[unsafe(link_section = ".page_table_l3")]
static mut PAGE_TABLE_L3: PageTable = PageTable::new();
#[unsafe(no_mangle)]
#[unsafe(link_section = ".page_table_l3_vmap")]
static mut PAGE_TABLE_L3_VMAP: PageTable = PageTable::new();
const ATTR_NORMAL: u64 = 0;
const ATTR_DEVICE: u64 = 1;
fn init_mair() {
    unsafe {
        let mair: u64 = 0x04FF;
        core::arch::asm!(
            "msr mair_el1, {0}",
            "isb",
            in(reg) mair,
        );
    }
}
fn init_tcr() {
    unsafe {
        let tcr: u64 =
            // TTBR0: 48-bit VA space
            (16 << 0)

            // Inner cacheability: Write-Back
            | (0b01 << 8)

            // Outer cacheability: Write-Back
            | (0b01 << 10)

            // Inner Shareable
            | (0b11 << 12)

            // TG0 = 4 KiB
            | (0b00 << 14)

            // TTBR1: unused, but valid
            // 48-bit VA space
            | (16 << 16)

            // Inner cacheability
            | (0b01 << 24)

            // Outer cacheability
            | (0b01 << 26)

            // Inner Shareable
            | (0b11 << 28)

            // TG1 = 4 KiB
            | (0b10 << 30)

            // EPD1: disable TTBR1 walks
            | (1 << 23)

            // IPS = 40-bit physical addresses
            | (0b010 << 32);

        core::arch::asm!(
            "msr tcr_el1, {0}",
            "isb",
            in(reg) tcr,
        );
    }
}
fn init_ttbr0() {
    unsafe {
        let l0 = &raw const PAGE_TABLE_L0 as *const _ as u64;
        core::arch::asm!(
            "msr ttbr0_el1, {0}",
            "dsb sy",
            "isb",
            in(reg) l0,
        );
    }
}
unsafe fn set_page_table() {
    // ========================================================
    // L0
    // ========================================================

    // All addresses we currently use are in the lower
    // 512 GiB, so they all use L0[0].
    PAGE_TABLE_L0.entries[0] =
        PageTableEntry::new_table(&raw const PAGE_TABLE_L1_LOW);

    // ========================================================
    // L1
    // ========================================================

    // 0x00000000..0x3FFFFFFF
    // Contains our device mappings at 0x08000000 and 0x09000000.
    PAGE_TABLE_L1_LOW.entries[0] =
        PageTableEntry::new_table(&raw const PAGE_TABLE_L2_DEVICE);

    // 0x40000000..0x7FFFFFFF
    // Contains our kernel RAM.
    PAGE_TABLE_L1_LOW.entries[1] =
        PageTableEntry::new_table(&raw const PAGE_TABLE_L2_RAM);
    PAGE_TABLE_L1_LOW.entries[2] = 
        PageTableEntry::new_table(&raw const PAGE_TABLE_L2_VMAP);

    // ========================================================
    // L2: devices
    // ========================================================

    // 0x08000000
    PAGE_TABLE_L2_DEVICE.entries[64] =
        PageTableEntry::new_block(0x08000000, ATTR_DEVICE, 0b00, 0b10, true);

    // 0x09000000
    PAGE_TABLE_L2_DEVICE.entries[72] =
        PageTableEntry::new_block(0x09000000, ATTR_DEVICE, 0b00, 0b10, true,);

    PAGE_TABLE_L2_DEVICE.entries[80] =
    PageTableEntry::new_block(
        0x0A000000,
        ATTR_DEVICE,
        0b00,
        0b10,
        true,
    );
    // ========================================================
    // L2: RAM
    // ========================================================
    PAGE_TABLE_L2_RAM.entries[0] =
        PageTableEntry::new_table(&raw const PAGE_TABLE_L3);
    for i in 1..256 {
        PAGE_TABLE_L2_RAM.entries[i] =
            PageTableEntry::new_block(
                0x4000_0000 + i * 0x20_0000,
                ATTR_NORMAL,
                0b00,
                0b11,
                false,
            );
    }
    PAGE_TABLE_L2_VMAP.entries[0] = 
        PageTableEntry::new_table(&raw const PAGE_TABLE_L3_VMAP);
    for i in 0..512 {
        PAGE_TABLE_L3.entries[i] =
            PageTableEntry::new_page(
                0x4000_0000 + i * PAGE_SIZE,
                ATTR_NORMAL,
                0b00,
                0b11,
                false,
            );
    }
}
// ============================================================
// MMU / stack transition
// ============================================================
fn enable_mmu() {
    unsafe {
        let mut sctlr: u64;

        core::arch::asm!(
            "mrs {0}, sctlr_el1",
            out(reg) sctlr,
        );

        // Enable MMU
        sctlr |= 1 << 0;

        // Disable WXN so writable kernel RAM can also be executable.
        sctlr &= !(1 << 19);

        core::arch::asm!(
            "dsb sy",
            "msr sctlr_el1, {0}",
            "isb",
            in(reg) sctlr,
        );
    }
}
// ============================================================
// VM
// ============================================================

fn vm_alloc_page(address_space_id: usize) -> Option<usize> {
    let physical = alloc_page()?;

    let virt_addr = match alloc_virtual_page() {
        Some(addr) => addr,
        None => {
            free_page(physical);
            return None;
        }
    };

    unsafe {
        let address_space = &mut ADDRESS_SPACES[address_space_id];

        map_page(address_space, virt_addr, physical);
    }

    Some(virt_addr)
}
fn vm_free_page(virt_addr: usize){
    unsafe {
        let address_space = &mut ADDRESS_SPACES[CURRENT_ASID];
        let index = (virt_addr >> 12) & 0x1FF;
        assert!(ADDRESS_SPACES[CURRENT_ASID].l3_vmap.entries[index].is_valid());
        let entry = ADDRESS_SPACES[CURRENT_ASID].l3_vmap.entries[index].0;
        let phys_addr = (entry >> 12) << 12;
        unmap_page(address_space, virt_addr);
        free_virtual_page(virt_addr);
        free_page(phys_addr as usize);
    }
}
unsafe fn switch_address_space(id: usize) {
    assert!(id < MAX_TASKS);

    let table = &raw const ADDRESS_SPACES[id].l0 as *const PageTable as u64;
    let asid = id as u64;
    let ttbr0 = table | (asid << 48);

    core::arch::asm!(
        "dsb sy",
        "tlbi vmalle1",
        "dsb sy",
        "msr ttbr0_el1, {0}",
        "isb",
        in(reg) ttbr0,
    );
}
fn alloc_user_pages(address_space_id: usize, pages: usize) -> Option<usize> {
    if address_space_id >= MAX_TASKS || pages == 0 {
        return None;
    }

    let bytes = pages.checked_mul(PAGE_SIZE)?;

    let start = unsafe {
        USER_HEAP_NEXT[address_space_id]
    };

    let end = start.checked_add(bytes)?;

    if start < USER_HEAP_START || end > USER_HEAP_END {
        return None;
    }

    let mut mapped = 0;

    while mapped < pages {
        let physical = match alloc_page() {
            Some(address) => address,
            None => break,
        };

        let virtual_address = start + mapped * PAGE_SIZE;

        unsafe {
            map_user_page(
                &mut ADDRESS_SPACES[address_space_id],
                virtual_address,
                physical,
                false,
            );
        }

        mapped += 1;
    }

    if mapped != pages {
        unsafe {
            let address_space = &mut ADDRESS_SPACES[address_space_id];

            for i in 0..mapped {
                let virtual_address = start + i * PAGE_SIZE;
                let index = (virtual_address >> 12) & 0x1FF;
                let entry = address_space.l3_vmap.entries[index].0;
                let physical = ((entry >> 12) << 12) as usize;

                unmap_page(address_space, virtual_address);
                free_page(physical);
            }

            core::arch::asm!("dsb sy", "isb");
        }

        return None;
    }

    unsafe {
        USER_HEAP_NEXT[address_space_id] = end;
        core::arch::asm!("dsb sy", "isb");
    }

    Some(start)
}
fn validate_user_range(addr_space_id: usize, ptr: usize, len: usize, write: bool) -> bool{
    if addr_space_id >= MAX_TASKS || ptr < VMAP_START {return false;}
    let end = match ptr.checked_add(len){
        Some(end) => end,
        None => return false,
    };
    if end > VMAP_END{return false;}
    if len == 0{return true;}

    let first_page = ptr & !(PAGE_SIZE - 1);
    let last_page = (end - 1) & !(PAGE_SIZE - 1);
    let addr_spaces = core::ptr::addr_of!(ADDRESS_SPACES);
    let mut page = first_page;

    loop{
        let index = (page >> 12) & 0x1FF;
        let entry = unsafe {
            (*addr_spaces)[addr_space_id].l3_vmap.entries[index]
        };
        if !entry.is_valid(){return false;}
        let ap = (entry.0 >> 6) & 0b11;
        if ap != 0b01 && ap != 0b11{return false;}
        if write && ap != 0b01{return false;}
        if page == last_page{break;}
        page += PAGE_SIZE;
    }
    true
}
// ============================================================
// Exception decoding
// ============================================================
fn decode_esr(esr: u64) -> &'static str {
    let ec = (esr >> 26) & 0x3F;
    match ec {
        0x00 => "Unknown exception",
        0x01 => "Trapped WFI/WFE",
        0x07 => "Trapped SVE/SME",
        0x15 => "SVC instruction",
        0x16 => "HVC instruction",
        0x17 => "SMC instruction",
        0x18 => "Trapped MSR/MRS",
        0x20 => "Instruction abort from lower EL",
        0x21 => "Instruction abort from same EL",
        0x22 => "PC alignment fault",
        0x24 => "Data abort from lower EL",
        0x25 => "Data abort from same EL",
        0x26 => "SP alignment fault",
        0x2C => "FP exception",
        0x3C => "BRK instruction",
        _ => "Other synchronous exception",
    }
}
fn decode_data_abort(esr: u64) {
    let mut uart = Uart::new(0x0900_0000);
    let iss = esr & 0x01FF_FFFF;
    let isv = (iss >> 24) & 1;
    let wnr = (iss >> 6) & 1;
    let sas = (iss >> 22) & 0x3;
    let dfsc = iss & 0x3F;
    let access_size = match sas {
        0 => "byte",
        1 => "halfword",
        2 => "word",
        3 => "doubleword",
        _ => "unknown",
    };
    let fault = match dfsc {
        0x04 => "Translation fault, level 0",
        0x05 => "Translation fault, level 1",
        0x06 => "Translation fault, level 2",
        0x07 => "Translation fault, level 3",
        0x09 => "Access flag fault, level 1",
        0x0A => "Access flag fault, level 2",
        0x0B => "Access flag fault, level 3",
        0x0D => "Permission fault, level 1",
        0x0E => "Permission fault, level 2",
        0x0F => "Permission fault, level 3",
        0x10 => "Synchronous external abort",
        0x11 => "Synchronous external abort, translation table walk, level 1",
        0x12 => "Synchronous external abort, translation table walk, level 2",
        0x13 => "Synchronous external abort, translation table walk, level 3",
        _ => "Other data abort",
    };
    writeln!(uart, "Data abort details:").ok();
    writeln!(uart, "ISV = {}", isv).ok();
    writeln!(uart, "Access = {}", if wnr == 1 { "write" } else { "read" }).ok();
    writeln!(uart, "Access size = {}", access_size).ok();
    writeln!(uart, "DFSC = {:#04x}", dfsc).ok();
    writeln!(uart, "Fault = {}", fault).ok();
}
// ============================================================
// Synchronous exceptions
// ============================================================
#[unsafe(no_mangle)]
extern "C" fn exception_sync_rust(frame: &mut ExceptionFrame) {
    let mut uart = Uart::new(0x0900_0000);
    let ec = (frame.esr >> 26) & 0x3F;
    let il = (frame.esr >> 25) & 1;
    let iss = frame.esr & 0x01FF_FFFF;
    if ec == 0x15 {
        match iss {
            1 => unsafe {
                let scheduler_ptr = &raw mut SCHEDULER;
                if let Some(scheduler) = (*scheduler_ptr).as_mut() {
                    scheduler.exit_current(frame);
                    return;
                }
                panic!("SVC #1 with no scheduler");
            },
            2 => unsafe {
                let scheduler_ptr = &raw mut SCHEDULER;
                if let Some(scheduler) = (*scheduler_ptr).as_mut() {
                    scheduler.yield_current(frame);   
                    return;
                }
                panic!("SVC #2 with no scheduler");
            },
            3 => {
                let ptr = frame.x[0] as usize;
                let len = frame.x[1] as usize;
                let end = match ptr.checked_add(len) {
                    Some(end) => end,
                    None => {
                        frame.x[0] = (-1i64) as u64;
                        return;
                    }
                };
                if !validate_user_range(unsafe{CURRENT_ASID}, ptr, len, false) {
                    frame.x[0] = (-1i64) as u64;
                    return;
                }
                let bytes = unsafe {
                    core::slice::from_raw_parts(ptr as *const u8, len)
                };
                for &byte in bytes {
                    let mut uart = Uart::new(0x0900_0000);
                    uart.put_byte(byte);
                }
                frame.x[0] = len as u64;
                return;
            },
            4 => {
                let pages = frame.x[0] as usize;

                let address_space_id = unsafe {
                    CURRENT_ASID
                };

                frame.x[0] = match alloc_user_pages(
                    address_space_id,
                    pages,
                ) {
                    Some(address) => address as u64,
                    None => 0,
                };

                return;
            }
            5 => {
                frame.x[0] = match uart.try_get_byte(){
                    Some(byte) => byte as u64,
                    None => u64::MAX,
                };
                return;
            }
            6 => {
                let path_ptr = frame.x[0] as usize;
                let path_len = frame.x[1] as usize;
                let data_ptr = frame.x[2] as usize;
                let data_len = frame.x[3] as usize;

                let path_end = match path_ptr.checked_add(path_len) {
                    Some(end) => end,
                    None => {
                        frame.x[0] = u64::MAX;
                        return;
                    }
                };
                let data_end = match data_ptr.checked_add(data_len) {
                    Some(end) => end,
                    None => {
                        frame.x[0] = u64::MAX;
                        return;
                    }
                };
                if path_ptr < VMAP_START || path_end > VMAP_END || data_ptr < VMAP_START || data_end > VMAP_END {
                    frame.x[0] = u64::MAX;
                    return;
                }
                let addr_space_id = unsafe{CURRENT_ASID};
                if !validate_user_range(addr_space_id, path_ptr, path_len, false
                || !validate_user_range(addr_space_id, data_ptr, data_len, false)){
                    frame.x[0] = u64::MAX;
                    return;
                }

                let path = unsafe { core::slice::from_raw_parts(path_ptr as *const u8, path_len) };
                let data = unsafe { core::slice::from_raw_parts(data_ptr as *const u8, data_len) };
                let filesystem_ptr = &raw mut FILESYSTEM_PTR;
                unsafe {
                    if let Some(filesystem) = (*filesystem_ptr).as_mut() {
                        match filesystem.createFilePath(path, data) {
                            Ok(inode) => frame.x[0] = inode as u64,
                            Err(()) => frame.x[0] = u64::MAX,
                        }
                    } else {
                        frame.x[0] = u64::MAX;
                    }
                }
                return;
            }
            7 => {
                let path_ptr = frame.x[0] as usize;
                let path_len = frame.x[1] as usize;
                let buffer_ptr = frame.x[2] as usize;
                let buffer_len = frame.x[3] as usize;

                let path_end = match path_ptr.checked_add(path_len) {
                    Some(end) => end,
                    None => {
                        frame.x[0] = u64::MAX;
                        return;
                    }
                };
                let buffer_end = match buffer_ptr.checked_add(buffer_len) {
                    Some(end) => end,
                    None => {
                        frame.x[0] = u64::MAX;
                        return;
                    }
                };
                if path_ptr < VMAP_START || path_end > VMAP_END || buffer_ptr < VMAP_START || buffer_end > VMAP_END {
                    frame.x[0] = u64::MAX;
                    return;
                }
                let address_space_id = unsafe { CURRENT_ASID };

                if !validate_user_range(address_space_id, path_ptr, path_len, false)
                    || !validate_user_range(address_space_id, buffer_ptr, buffer_len, true)
                {
                    frame.x[0] = u64::MAX;
                    return;
                }

                let path = unsafe { core::slice::from_raw_parts(path_ptr as *const u8, path_len) };
                let buffer = unsafe { core::slice::from_raw_parts_mut(buffer_ptr as *mut u8, buffer_len) };
                let filesystem_ptr = &raw mut FILESYSTEM_PTR;
                unsafe {
                    if let Some(filesystem) = (*filesystem_ptr).as_mut() {
                        match filesystem.readFilePath(path, buffer) {
                            Ok(size) => frame.x[0] = size as u64,
                            Err(()) => frame.x[0] = u64::MAX,
                        }
                    } else {
                        frame.x[0] = u64::MAX;
                    }
                }
                return;
            }
            8 => {
                let path_ptr = frame.x[0] as usize;
                let path_len = frame.x[1] as usize;
                let data_ptr = frame.x[2] as usize;
                let data_len = frame.x[3] as usize;

                let path_end = match path_ptr.checked_add(path_len) {
                    Some(end) => end,
                    None => {
                        frame.x[0] = u64::MAX;
                        return;
                    }
                };
                let data_end = match data_ptr.checked_add(data_len) {
                    Some(end) => end,
                    None => {
                        frame.x[0] = u64::MAX;
                        return;
                    }
                };
                if path_ptr < VMAP_START || path_end > VMAP_END || data_ptr < VMAP_START || data_end > VMAP_END {
                    frame.x[0] = u64::MAX;
                    return;
                }
                let address_space_id = unsafe { CURRENT_ASID };

                if !validate_user_range(address_space_id, path_ptr, path_len, false)
                    || !validate_user_range(address_space_id, data_ptr, data_len, false)
                {
                    frame.x[0] = u64::MAX;
                    return;
                }

                let path = unsafe { core::slice::from_raw_parts(path_ptr as *const u8, path_len) };
                let data = unsafe { core::slice::from_raw_parts(data_ptr as *const u8, data_len) };
                let filesystem_ptr = &raw mut FILESYSTEM_PTR;
                unsafe {
                    if let Some(filesystem) = (*filesystem_ptr).as_mut() {
                        match filesystem.writeFilePath(path, data) {
                            Ok(size) => frame.x[0] = size as u64,
                            Err(()) => frame.x[0] = u64::MAX,
                        }
                    } else {
                        frame.x[0] = u64::MAX;
                    }
                }
                return;
            }
            9 => {
                let path_ptr = frame.x[0] as usize;
                let path_len = frame.x[1] as usize;
                let end = match path_ptr.checked_add(path_len) {
                    Some(end) => end,
                    None => {
                        frame.x[0] = u64::MAX;
                        return;
                    }
                };
                if path_ptr < VMAP_START || end > VMAP_END {
                    frame.x[0] = u64::MAX;
                    return;
                }
                let address_space_id = unsafe { CURRENT_ASID };

                if !validate_user_range(address_space_id, path_ptr, path_len, false) {
                    frame.x[0] = u64::MAX;
                    return;
                }
                let path = unsafe { core::slice::from_raw_parts(path_ptr as *const u8, path_len) };
                let filesystem_ptr = &raw mut FILESYSTEM_PTR;
                unsafe {
                    if let Some(filesystem) = (*filesystem_ptr).as_mut() {
                        match filesystem.deleteFilePath(path) {
                            Ok(()) => frame.x[0] = 0,
                            Err(()) => frame.x[0] = u64::MAX,
                        }
                    } else {
                        frame.x[0] = u64::MAX;
                    }
                }
                return;
            },
            10 =>{
                let ptr = frame.x[0] as usize;
                let len = frame.x[1] as usize;
                if len == 0 || len > 512{
                    frame.x[0] = (-1i64) as u64;
                    return;
                }
                let end = match ptr.checked_add(len){
                    Some(end) => end,
                    None =>{
                        frame.x[0] = (-1i64) as u64;
                        return;
                    }
                };
                if ptr < VMAP_START || end > VMAP_END{
                    frame.x[0] = (-1i64) as u64;
                    return;
                }
                let address_space_id = unsafe { CURRENT_ASID };

                if !validate_user_range(address_space_id, ptr, len, false) {
                    frame.x[0] = (-1i64) as u64;
                    return;
                }
                let mut path = [0u8; 512];
                unsafe{
                    core::ptr::copy_nonoverlapping(ptr as *const u8, path.as_mut_ptr(), len);
                }
                let filesystem_ptr = &raw mut FILESYSTEM_PTR;
                unsafe {
                    if let Some(filesystem) = (*filesystem_ptr).as_mut() {
                        match filesystem.statPath(&path[..len]) {
                            Ok(stat) => {
                                frame.x[0] = stat.inode as u64;
                                frame.x[1] = stat.size;
                                frame.x[2] = match stat.kind {
                                    filesystem::FileKind::File => 1,
                                    filesystem::FileKind::Directory => 2,
                                };
                                frame.x[3] = stat.links as u64;
                            }
                            Err(()) => {
                                frame.x[0] = (-1i64) as u64;
                            }
                        }
                    } else {
                        frame.x[0] = (-1i64) as u64;
                    }
                }

                return;
            },
            11 => {
                let path_ptr = frame.x[0] as usize;
                let path_len = frame.x[1] as usize;
                let end = match path_ptr.checked_add(path_len) {
                    Some(end) => end,
                    None => {
                        frame.x[0] = u64::MAX;
                        return;
                    }
                };
                if path_ptr < VMAP_START || end > VMAP_END {
                    frame.x[0] = u64::MAX;
                    return;
                }
                let address_space_id = unsafe { CURRENT_ASID };

                if !validate_user_range(address_space_id, path_ptr, path_len, false) {
                    frame.x[0] = u64::MAX;
                    return;
                }
                let path = unsafe { core::slice::from_raw_parts(path_ptr as *const u8, path_len) };
                let filesystem_ptr = &raw mut FILESYSTEM_PTR;
                unsafe {
                    if let Some(filesystem) = (*filesystem_ptr).as_mut() {
                        match filesystem.createDirectoryPath(path) {
                            Ok(inode) => frame.x[0] = inode as u64,
                            Err(()) => frame.x[0] = u64::MAX,
                        }
                    } else {
                        frame.x[0] = u64::MAX;
                    }
                }
                return;
            }
            12 => {
                let path_ptr = frame.x[0] as usize;
                let path_len = frame.x[1] as usize;
                let end = match path_ptr.checked_add(path_len) {
                    Some(end) => end,
                    None => {
                        frame.x[0] = u64::MAX;
                        return;
                    }
                };
                if path_ptr < VMAP_START || end > VMAP_END {
                    frame.x[0] = u64::MAX;
                    return;
                }
                let address_space_id = unsafe { CURRENT_ASID };

                if !validate_user_range(address_space_id, path_ptr, path_len, false) {
                    frame.x[0] = u64::MAX;
                    return;
                }
                let path = unsafe { core::slice::from_raw_parts(path_ptr as *const u8, path_len) };
                let filesystem_ptr = &raw mut FILESYSTEM_PTR;
                unsafe {
                    if let Some(filesystem) = (*filesystem_ptr).as_mut() {
                        match filesystem.removeDirectoryPath(path) {
                            Ok(()) => frame.x[0] = 0,
                            Err(()) => frame.x[0] = u64::MAX,
                        }
                    } else {
                        frame.x[0] = u64::MAX;
                    }
                }
                return;
            }
            13 => {
                let path_ptr = frame.x[0] as usize;
                let path_len = frame.x[1] as usize;
                let end = match path_ptr.checked_add(path_len) {
                    Some(end) => end,
                    None => {
                        frame.x[0] = u64::MAX;
                        return;
                    }
                };
                if path_len == 0 || path_len > 512 || path_ptr < VMAP_START || end > VMAP_END {
                    frame.x[0] = u64::MAX;
                    return;
                }
                let address_space_id = unsafe { CURRENT_ASID };

                if !validate_user_range(address_space_id, path_ptr, path_len, false) {
                    frame.x[0] = u64::MAX;
                    return;
                }
                let mut path = [0u8; 512];
                unsafe {
                    core::ptr::copy_nonoverlapping(path_ptr as *const u8, path.as_mut_ptr(), path_len);
                }

                let filesystem_ptr = &raw mut FILESYSTEM_PTR;
                let scheduler_ptr = &raw mut SCHEDULER;
                unsafe {
                    let filesystem = match (*filesystem_ptr).as_mut() {
                        Some(filesystem) => filesystem,
                        None => {
                            frame.x[0] = u64::MAX;
                            return;
                        }
                    };
                    let scheduler = match (*scheduler_ptr).as_mut() {
                        Some(scheduler) => scheduler,
                        None => {
                            frame.x[0] = u64::MAX;
                            return;
                        }
                    };
                    if scheduler.foreground.is_some() || scheduler.tasks.available_slot().is_none()
                        {
                            frame.x[0] = u64::MAX;
                            return;
                        }

                    let run_buffer = unsafe {
                        core::slice::from_raw_parts_mut(
                            core::ptr::addr_of_mut!(RUN_LOAD_BUFFER).cast::<u8>(),
                            MAX_RUN_SIZE,
                        )
                    };
                    let run_size = match filesystem.readFilePath(&path[..path_len], run_buffer) {
                        Ok(size) => size,
                        Err(()) => {
                            frame.x[0] = u64::MAX;
                            return;
                        }
                    };

                    if run_size == 0 || run_size > MAX_RUN_SIZE {
                        frame.x[0] = u64::MAX;
                        return;
                    }

                    let run_data = core::slice::from_raw_parts(
                        core::ptr::addr_of!(RUN_LOAD_BUFFER).cast::<u8>(),
                        run_size,
                    );

                    if !validate_run(run_data) {
                        frame.x[0] = u64::MAX;
                        return;
                    }

                    if !scheduler.spawn_foreground_user(run_data, frame) {
                        frame.x[0] = u64::MAX;
                    }

                    return;
                }
                return;
            }
            14 => {
                let path_ptr = frame.x[0] as usize;
                let path_len = frame.x[1] as usize;
                let buffer_ptr = frame.x[2] as usize;
                let buffer_len = frame.x[3] as usize;

                if path_len == 0
                    || path_len > 512
                    || buffer_len == 0
                    || buffer_len > MAX_RUN_SIZE
                {
                    frame.x[0] = u64::MAX;
                    return;
                }

                let path_end = match path_ptr.checked_add(path_len) {
                    Some(end) => end,
                    None => {
                        frame.x[0] = u64::MAX;
                        return;
                    }
                };

                let buffer_end = match buffer_ptr.checked_add(buffer_len) {
                    Some(end) => end,
                    None => {
                        frame.x[0] = u64::MAX;
                        return;
                    }
                };

                if path_ptr < VMAP_START
                    || path_end > VMAP_END
                    || buffer_ptr < VMAP_START
                    || buffer_end > VMAP_END
                {
                    frame.x[0] = u64::MAX;
                    return;
                }
                let address_space_id = unsafe { CURRENT_ASID };

                if !validate_user_range(address_space_id, path_ptr, path_len, false)
                    || !validate_user_range(address_space_id, buffer_ptr, buffer_len, true)
                {
                    frame.x[0] = u64::MAX;
                    return;
                }

                // Copy the path before accessing the output buffer.
                let mut path = [0u8; 512];
                unsafe {
                    core::ptr::copy_nonoverlapping(
                        path_ptr as *const u8,
                        path.as_mut_ptr(),
                        path_len,
                    );
                }

                let output = unsafe {
                    core::slice::from_raw_parts_mut(
                        buffer_ptr as *mut u8,
                        buffer_len,
                    )
                };

                let filesystem_ptr = &raw mut FILESYSTEM_PTR;

                unsafe {
                    let filesystem = match (*filesystem_ptr).as_mut() {
                        Some(filesystem) => filesystem,
                        None => {
                            frame.x[0] = u64::MAX;
                            return;
                        }
                    };

                    let stat = match filesystem.statPath(&path[..path_len]) {
                        Ok(stat) => stat,
                        Err(()) => {
                            frame.x[0] = u64::MAX;
                            return;
                        }
                    };

                    if stat.kind != filesystem::FileKind::Directory {
                        frame.x[0] = u64::MAX;
                        return;
                    }

                    let mut written = 0usize;
                    let mut overflow = false;

                    let result = filesystem.listDirectory(
                        stat.inode,
                        |name, _inode, _kind| {
                            if overflow {
                                return;
                            }

                            // Each entry is a name followed by '\n'.
                            let entry_len = match name.len().checked_add(1) {
                                Some(length) => length,
                                None => {
                                    overflow = true;
                                    return;
                                }
                            };

                            let end = match written.checked_add(entry_len) {
                                Some(end) => end,
                                None => {
                                    overflow = true;
                                    return;
                                }
                            };

                            if end > output.len() {
                                overflow = true;
                                return;
                            }

                            output[written..written + name.len()]
                                .copy_from_slice(name);

                            output[written + name.len()] = b'\n';
                            written = end;
                        },
                    );

                    if result.is_err() || overflow {
                        frame.x[0] = u64::MAX;
                    } else {
                        frame.x[0] = written as u64;
                    }
                }

                return;
            }
            15 => {
                let filesystem_ptr = &raw mut FILESYSTEM_PTR;
                unsafe {
                    let filesystem = match (*filesystem_ptr).as_mut() {
                        Some(filesystem) => filesystem,
                        None => {
                            frame.x[0] = u64::MAX;
                            return;
                        }
                    };

                    match filesystem.disk_usage() {
                        Ok((total_blocks, used_blocks, free_blocks, block_size)) => {
                            frame.x[0] = total_blocks;
                            frame.x[1] = used_blocks;
                            frame.x[2] = free_blocks;
                            frame.x[3] = block_size;
                        }
                        Err(()) => {
                            frame.x[0] = u64::MAX;
                        }
                    }
                }
                return;
            }
            42 => {
                writeln!(uart, "SVC #42 triggered. Life, the universe and everything?");
                return;
            }
            _ => {}
        }
    }
    let sctlr: u64;
    let vbar: u64;
    let currentel: u64;
    unsafe {
        core::arch::asm!(
            "mrs {0}, sctlr_el1",
            "mrs {1}, vbar_el1",
            "mrs {2}, currentel",
            out(reg) sctlr,
            out(reg) vbar,
            out(reg) currentel,
        );
    }
    writeln!(uart, "SCTLR_EL1 = {:#018x}", sctlr).ok();
    writeln!(uart, "VBAR_EL1 = {:#018x}", vbar).ok();
    writeln!(uart, "CURRENTEL = {:#018x}", currentel).ok();
    writeln!(uart, "=== SYNCHRONOUS EXCEPTION ===").ok();
    writeln!(uart, "Cause   = {}", decode_esr(frame.esr)).ok();
    writeln!(uart, "EC      = {:#04x}", ec).ok();
    writeln!(uart, "IL      = {}", il).ok();
    writeln!(uart, "ISS     = {:#08x}", iss).ok();
    writeln!(uart, "X0      = {:#018x}", frame.x[0]).ok();
    writeln!(uart, "X1      = {:#018x}", frame.x[1]).ok();
    writeln!(uart, "X30     = {:#018x}", frame.x[30]).ok();
    writeln!(uart, "SP      = {:#018x}", frame.sp).ok();
    writeln!(uart, "ELR     = {:#018x}", frame.elr).ok();
    writeln!(uart, "SPSR    = {:#018x}", frame.spsr).ok();
    writeln!(uart, "ESR     = {:#018x}", frame.esr).ok();
    writeln!(uart, "FAR     = {:#018x}", frame.far).ok();
    if ec == 0x25 && frame.far == 0xDEAD_0000 {
        decode_data_abort(frame.esr);
        writeln!(uart, "Recovering from data abort...").ok();
        frame.elr += 4;
        return;
    }
    if ec == 0x3C {
        frame.elr += 4;
        return;
    }
    writeln!(uart, "KERNEL PANIC! Unhandled exception!").ok();
    loop {
        unsafe {
            core::arch::asm!("wfe");
        }
    }
}
// ============================================================
// IRQ
// ============================================================
#[unsafe(no_mangle)]
extern "C" fn exception_irq_rust(frame: &mut ExceptionFrame, interrupt_id: u32) {
    if interrupt_id != 30 {
        return;
    }
    let _tick = TICKS.fetch_add(1, Ordering::Relaxed) + 1;
    Timer::set_timeout(Timer::frequency() / 10);
    unsafe {
        let scheduler_ptr = &raw mut SCHEDULER;
        if let Some(scheduler) = (*scheduler_ptr).as_mut() {
            scheduler.tick(frame);
        }
    }
}
// ============================================================
// FIQ
// ============================================================
#[unsafe(no_mangle)]
extern "C" fn exception_fiq_rust(frame: &mut ExceptionFrame) {
    let mut uart = Uart::new(0x0900_0000);
    writeln!(uart, "=== FIQ ===").ok();
    writeln!(uart, "ELR = {:#018x}", frame.elr).ok();
    writeln!(uart, "SP  = {:#018x}", frame.sp).ok();
}
// ============================================================
// SError
// ============================================================
#[unsafe(no_mangle)]
extern "C" fn exception_serror_rust() -> ! {
    let mut uart = Uart::new(0x0900_0000);
    writeln!(uart, "SError exception!").ok();
    loop {
        unsafe {
            core::arch::asm!("wfe");
        }
    }
}
// ============================================================
// Kernel entry
// ============================================================
#[unsafe(no_mangle)]
pub extern "C" fn rust_start() -> ! {
    let mut uart = Uart::new(0x0900_0000);
    writeln!(uart, "Starting...").unwrap();
    writeln!(uart, "Enabling FP/SIMD...").unwrap();
    unsafe {
        core::arch::asm!(
            "mrs x0, cpacr_el1",
            "orr x0, x0, #(3 << 20)",
            "msr cpacr_el1, x0",
            "isb",
        );
    }
    writeln!(uart, "Enabled FP/SIMD!").unwrap();
    writeln!(uart, "Installing exception vector....").unwrap();
    unsafe {
        let vector = exception_vector as *const ();
        core::arch::asm!(
            "msr VBAR_EL1, {}",
            "isb",
            in(reg) vector,
        );
    }
    writeln!(uart, "Vector installed!").unwrap();
    writeln!(uart, "Initialising memory allocator").unwrap();
    init_memory();
    writeln!(uart, "Memory allocator initialised!").unwrap();
    writeln!(uart, "Initialising MMU...").unwrap();
    unsafe {
        set_page_table();
    }
    writeln!(uart, "Page tables configured!").unwrap();
    init_mair();
    writeln!(uart, "MAIR configured!").unwrap();
    init_tcr();
    writeln!(uart, "TCR configured!").unwrap();
    init_ttbr0();
    writeln!(uart, "TTBR0 configured!").unwrap();
    unsafe {
        let tcr: u64;
        let ttbr0: u64;
        let mair: u64;
        let sctlr: u64;
        core::arch::asm!(
            "mrs {0}, tcr_el1",
            "mrs {1}, ttbr0_el1",
            "mrs {2}, mair_el1",
            "mrs {3}, sctlr_el1",
            out(reg) tcr,
            out(reg) ttbr0,
            out(reg) mair,
            out(reg) sctlr,
        );
    }
    writeln!(uart, "Enabling MMU...").unwrap();
    let sp: u64;
    unsafe {
        core::arch::asm!(
            "mov {0}, sp",
            out(reg) sp,
        );
    }
    enable_mmu();

    writeln!(uart, "MMU enabled!").unwrap();
    writeln!(uart, "Scanning VirtIO MMIO devices...").unwrap();

    for base in (0x0A00_0000usize..=0x0A00_3E00).step_by(0x200) {
        unsafe {
            let magic = core::ptr::read_volatile(base as *const u32);
            let version = core::ptr::read_volatile((base + 4) as *const u32);
            let device_id = core::ptr::read_volatile((base + 8) as *const u32);

            if magic == 0x7472_6976 && device_id != 0 {
                writeln!(
                    uart,
                    "VirtIO at {:#x}: version={}, device={}",
                    base,
                    version,
                    device_id
                ).unwrap();
            }
        }
    }
    writeln!(uart, "Initialising VirtIO GPU...").unwrap();

    let gpu = VirtioGpu::new();
    let (magic, version, device_id) = gpu.probe();

    writeln!(
        uart,
        "GPU: magic={:#x} version={} device={}",
        magic,
        version,
        device_id
    ).unwrap();

    match gpu.init() {
        Ok(()) => {
            writeln!(uart, "VirtIO GPU initialised!").unwrap();
        }
        Err(error) => {
            writeln!(uart, "VirtIO GPU init failed: {:?}", error).unwrap();
        }
    }
    match gpu.get_display_info() {
        Ok((width, height)) => {
            writeln!(uart, "GPU display: {}x{}", width, height).unwrap();
        }
        Err(error) => {
            writeln!(uart, "GPU display query failed: {:?}", error).unwrap();
        }
    }
    writeln!(uart, "Initialising virtio block device...").unwrap();

    let disk = VirtioBlk::new();
    let (magic, version, device_id) = disk.probe();

    writeln!(
        uart,
        "Virtio: magic={:#x} version={} device={}",
        magic,
        version,
        device_id
    ).unwrap();
    match disk.init(){
        Ok(()) => {
            writeln!(uart, "Virtio block device initialised!").unwrap();
            let sectors = disk.capacity_sectors();
            let blocks = disk.capacity_blocks();
            writeln!(uart, "Disk sectors: {}", sectors).unwrap();
            writeln!(uart, "4 KiB blocks: {}", blocks).unwrap();

            let test_block = blocks -1;

            let mut write_buffer = [0u8; 4096];
            let mut read_buffer = [0u8; 4096];
            write_buffer[0..8].copy_from_slice(b"OUROTEST");

            match disk.write_block(test_block, &write_buffer){
                Ok(()) => {
                    writeln!(uart, "Block write: PASS").unwrap();
                }
                Err(error) => {
                    writeln!(uart, "Block write: FAIL {:?}", error).unwrap();
                    loop{unsafe{core::arch::asm!("wfe");}}
                }

            }
            match disk.read_block(test_block, &mut read_buffer) {
                Ok(()) => {
                    if &read_buffer[0..8] == b"OUROTEST" {
                        writeln!(uart, "Block read: PASS").unwrap();
                    } else {
                        writeln!(uart, "Block read: FAIL (bad data)").unwrap();

                        loop {
                            unsafe {
                                core::arch::asm!("wfe");
                            }
                        }
                    }
                }

                Err(error) => {
                    writeln!(uart, "Block read: FAIL {:?}", error).unwrap();

                    loop {
                        unsafe {
                            core::arch::asm!("wfe");
                        }
                    }
                }
            }

            writeln!(uart, "Virtio block test complete!").unwrap();
        }
        Err(error) => {
            writeln!(uart, "Virtio init failed: {:?}", error).unwrap();
            loop{unsafe{core::arch::asm!("wfe");}}
        }
    }
    writeln!(uart, "Initialising OuroFS...").unwrap();

    let mut filesystem = filesystem::FileSystem::new(disk);
    unsafe {
        FILESYSTEM_PTR = &mut filesystem;
    }
    match filesystem.mount() {
        Ok(superblock) => {
            writeln!(uart, "OuroFS mounted!").unwrap();
            writeln!(uart, "Filesystem blocks: {}", superblock.total_blocks).unwrap();
            writeln!(uart, "Block size: {}", superblock.block_size).unwrap();
            writeln!(uart, "Inodes: {}", superblock.inode_count).unwrap();
        }

        Err(()) => {
            writeln!(uart, "No valid OuroFS found. Formatting...").unwrap();

            match filesystem.format() {
                Ok(()) => {
                    writeln!(uart, "OuroFS formatted!").unwrap();

                    match filesystem.mount() {
                        Ok(superblock) => {
                            writeln!(uart, "OuroFS mounted after formatting!").unwrap();
                            writeln!(uart, "Filesystem blocks: {}", superblock.total_blocks).unwrap();
                            writeln!(uart, "Block size: {}", superblock.block_size).unwrap();
                            writeln!(uart, "Inodes: {}", superblock.inode_count).unwrap();
                        }

                        Err(()) => {
                            writeln!(uart, "ERROR: Could not mount newly formatted OuroFS!").unwrap();
                            loop{unsafe{core::arch::asm!("wfe");}}
                        }
                    }
                }

                Err(()) => {
                    writeln!(uart, "ERROR: OuroFS formatting failed!").unwrap();
                    loop{unsafe{core::arch::asm!("wfe");}}
                }
            }
        }
    }
    // Read the executable from the filesystem into static storage. Do not
    // use a large local array here: the kernel stack is only 16 KiB.
    let runSize = unsafe {
        let buffer = &raw mut RUN_LOAD_BUFFER;

        filesystem
            .readFilePath(b"/bin/user-init.run", &mut *buffer)
            .expect("Failed to read /bin/user-init.run from OuroFS")
    };

    assert!(runSize <= MAX_RUN_SIZE, "user-init.run exceeds load buffer");

    let runData: &[u8] = unsafe {
        core::slice::from_raw_parts(
            core::ptr::addr_of!(RUN_LOAD_BUFFER).cast::<u8>(),
            runSize,
        )
    };

    writeln!(uart, "Loaded {} bytes from /bin/user-init.run.", runSize).unwrap();

    unsafe {
        let scheduler = &raw mut SCHEDULER;
        (*scheduler) = Some(Scheduler::new());
        if let Some(scheduler) = (*scheduler).as_mut() {
            scheduler.add_task(idle_task);
            scheduler.add_user_task(runData);
        }
    }
    writeln!(uart, "Scheduler initialised!").unwrap();
    writeln!(uart, "Enabling GIC...").unwrap();
    let gic = Gic::new();
    gic.enable();
    writeln!(uart, "GIC enabled!").unwrap();
    writeln!(uart, "Reading timer...").unwrap();
    let freq = Timer::frequency();
    let _counter = Timer::counter();
    writeln!(uart, "Arming timer...").unwrap();
    Timer::set_timeout(freq / 10);
    Timer::enable();
    writeln!(uart, "Timer armed!").unwrap();
    writeln!(uart, "Enabling CPU IRQs and passing to scheduler...").unwrap();
    unsafe {
        core::arch::asm!("msr daifclr, #2");
    }
    writeln!(uart, "CPU IRQs enabled!").unwrap();
    writeln!(uart, "Scheduler loaded, placeholder wfe").unwrap();
    loop {
        unsafe {
            core::arch::asm!("wfe");
        }
    }
}
// ============================================================
// Panic
// ============================================================
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    let mut uart = Uart::new(0x0900_0000);
    uart.write_bytes("KERNEL PANIC!\r\n");
    loop {
        unsafe {
            core::arch::asm!("wfe");
        }
    }
}