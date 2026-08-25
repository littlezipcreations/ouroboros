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

// Each task gets a permanently located stack.
// These never move, so pointers into them remain valid.
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

        // x0 = entry function
        task.context.x[0] = entry as usize as u64;

        // Return from task_trampoline -> task_exit
        task.context.x[30] = task_exit as usize as u64;
        task.context.x[1..].fill(0); //Make sure it actually is ALL ZEROS!

        task
    }
}

impl Task {
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

fn task_a() {
    let mut uart = Uart::new(0x0900_0000);
    let mut A = 0u64;
    loop{
        for _ in 0..500_000{ core::hint::spin_loop();}
        unsafe {
            A = A.wrapping_add(1);
            writeln!(uart, "A value = {:#x}", A);
        }
        if A == 100u64{task_exit();}
    }
}
fn task_b() {
    let mut uart = Uart::new(0x0900_0000);
    let mut B = 1000u64;
    loop{
        for _ in 0..500_000{core::hint::spin_loop();}
        unsafe{
            B = B.wrapping_add(1);
            writeln!(uart, "B value = {:#x}", B);
        }
        if B == 2000u64{task_exit();}
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

//RAM

const PAGE_SIZE: usize = 4096; //4KiB
const RAM_START: usize = 0x4000_0000;
const RAM_END: usize = 0x6000_0000;
const KERNEL_END: usize = 0x4000_C000;
const FIRST_FREE_PAGE: usize = KERNEL_END;
const PAGE_COUNT: usize = (RAM_END - FIRST_FREE_PAGE) / PAGE_SIZE;
const BITMAP_SIZE: usize = (PAGE_COUNT + 7) / 8;
static mut PAGE_BITMAP: [u8; BITMAP_SIZE] = [0; BITMAP_SIZE];
unsafe extern "C"{
    static _kernel_start: u8;
    static _kernel_end: u8;
}
fn kernel_ram_helper(){
    let mut uart = Uart::new(0x0900_0000);
    let start = unsafe { &_kernel_start as *const u8 as usize};
    let end = unsafe { &_kernel_end as *const u8 as usize};
    writeln!(uart, "KERNEL START = {:#018x}", start).ok();
    writeln!(uart, "KERNEL END = {:#018x}", end).ok();
}
fn is_page_used(page: usize) -> bool {
    let byte = page / 8;
    let bit = page % 8;
    let mask = 1u8 << bit;

    unsafe {
        (PAGE_BITMAP[byte] & mask) != 0
    }
}
fn mark_page_used(page: usize) {
    let byte = page / 8;
    let bit = page % 8;
    let mask = 1u8 << bit;

    unsafe {
        PAGE_BITMAP[byte] |= mask;
    }
}
fn mark_page_free(page: usize) {
    let byte = page / 8;
    let bit = page % 8;
    let mask = 1u8 << bit;

    unsafe {
        PAGE_BITMAP[byte] &= !mask;
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
    let tick = TICKS.fetch_add(1, Ordering::Relaxed) + 1;
    Timer::set_timeout(Timer::frequency() / 10);
    unsafe {
        let scheduler_ptr = &raw mut SCHEDULER;
        if let Some(scheduler) = (*scheduler_ptr).as_mut() {
            scheduler.tick(frame);
            let mut uart = Uart::new(0x0900_0000);
            writeln!(
                uart,
                "TICK {} -> TASK {} frame_pc={:#018x} saved_pc={:#018x} SP={:#018x}",
                tick,
                scheduler.current,
                frame.elr,
                scheduler.tasks.tasks[scheduler.current]
                    .as_ref()
                    .map(|t| t.context.pc)
                    .unwrap_or(0),
                frame.sp,
            )
            .ok();
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
    writeln!(uart, "Initialising scheduler...").unwrap();
    unsafe {
        let scheduler = &raw mut SCHEDULER;
        (*scheduler) = Some(Scheduler::new());
        if let Some(scheduler) = (*scheduler).as_mut() {
            scheduler.add_task(idle_task);
            //scheduler.add_task(task_a);
            //scheduler.add_task(task_b);
        }
    }
    writeln!(uart, "Scheduler initialised!").unwrap();
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
    writeln!(uart, "Checking RAM...").unwrap();
    writeln!(uart, "PAGE_SIZE: {}", PAGE_SIZE).unwrap(); 
    writeln!(uart, "RAM_START: {}", RAM_START).unwrap();
    writeln!(uart, "RAM_END: {}", RAM_END).unwrap();
    writeln!(uart, "KERNEL_END: {}", KERNEL_END).unwrap();
    writeln!(uart, "FIRST_FREE_PAGE: {}", FIRST_FREE_PAGE).unwrap();
    writeln!(uart, "PAGE_COUNT: {}", PAGE_COUNT).unwrap();
    writeln!(uart, "BITMAP_SIZE: {}", BITMAP_SIZE).unwrap();
    writeln!(uart, "Enabling CPU IRQs and passing to scheduler...").unwrap();
    unsafe {
        core::arch::asm!("msr daifclr, #2");
    }
    writeln!(uart, "CPU IRQs enabled!").unwrap();
    writeln!(uart, "Calling WFE...").unwrap();
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