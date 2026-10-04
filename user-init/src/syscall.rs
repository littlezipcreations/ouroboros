#[inline(always)]
pub fn test(){
    unsafe{
        core::arch::asm!("svc #42");
    }
}
#[inline(always)]
pub fn yield_now(){
    unsafe{
        core::arch::asm!("svc #2");
    }
}
#[inline(always)]
pub fn exit() -> !{
    unsafe{
        core::arch::asm!("svc #1", options(noreturn));
    }
}