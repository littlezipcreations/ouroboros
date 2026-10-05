use core::arch::asm;
#[inline(always)]
pub fn test(){
    unsafe{
        asm!("svc #42");
    }
}
#[inline(always)]
pub fn yield_now(){
    unsafe{
        asm!("svc #2");
    }
}
#[inline(always)]
pub fn exit() -> !{
    unsafe{
        asm!("svc #1", options(noreturn));
    }
}
pub fn sys_write(buffer: &[u8]) -> usize {
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