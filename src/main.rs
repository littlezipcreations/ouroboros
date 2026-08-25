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
    fn exception_sync_entry();
}

// ============================================================
// Kernel stack
// ============================================================

#[repr(align(16))]
struct KernelStack([u8; 16 * 1024]);

#[unsafe(no_mangle)]
static mut STACK: KernelStack = KernelStack([0; 16 * 1024]);

// ============================================================
// Tasks
// ============================================================

const TASK_STACK_SIZE: usize = 16 * 1024;

#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq)]
enum TaskState {
    Ready,
    Running,
    Dead,
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
    stack: [u8; TASK_STACK_SIZE],
}

impl Task {
    fn new(id: usize, entry: fn()) -> Self {
        let mut task = Self {
            id,
            state: TaskState::Ready,

            context: CpuContext {
                x: [0; 31],

                // Filled in after the task is moved into the task table.
                sp: 0,

                // Every task starts through the trampoline.
                pc: task_trampoline as usize as u64,

                spsr: SPSR_EL1H,
            },

            stack: [0; TASK_STACK_SIZE],
        };

        // Pass the real task entry function in x0.
        task.context.x[0] = entry as usize as u64;

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

const MAX_TASKS: usize = 4;

const SPSR_EL1H: u64 = 0b0101;

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

        // The task has now been moved into its final location.
        // Calculate the stack pointer from the actual stored task.
        let task = self.tasks[id].as_mut().unwrap();

        let stack_top =
            task.stack.as_ptr() as u64 + task.stack.len() as u64;

        task.context.sp = stack_top & !0xF;

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

    // The SVC should never return to us because the scheduler
    // marks this task DEAD and switches to another task.
    loop {
        core::hint::spin_loop();
    }
}
fn yield_now(){
    unsafe{
        core::arch::asm!("svc #2",
        options(nomem, nostack));
    }
}

// ============================================================
// Test tasks
// ============================================================

fn task_a() {
    let mut value: u64 = 0xAAAAAAAAAAAAAAAA;

    loop {
        value = value.wrapping_mul(6364136223846793005);
        value = value.wrapping_add(1);

        if value & 0xFFFF == 0 {
            let mut uart = Uart::new(0x0900_0000);
            writeln!(uart, "A value = {:#x}", value).ok();
        }
    }
}

fn task_b() {
    let mut value: u64 = 0xBBBBBBBBBBBBBBBB;

    loop {
        value = value.wrapping_mul(1442695040888963407);
        value = value.wrapping_add(1);

        if value & 0xFFFF == 0 {
            let mut uart = Uart::new(0x0900_0000);
            writeln!(uart, "B value = {:#x}", value).ok();
        }
    }
}

// ============================================================
// Kernel tasks
// ============================================================

fn idle_task() {
    loop{
        unsafe{
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

   fn next(&self) -> usize {
        if self.tasks.count <= 1 {
            return 0;
        }

        let start = self.current;

        // Prefer another alive real task.
        for offset in 1..self.tasks.count {
            let candidate = (start + offset) % self.tasks.count;

            if candidate == 0 {
                continue;
            }

            if let Some(task) = &self.tasks.tasks[candidate] {
                if task.state != TaskState::Dead {
                    return candidate;
                }
            }
        }

        // No other real task exists.
        // Continue the current task if it is still alive.
        if self.current != 0 {
            if let Some(task) = &self.tasks.tasks[self.current] {
                if task.state != TaskState::Dead {
                    return self.current;
                }
            }
        }

        // No real tasks remain.
        0
    }

    fn load_current(&self, frame: &mut ExceptionFrame) {
        if self.tasks.count == 0 {
            return;
        }

        if let Some(task) = &self.tasks.tasks[self.current] {
            task.load_context(frame);
        }
    }

    fn tick(&mut self, frame: &mut ExceptionFrame) {
        if self.tasks.count == 0 {
            return;
        }

        if !self.started {
            self.started = true;

            self.current = if self.tasks.count > 1 { 1 } else { 0 };

            if let Some(task) = &mut self.tasks.tasks[self.current] {
                task.state = TaskState::Running;
                task.load_context(frame);
            }

            return;
        }

        if self.current != 0 {
            if let Some(task) = &mut self.tasks.tasks[self.current] {
                task.state = TaskState::Ready;
                task.save_context(frame);
            }
        }

        let next = self.next();

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

        // Kill current task.
        if let Some(task) = &mut self.tasks.tasks[self.current] {
            task.state = TaskState::Dead;
            task.save_context(frame);
        }

        // Find another real runnable task.
        for id in 1..self.tasks.count {
            if let Some(task) = &self.tasks.tasks[id] {
                if task.state != TaskState::Dead {
                    self.current = id;

                    if let Some(task) = &mut self.tasks.tasks[id] {
                        task.state = TaskState::Running;
                        task.load_context(frame);
                    }

                    return;
                }
            }
        }

        // No real tasks remain.
        self.current = 0;

        if let Some(idle) = &mut self.tasks.tasks[0] {
            idle.state = TaskState::Running;
            idle.load_context(frame);
        }
    }
    fn yield_current(&mut self, frame: &mut ExceptionFrame) {
        if self.tasks.count <= 1 {
            return;
        }

        // Save the currently running real task.
        if self.current != 0 {
            if let Some(task) = &mut self.tasks.tasks[self.current] {
                task.state = TaskState::Ready;
                task.save_context(frame);
            }
        }

        // Pick another READY task.
        let next = self.next();

        self.current = next;

        if let Some(task) = &mut self.tasks.tasks[next] {
            task.state = TaskState::Running;
            task.load_context(frame);
        }
    }
}

static mut SCHEDULER: Option<Scheduler> = None;

// ============================================================
// GIC distributor
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

        writeln!(uart, "Configuring CPU prority...").ok();
        self.write_cpu(0x004, 0xFF);

        writeln!(uart, "Enabling GIC CPU interface...").ok();
        self.write_cpu(0x000, 1);

        writeln!(uart, "GIC enabled! Returning to main...").ok();
    }

    fn sgi(&self, id: u8) {
        let value = (1u32 << 16) | (id as u32);
        self.write_dist(0xF00, value);
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
            core::arch::asm!(
                "mrs {0}, cntfrq_el0",
                out(reg) freq,
            );
        }

        freq
    }

    fn counter() -> u64 {
        let counter: u64;

        unsafe {
            core::arch::asm!(
                "mrs {0}, cntpct_el0",
                out(reg) counter,
            );
        }

        counter
    }

    fn set_timeout(ticks: u64) {
        unsafe {
            core::arch::asm!(
                "msr cntp_tval_el0, {0}",
                in(reg) ticks,
            );
        }
    }

    fn enable() {
        unsafe {
            core::arch::asm!(
                "msr cntp_ctl_el0, {0}",
                "isb",
                in(reg) 1u64,
            );
        }
    }
}

// ============================================================
// Exception decoding
// ============================================================

fn decode_esr(esr: u64) -> &'static str {
    let ec = (esr >> 26) & 0x3f;

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
        0x2c => "FP exception",
        0x3c => "BRK instruction",
        _ => "Other synchronous exception",
    }
}

fn decode_data_abort(esr: u64) {
    let mut uart = Uart::new(0x0900_0000);

    let iss = esr & 0x01ff_ffff;

    let isv = (iss >> 24) & 1;
    let wnr = (iss >> 6) & 1;
    let sas = (iss >> 22) & 0x3;
    let dfsc = iss & 0x3f;

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
        0x0a => "Access flag fault, level 2",
        0x0b => "Access flag fault, level 3",

        0x0D => "Permission fault, level 1",
        0x0E => "Permission fault, level 2",
        0x0F => "Permission fault, level 3",

        0x10 => "Synchronous external abort",

        0x11 => {
            "Synchronous external abort, translation table walk, level 1"
        }

        0x12 => {
            "Synchronous external abort, translation table walk, level 2"
        }

        0x13 => {
            "Synchronous external abort, translation table walk, level 3"
        }

        _ => "Other data abort",
    };

    writeln!(uart, "Data abort details:").ok();
    writeln!(uart, "ISV = {}", isv).ok();
    writeln!(
        uart,
        "Access = {}",
        if wnr == 1 { "write" } else { "read" }
    )
    .ok();
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
    let ec = (frame.esr >> 26) & 0x3f;
    let il = (frame.esr >> 25) & 1;
    let iss = frame.esr & 0x01ff_ffff;
    let sctlr: u64;
    let vbar: u64;
    let currentel: u64;
    // --------------------------------------------------------
    // SVC
    //
    // SVC #1 = task_exit()
    // --------------------------------------------------------

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
        2 => unsafe{
            let scheduler_ptr = &raw mut SCHEDULER;
            if let Some(scheduler) = (*scheduler_ptr).as_mut(){
                scheduler.yield_current(frame);
                return;
            }
            panic!("SVC #2 with no scheduler");
        },
        _=>{}
    }
}

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


    // --------------------------------------------------------
    // Test data abort recovery
    // --------------------------------------------------------

    if ec == 0x25 && frame.far == 0xDEAD_0000 {
        decode_data_abort(frame.esr);

        writeln!(uart, "Recovering from data abort...").ok();

        frame.elr += 4;

        return;
    }

    // --------------------------------------------------------
    // BRK
    // --------------------------------------------------------

    if ec == 0x3c {
        frame.elr += 4;
        return;
    }

    // --------------------------------------------------------
    // Unhandled exception
    // --------------------------------------------------------

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
extern "C" fn exception_irq_rust(
    frame: &mut ExceptionFrame,
    interrupt_id: u32,
) {
    if interrupt_id != 30 {
        return;
    }

    let tick = TICKS.fetch_add(1, Ordering::Relaxed) + 1;

    // 10 Hz scheduler tick.
    Timer::set_timeout(Timer::frequency() / 10);

    unsafe {
        let scheduler_ptr = &raw mut SCHEDULER;

        if let Some(scheduler) = (*scheduler_ptr).as_mut() {
            scheduler.tick(frame);

            let mut uart = Uart::new(0x0900_0000);
            if scheduler.current != 0{
            writeln!(
            uart,
            "TICK {} -> TASK {} PC={:#018x} SP={:#018x}",
            tick,
            scheduler.current,
            scheduler.tasks.tasks[scheduler.current]
                .as_ref()
                .map(|t| t.context.pc)
                .unwrap_or(0),
            scheduler.tasks.tasks[scheduler.current]
                .as_ref()
                .map(|t| t.context.sp)
                .unwrap_or(0),
        )
        .ok();
        }
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
// Kernel entry point
// ============================================================

#[unsafe(no_mangle)]
pub extern "C" fn rust_start() -> ! {
    let mut uart = Uart::new(0x0900_0000);

    writeln!(uart, "Starting...").unwrap();

    // --------------------------------------------------------
    // Enable FP/SIMD at EL1.
    //
    // LLVM may use Advanced SIMD instructions for ordinary
    // Rust operations such as copies.
    // --------------------------------------------------------

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

    // --------------------------------------------------------
    // Scheduler
    // --------------------------------------------------------

    writeln!(uart, "Initialising scheduler...").unwrap();

    unsafe {
        let scheduler = &raw mut SCHEDULER;

        (*scheduler) = Some(Scheduler::new());

        if let Some(scheduler) = (*scheduler).as_mut() {
            scheduler.add_task(idle_task);
            scheduler.add_task(task_a);
            scheduler.add_task(task_b);
        }
    }

    writeln!(uart, "Scheduler initialised!").unwrap();

    // --------------------------------------------------------
    // Exception vector
    // --------------------------------------------------------

    unsafe {
        writeln!(uart, "Installing exception vector....").unwrap();

        let vector = exception_vector as *const ();

        core::arch::asm!(
            "msr VBAR_EL1, {}",
            "isb",
            in(reg) vector,
        );
    }

    writeln!(uart, "Vector installed!").unwrap();

    // --------------------------------------------------------
    // GIC
    // --------------------------------------------------------

    writeln!(uart, "Enabling GIC...").unwrap();

    let gic = Gic::new();
    gic.enable();

    writeln!(uart, "GIC enabled! Returned to main!").unwrap();

    // --------------------------------------------------------
    // Timer
    // --------------------------------------------------------

    writeln!(uart, "Reading timer...").unwrap();

    let freq = Timer::frequency();
    let _counter = Timer::counter();

    writeln!(uart, "Arming timer...").unwrap();

    Timer::set_timeout(freq / 10);
    Timer::enable();

    writeln!(uart, "Timer armed!").unwrap();

    // --------------------------------------------------------
    // Enable IRQs.
    // --------------------------------------------------------

    writeln!(uart, "Enabling CPU IRQs...").unwrap();

    unsafe {
        core::arch::asm!("msr daifclr, #2");
    }

    writeln!(uart, "CPU IRQs enabled!").unwrap();

    // --------------------------------------------------------
    // Wait for first timer interrupt.
    //
    // The first timer tick loads task 0's initial context.
    // --------------------------------------------------------

    writeln!(uart, "Calling WFE...").unwrap();

    loop {
        unsafe {
            core::arch::asm!("wfe");
        }
    }
}

// ============================================================
// Panic handler
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