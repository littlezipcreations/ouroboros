.section .text.exceptions
.align 11
.global exception_vector
.global exception_sync_entry
.global exception_irq_entry
.global exception_fiq_entry
.global exception_serror_entry
.extern exception_sync_rust
.extern exception_irq_rust
.extern exception_fiq_rust
.extern exception_serror_rust

// ============================================================
// Exception vector
// ============================================================
exception_vector:
.align 7
b exception_sync_entry
.align 7
b exception_irq_entry
.align 7
b exception_fiq_entry
.align 7
b exception_serror_entry
.align 7
b exception_sync_entry
.align 7
b exception_irq_entry
.align 7
b exception_fiq_entry
.align 7
b exception_serror_entry
.align 7
b exception_sync_entry
.align 7
b exception_irq_entry
.align 7
b exception_fiq_entry
.align 7
b exception_serror_entry
.align 7
b exception_sync_entry
.align 7
b exception_irq_entry
.align 7
b exception_fiq_entry
.align 7
b exception_serror_entry

// ============================================================
// ExceptionFrame
//
// 0x000  x0
// 0x008  x1
// ...
// 0x0F0  x30
// 0x0F8  sp
// 0x100  elr
// 0x108  spsr
// 0x110  esr
// 0x118  far
//
// Total = 0x120 = 288 bytes
// ============================================================

// ============================================================
// Common context restore
//
// Entry:
//   SP = current exception frame
//
// This routine expects the frame to contain the context that
// should be resumed.
//
// Important:
//   x20 is temporarily used to hold the new task's SP.
//   TPIDR_EL1 temporarily holds the new task's x20.
//
// This is safe because TPIDR_EL1 is not part of the task's
// architectural execution context here.
// ============================================================
exception_restore_context:
    // --------------------------------------------------------
    // Save the destination task's SP in TPIDR_EL1.
    //
    // We cannot use an ordinary register because all x0-x30
    // must eventually be restored.
    // --------------------------------------------------------

    ldr     x20, [sp, #0x0F8]
    ldr     x1,  [sp, #0x0A0]
    msr     tpidr_el1, x1

    // --------------------------------------------------------
    // Restore ELR/SPSR.
    // --------------------------------------------------------

    ldr     x1, [sp, #0x100]
    msr     elr_el1, x1

    ldr     x1, [sp, #0x108]
    msr     spsr_el1, x1

    // --------------------------------------------------------
    // Restore registers.
    //
    // x20 is deliberately skipped because it is currently
    // holding the destination SP.
    // --------------------------------------------------------

    ldp     x0,  x1,  [sp, #0x000]
    ldp     x2,  x3,  [sp, #0x010]
    ldp     x4,  x5,  [sp, #0x020]
    ldp     x6,  x7,  [sp, #0x030]
    ldp     x8,  x9,  [sp, #0x040]
    ldp     x10, x11, [sp, #0x050]
    ldp     x12, x13, [sp, #0x060]
    ldp     x14, x15, [sp, #0x070]
    ldp     x16, x17, [sp, #0x080]
    ldp     x18, x19, [sp, #0x090]

    ldp     x21, x22, [sp, #0x0A8]
    ldp     x23, x24, [sp, #0x0B8]
    ldp     x25, x26, [sp, #0x0C8]
    ldp     x27, x28, [sp, #0x0D8]

    ldr     x29, [sp, #0x0E8]
    ldr     x30, [sp, #0x0F0]

    // --------------------------------------------------------
    // x20 currently contains the destination task SP.
    //
    // Remove the exception frame and saved x30 from the
    // CURRENT stack, then switch to the TASK stack.
    // --------------------------------------------------------

    add     sp, sp, #0x120
    add     sp, sp, #0x010

    mov     sp, x20

    // --------------------------------------------------------
    // Finally restore the real x20.
    // --------------------------------------------------------

    mrs     x20, tpidr_el1

    eret

// ============================================================
// Synchronous exception
// ============================================================
exception_sync_entry:
str x30, [sp, #-16]!
sub sp, sp, #288
stp x0, x1, [sp, #0x000]
stp x2, x3, [sp, #0x010]
stp x4, x5, [sp, #0x020]
stp x6, x7, [sp, #0x030]
stp x8, x9, [sp, #0x040]
stp x10, x11, [sp, #0x050]
stp x12, x13, [sp, #0x060]
stp x14, x15, [sp, #0x070]
stp x16, x17, [sp, #0x080]
stp x18, x19, [sp, #0x090]
stp x20, x21, [sp, #0x0A0]
stp x22, x23, [sp, #0x0B0]
stp x24, x25, [sp, #0x0C0]
stp x26, x27, [sp, #0x0D0]
stp x28, x29, [sp, #0x0E0]
ldr x1, [sp, #0x120]
str x1, [sp, #0x0F0]
add x1, sp, #0x130
str x1, [sp, #0x0F8]
mrs x1, elr_el1
str x1, [sp, #0x100]
mrs x1, spsr_el1
str x1, [sp, #0x108]
mrs x1, esr_el1
str x1, [sp, #0x110]
mrs x1, far_el1
str x1, [sp, #0x118]
mov x0, sp
bl exception_sync_rust
b exception_restore_context

// ============================================================
// IRQ exception
// ============================================================
exception_irq_entry:
str x30, [sp, #-16]!
sub sp, sp, #288
stp x0, x1, [sp, #0x000]
stp x2, x3, [sp, #0x010]
stp x4, x5, [sp, #0x020]
stp x6, x7, [sp, #0x030]
stp x8, x9, [sp, #0x040]
stp x10, x11, [sp, #0x050]
stp x12, x13, [sp, #0x060]
stp x14, x15, [sp, #0x070]
stp x16, x17, [sp, #0x080]
stp x18, x19, [sp, #0x090]
stp x20, x21, [sp, #0x0A0]
stp x22, x23, [sp, #0x0B0]
stp x24, x25, [sp, #0x0C0]
stp x26, x27, [sp, #0x0D0]
stp x28, x29, [sp, #0x0E0]
ldr x1, [sp, #0x120]
str x1, [sp, #0x0F0]
add x1, sp, #0x130
str x1, [sp, #0x0F8]
mrs x1, elr_el1
str x1, [sp, #0x100]
mrs x1, spsr_el1
str x1, [sp, #0x108]
mrs x1, esr_el1
str x1, [sp, #0x110]
mrs x1, far_el1
str x1, [sp, #0x118]
mov x2, #0x8010
lsl x2, x2, #12
ldr w19, [x2, #0x0C]
mov w1, w19
mov x0, sp
bl exception_irq_rust
mov x2, #0x8010
lsl x2, x2, #12
str w19, [x2, #0x10]
b exception_restore_context

// ============================================================
// FIQ exception
// ============================================================
exception_fiq_entry:
str x30, [sp, #-16]!
sub sp, sp, #288
stp x0, x1, [sp, #0x000]
stp x2, x3, [sp, #0x010]
stp x4, x5, [sp, #0x020]
stp x6, x7, [sp, #0x030]
stp x8, x9, [sp, #0x040]
stp x10, x11, [sp, #0x050]
stp x12, x13, [sp, #0x060]
stp x14, x15, [sp, #0x070]
stp x16, x17, [sp, #0x080]
stp x18, x19, [sp, #0x090]
stp x20, x21, [sp, #0x0A0]
stp x22, x23, [sp, #0x0B0]
stp x24, x25, [sp, #0x0C0]
stp x26, x27, [sp, #0x0D0]
stp x28, x29, [sp, #0x0E0]
ldr x1, [sp, #0x120]
str x1, [sp, #0x0F0]
add x1, sp, #0x130
str x1, [sp, #0x0F8]
mrs x1, elr_el1
str x1, [sp, #0x100]
mrs x1, spsr_el1
str x1, [sp, #0x108]
mrs x1, esr_el1
str x1, [sp, #0x110]
mrs x1, far_el1
str x1, [sp, #0x118]
mov x0, sp
bl exception_fiq_rust
b exception_restore_context

// ============================================================
// SError
// ============================================================
exception_serror_entry:
bl exception_serror_rust
b exception_serror_entry