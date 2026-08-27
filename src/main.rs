#![no_std]
#![no_main]
#![allow(warnings)]
mod exception;
use exception::ExceptionFrame;
core::arch::global_asm!(include_str!("boot.S"));
core::arch::global_asm!(include_str!("exceptions.S"));
use core::fmt::{self, Write};
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
            (self.base as *mut u8).write_volatile(byte);
        }
    }
    fn write_bytes(&self, s: &str) {
        for &byte in s.as_bytes() {
            self.put_byte(byte);
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
const MAX_TASKS: usize = 4;
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
}
#[repr(C)]
struct Task {
    id: usize,
    state: TaskState,
    context: CpuContext,
}
const SPSR_EL1H: u64 = 0b0101;
#[repr(align(16))]
#[derive(Copy, Clone)]
struct TaskStack([u8; TASK_STACK_SIZE]);
#[unsafe(no_mangle)]
#[unsafe(link_section = ".task_stacks")]
static mut TASK_STACKS: [TaskStack; MAX_TASKS] =
    [TaskStack([0; TASK_STACK_SIZE]); MAX_TASKS];
impl Task {
    fn new(id: usize, entry: fn()) -> Self {
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
            context: CpuContext {
                x: [0; 31],
                sp: initial_sp,
                pc: task_trampoline as usize as u64,
                spsr: SPSR_EL1H,
            },
        };
        task.context.x[0] = entry as usize as u64;
        task.context.x[30] = task_exit as usize as u64;
        task.context.x[1..].fill(0);
        task
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
        if a == 100 {
            task_exit();
        }
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
        if b == 2000 {
            task_exit();
        }
    }
}
fn task_c() { //RAM test
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
        writeln!(uart, "").unwrap();
        writeln!(uart, "Testing page bitmap...").unwrap();
        let mut bitmap_passed = true;
        if is_page_used(0) {
            writeln!(uart, "FAIL: page 0 starts used").unwrap();
            bitmap_passed = false;
        }
        mark_page_used(0);
        if !is_page_used(0) {
            writeln!(uart, "FAIL: page 0 was not marked used").unwrap();
            bitmap_passed = false;
        }
        mark_page_free(0);
        if is_page_used(0) {
            writeln!(uart, "FAIL: page 0 was not marked free").unwrap();
            bitmap_passed = false;
        }
        mark_page_used(7);
        mark_page_used(8);
        if is_page_used(6) {
            writeln!(uart, "FAIL: page 6 incorrectly marked used").unwrap();
            bitmap_passed = false;
        }
        if !is_page_used(7) {
            writeln!(uart, "FAIL: page 7 was not marked used").unwrap();
            bitmap_passed = false;
        }
        if !is_page_used(8) {
            writeln!(uart, "FAIL: page 8 was not marked used").unwrap();
            bitmap_passed = false;
        }
        if is_page_used(9) {
            writeln!(uart, "FAIL: page 9 incorrectly marked used").unwrap();
            bitmap_passed = false;
        }
        mark_page_free(7);
        mark_page_free(8);
        if bitmap_passed {
            writeln!(uart, "PASS: page bitmap").unwrap();
        } else {
            writeln!(uart, "FAIL: page bitmap").unwrap();
        }
        writeln!(uart, "").unwrap();
        writeln!(uart, "Testing single page allocation...").unwrap();
        let first_page = alloc_page();
        match first_page {
            Some(addr) => {
                writeln!(uart, "Allocated page: {:#x}", addr).unwrap();
                let expected = FIRST_FREE_PAGE;
                if addr == expected {
                    writeln!(uart, "PASS: first page address").unwrap();
                } else {
                    writeln!(uart, "FAIL: expected {:#x}, got {:#x}", expected, addr).unwrap();
                }
                let page = (addr - FIRST_FREE_PAGE) / PAGE_SIZE;
                if is_page_used(page) {
                    writeln!(uart, "PASS: allocated page marked used").unwrap();
                } else {
                    writeln!(uart, "FAIL: allocated page still free").unwrap();
                }
                free_page(addr);
                if !is_page_used(page) {
                    writeln!(uart, "PASS: freed page marked free").unwrap();
                } else {
                    writeln!(uart, "FAIL: freed page still used").unwrap();
                }
            }
            None => {
                writeln!(uart, "FAIL: could not allocate page").unwrap();
            }
        }
        writeln!(uart, "").unwrap();
        writeln!(uart, "Testing multiple page allocation...").unwrap();
        let a = alloc_page();
        let b = alloc_page();
        let c = alloc_page();
        match (a, b, c) {
            (Some(a), Some(b), Some(c)) => {
                writeln!(uart, "Page A: {:#x}", a).unwrap();
                writeln!(uart, "Page B: {:#x}", b).unwrap();
                writeln!(uart, "Page C: {:#x}", c).unwrap();
                let mut passed = true;
                if a == b {
                    writeln!(uart, "FAIL: A == B").unwrap();
                    passed = false;
                }
                if a == c {
                    writeln!(uart, "FAIL: A == C").unwrap();
                    passed = false;
                }
                if b == c {
                    writeln!(uart, "FAIL: B == C").unwrap();
                    passed = false;
                }
                if passed {
                    writeln!(uart, "PASS: allocations are unique").unwrap();
                }
                free_page(a);
                free_page(b);
                free_page(c);
                writeln!(uart, "Freed A, B, C").unwrap();
            }
            _ => {
                writeln!(uart, "FAIL: could not allocate three pages").unwrap();
                if let Some(addr) = a {
                    free_page(addr);
                }
                if let Some(addr) = b {
                    free_page(addr);
                }
                if let Some(addr) = c {
                    free_page(addr);
                }
            }
        }
        writeln!(uart, "").unwrap();
        writeln!(uart, "Testing page reuse...").unwrap();
        let a = alloc_page();
        match a {
            Some(a) => {
                writeln!(uart, "First allocation: {:#x}", a).unwrap();
                free_page(a);
                writeln!(uart, "Freed page").unwrap();
                let b = alloc_page();
                match b {
                    Some(b) => {
                        writeln!(uart, "Second allocation: {:#x}", b).unwrap();
                        if a == b {
                            writeln!(uart, "PASS: freed page was reused").unwrap();
                        } else {
                            writeln!(uart, "FAIL: expected {:#x}, got {:#x}", a, b).unwrap();
                        }
                        free_page(b);
                    }
                    None => {
                        writeln!(uart, "FAIL: could not reallocate page").unwrap();
                    }
                }
            }
            None => {
                writeln!(uart, "FAIL: initial allocation failed").unwrap();
            }
        }
        writeln!(uart, "").unwrap();
        writeln!(uart, "Testing actual RAM access...").unwrap();
        let page = alloc_page();
        match page {
            Some(addr) => {
                writeln!(uart, "Allocated test page: {:#x}", addr).unwrap();
                let ptr = addr as *mut u8;
                for i in 0..PAGE_SIZE {
                    ptr.add(i).write_volatile(0xAA);
                }
                writeln!(uart, "Wrote 0xAA to entire page").unwrap();
                let mut passed = true;
                for i in 0..PAGE_SIZE {
                    let value = ptr.add(i).read_volatile();
                    if value != 0xAA {
                        writeln!(uart, "FAIL: byte {} = {:#x}", i, value).unwrap();
                        passed = false;
                        break;
                    }
                }
                if passed {
                    writeln!(uart, "PASS: entire page read back correctly").unwrap();
                }
                for i in 0..PAGE_SIZE {
                    ptr.add(i).write_volatile(0x55);
                }
                writeln!(uart, "Wrote 0x55 to entire page").unwrap();
                let mut passed = true;
                for i in 0..PAGE_SIZE {
                    let value = ptr.add(i).read_volatile();
                    if value != 0x55 {
                        writeln!(uart, "FAIL: byte {} = {:#x}", i, value).unwrap();
                        passed = false;
                        break;
                    }
                }
                if passed {
                    writeln!(uart, "PASS: second RAM pattern").unwrap();
                }
                free_page(addr);
                writeln!(uart, "Test page freed").unwrap();
            }
            None => {
                writeln!(uart, "FAIL: could not allocate RAM test page").unwrap();
            }
        }
        writeln!(uart, "L0[0] = {:#018x}", PAGE_TABLE_L0.entries[0].0).unwrap();
        writeln!(uart, "L1[0] = {:#018x}", PAGE_TABLE_L1_RAM.entries[0].0).unwrap();
        writeln!(uart, "L2[0] = {:#018x}", PAGE_TABLE_L2_RAM.entries[0].0).unwrap();
        writeln!(uart, "L2[1] = {:#018x}", PAGE_TABLE_L2_RAM.entries[1].0).unwrap();
        writeln!(uart, "").unwrap();
        unsafe {
            let physical = alloc_page();

            match physical {
                Some(physical) => {
                    writeln!(
                        uart,
                        "Allocated physical page: {:#x}",
                        physical
                    ).unwrap();

                    map_page(VMAP_START, physical);

                    writeln!(
                        uart,
                        "Mapped {:#x} -> {:#x}",
                        VMAP_START,
                        physical
                    ).unwrap();

                    let ptr = VMAP_START as *mut u64;

                    ptr.write_volatile(0x1234_5678_9ABC_DEF0);

                    let value = ptr.read_volatile();

                    writeln!(
                        uart,
                        "Read through VA: {:#x}",
                        value
                    ).unwrap();

                    free_page(physical);
                }

                None => {
                    writeln!(uart, "Failed to allocate test page").unwrap();
                }
            }
        }
        writeln!(uart, "================================").unwrap();
        writeln!(uart, "          RAM TESTED            ").unwrap();
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
// Scheduler
// ============================================================
struct Scheduler {
    tasks: TaskTable,
    current: usize,
    started: bool,
}
impl Scheduler {
    fn new() -> Self {
        Self {
            tasks: TaskTable::new(),
            current: 0,
            started: false,
        }
    }
    fn add_task(&mut self, entry: fn()) -> usize {
        self.tasks.create(entry)
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
        if let Some(task) = &mut self.tasks.tasks[self.current] {
            task.state = TaskState::Running;
            task.load_context(frame);
        }
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
        self.current = next;
        if let Some(task) = &mut self.tasks.tasks[next] {
            task.state = TaskState::Running;
            task.load_context(frame);
        }
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
        self.current = next;
        if let Some(task) = &mut self.tasks.tasks[next] {
            task.state = TaskState::Running;
            task.load_context(frame);
        }
    }
    fn exit_current(&mut self, frame: &mut ExceptionFrame) {
        if self.tasks.count == 0 {
            panic!("No tasks");
        }
        if self.current == 0 {
            panic!("Idle task cannot exit");
        }
        let dead = self.current;
        if let Some(task) = &mut self.tasks.tasks[dead] {
            task.state = TaskState::Dead;
        }
        for offset in 1..self.tasks.count {
            let id = (dead + offset) % self.tasks.count;
            if id == 0 {
                continue;
            }
            if let Some(task) = &self.tasks.tasks[id] {
                if task.state == TaskState::Ready {
                    self.current = id;
                    if let Some(task) = &mut self.tasks.tasks[id] {
                        task.state = TaskState::Running;
                        task.load_context(frame);
                    }
                    return;
                }
            }
        }
        self.current = 0;
        if let Some(idle) = &mut self.tasks.tasks[0] {
            idle.state = TaskState::Running;
            idle.load_context(frame);
        }
    }
}
// ============================================================
// Scheduler storage
// ============================================================
#[unsafe(no_mangle)]
#[unsafe(link_section = ".scheduler")]
static mut SCHEDULER: Option<Scheduler> = None;
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
        writeln!(uart, "GIC enabled! Returning to main...").ok();
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
unsafe fn map_page(virt_addr: usize, phys_addr: usize){
    assert!(virt_addr % PAGE_SIZE == 0);
    assert!(phys_addr % PAGE_SIZE == 0);
    assert!(virt_addr >= VMAP_START);
    assert!(virt_addr < VMAP_END);

    let index = (virt_addr >> 12) & 0x1FF;
    assert!(!PAGE_TABLE_L3_VMAP.entries[index].is_valid());
    PAGE_TABLE_L3_VMAP.entries[index] = 
        PageTableEntry::new_page(
            phys_addr,
            ATTR_NORMAL,
            0b00,
            0b11,
            false
        )
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
    writeln!(uart, "Initialising scheduler...").unwrap();
    unsafe {
        let scheduler = &raw mut SCHEDULER;
        (*scheduler) = Some(Scheduler::new());
        if let Some(scheduler) = (*scheduler).as_mut() {
            scheduler.add_task(idle_task);
            //scheduler.add_task(task_a);
            //scheduler.add_task(task_b);
            scheduler.add_task(task_c);
        }
    }
    writeln!(uart, "Scheduler initialised!").unwrap();
    writeln!(uart, "Enabling GIC...").unwrap();
    let gic = Gic::new();
    gic.enable();
    writeln!(uart, "GIC enabled! Returned to main!").unwrap();
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
    writeln!(uart, "Scheduler has crashed! Loading to wfe").unwrap();
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