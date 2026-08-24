#![no_std]
#![no_main]
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
// GIC distributor
// ============================================================

struct Gic{
    dist: usize,
    cpu: usize,
}
impl Gic{
    const fn new() -> Self{
        Self{dist: 0x0800_0000, cpu: 0x0801_0000}
    }
    fn write_dist(&self, offset:usize, value: u32){
        unsafe{
            ((self.dist + offset) as *mut u32).write_volatile(value);
        }
    }
    fn write_cpu(&self, offset:usize, value: u32){
        unsafe{
            ((self.cpu + offset) as *mut u32).write_volatile(value);
        }
    }
    fn enable(&self){
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
    fn sgi(&self, id: u8){
        let value = (1u32 << 16) | (id as u32);
        self.write_dist(0xF00, value);
    }
}

// ===========================================================
// Timer
// ===========================================================

struct Timer;
impl Timer{
    fn frequency() -> u64{
        let freq: u64;
        unsafe{
            core::arch::asm!(
                "mrs {0}, cntfrq_el0",
                out(reg) freq,
            );
        }
        freq
    }
    fn counter() -> u64{
        let counter: u64;
        unsafe{
            core::arch::asm!(
                "mrs {0}, cntpct_el0",
                out(reg) counter,
            );
        }
        counter
    }
    fn set_timeout(ticks: u64){
        unsafe{
            core::arch::asm!(
                "msr cntp_tval_el0, {0}",
                in(reg) ticks,
            );
        }
    }
    fn enable() {
        unsafe{
            core::arch::asm!(
                "msr cntp_ctl_el0, {0}",
                "isb",
                in(reg) 1u64,
            );
        }
    }
}

// ============================================================
// Exception handlers
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
#[unsafe(no_mangle)]
extern "C" fn exception_sync_rust(frame: &mut ExceptionFrame){
    let mut uart = Uart::new(0x0900_0000);
    let sctlr : u64;
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

    let ec = (frame.esr >> 26) & 0x3f;
    let il = (frame.esr >> 25) & 1;
    let iss = frame.esr & 0x01ff_ffff;

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
    } else if ec == 0x3c {
        frame.elr += 4;
    } else {
        writeln!(uart, "KERNEL PANIC! Unhandled exception!").ok();
        loop {
            unsafe {
                core::arch::asm!("wfe");
            }
        }
    }
}
#[unsafe(no_mangle)]
extern "C" fn exception_irq_rust(frame: &mut ExceptionFrame, interrupt_id: u32){
    let mut uart = Uart::new(0x0900_0000);

    //writeln!(uart, "=== IRQ ===").ok();
    //writeln!(uart, "INTID = {}", interrupt_id).ok();
    if interrupt_id == 30{
        writeln!(uart, "(timer)").ok();
    }
    //writeln!(uart, "ELR = {:#018x}", frame.elr).ok();
    //writeln!(uart, "SP = {:#018x}", frame.sp).ok();
    let tick = TICKS.fetch_add(1, Ordering::Relaxed) + 1;
    writeln!(uart, "TICK {}.", tick).ok();
}
#[unsafe(no_mangle)]
extern "C" fn exception_fiq_rust(frame: &mut ExceptionFrame) {
    let mut uart = Uart::new(0x0900_0000);

    writeln!(uart, "=== FIQ ===").ok();
    writeln!(uart, "ELR = {:#018x}", frame.elr).ok();
    writeln!(uart, "SP  = {:#018x}", frame.sp).ok();
}
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
    writeln!(uart, "Enabling GIC...").unwrap();
    let gic = Gic::new();
    gic.enable();
    writeln!(uart, "GIC enabled! Returned to main!").unwrap();
    writeln!(uart, "Reading timer...").unwrap();
    let freq = Timer::frequency();
    let counter = Timer::counter();
    writeln!(uart, "Timer frequency: {:#018x} Hz", freq).unwrap();
    writeln!(uart, "Timer counter: {:#018x}", counter).unwrap();
    writeln!(uart, "Arming timer...").unwrap();
    Timer::set_timeout( freq / 10);
    Timer::enable();
    writeln!(uart, "Timer armed!").unwrap();
    writeln!(uart, "Enabling CPU IRQs...").unwrap();
    unsafe { core::arch::asm!("msr daifclr, #2");}
    writeln!(uart, "CPU IRQs enabled").unwrap();
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