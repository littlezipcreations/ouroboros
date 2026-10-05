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
// 0x120  kernel_sp
//
// Total = 0x128 bytes
//
// During exception entry we reserve 0x130 bytes so that the
// saved x30 sits at frame + 0x130 and the original SP sits
// at frame + 0x140.
// ============================================================


// ============================================================
// Common context restore
//
// On entry:
//
//   SP = current exception frame
//
// frame->sp        = destination SP_EL0 for user tasks
//                    destination SP_EL1 for kernel tasks
//
// frame->kernel_sp = destination SP_EL1
//
// The current exception frame is always consumed using the
// CURRENT SP_EL1 before switching to the destination task's
// kernel stack.
// ============================================================

exception_restore_context:

    // --------------------------------------------------------
    // Save destination kernel SP in x20.
    //
    // We cannot use another normal register because every
    // general-purpose register must eventually be restored.
    // --------------------------------------------------------

    ldr     x20, [sp, #0x120]

    // --------------------------------------------------------
    // Save the destination task's real x20 in TPIDR_EL1.
    // --------------------------------------------------------

    ldr     x1, [sp, #0x0A0]
    msr     tpidr_el1, x1

    // --------------------------------------------------------
    // Restore ELR and SPSR.
    // --------------------------------------------------------

    ldr     x1, [sp, #0x100]
    msr     elr_el1, x1

    ldr     x1, [sp, #0x108]
    msr     spsr_el1, x1

    // --------------------------------------------------------
    // Restore SP_EL0.
    //
    // For an EL0 task this is its user stack.
    // For an EL1 task this value is irrelevant because EL1H
    // execution uses SP_EL1.
    // --------------------------------------------------------

    ldr     x1, [sp, #0x0F8]
    msr     sp_el0, x1

    // --------------------------------------------------------
    // Restore x0-x19.
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

    // --------------------------------------------------------
    // x20 is currently holding destination kernel SP, so skip
    // the saved x20 for the moment.
    // --------------------------------------------------------

    ldp     x21, x22, [sp, #0x0A8]
    ldp     x23, x24, [sp, #0x0B8]
    ldp     x25, x26, [sp, #0x0C8]
    ldp     x27, x28, [sp, #0x0D8]

    ldr     x29, [sp, #0x0E8]
    ldr     x30, [sp, #0x0F0]

    // --------------------------------------------------------
    // The current exception frame lives on the CURRENT task's
    // kernel stack.
    //
    // Consume the whole frame and the 16-byte area containing
    // the saved x30.
    // --------------------------------------------------------

    add     sp, sp, #0x130
    add     sp, sp, #0x010

    // --------------------------------------------------------
    // Now the old exception frame is completely finished.
    //
    // Switch SP_EL1 to the destination task's kernel stack.
    // --------------------------------------------------------

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

    // Save x30 in the 16-byte slot immediately below the
    // original SP_EL1.
    str     x30, [sp, #-16]!

    // Reserve:
    //
    //   0x128 bytes for ExceptionFrame
    //   0x008 bytes alignment/padding
    //
    // plus the already-saved x30 slot.
    //
    // From the resulting frame:
    //
    //   frame + 0x130 = saved x30
    //   frame + 0x140 = original SP_EL1
    //

    sub     sp, sp, #0x130

    stp     x0,  x1,  [sp, #0x000]
    stp     x2,  x3,  [sp, #0x010]
    stp     x4,  x5,  [sp, #0x020]
    stp     x6,  x7,  [sp, #0x030]
    stp     x8,  x9,  [sp, #0x040]
    stp     x10, x11, [sp, #0x050]
    stp     x12, x13, [sp, #0x060]
    stp     x14, x15, [sp, #0x070]
    stp     x16, x17, [sp, #0x080]
    stp     x18, x19, [sp, #0x090]
    stp     x20, x21, [sp, #0x0A0]
    stp     x22, x23, [sp, #0x0B0]
    stp     x24, x25, [sp, #0x0C0]
    stp     x26, x27, [sp, #0x0D0]
    stp     x28, x29, [sp, #0x0E0]

    // Restore saved x30 into the frame.
    ldr     x1, [sp, #0x130]
    str     x1, [sp, #0x0F0]

    // Save SPSR first so we can determine whether the
    // exception came from EL0 or EL1.

    mrs     x1, spsr_el1
    str     x1, [sp, #0x108]

    // --------------------------------------------------------
    // Determine origin EL.
    //
    // SPSR.M:
    //   0000 = EL0t
    //   0101 = EL1h
    // --------------------------------------------------------

    and     x2, x1, #0xf
    cbnz    x2, 1f

    // --------------------------------------------------------
    // Exception came from EL0.
    //
    // SP_EL0 contains the user stack.
    // --------------------------------------------------------

    mrs     x1, sp_el0
    str     x1, [sp, #0x0F8]

    b       2f

1:
    // --------------------------------------------------------
    // Exception came from EL1.
    //
    // SP_EL1 before exception = frame + 0x140.
    // --------------------------------------------------------

    add     x1, sp, #0x140
    str     x1, [sp, #0x0F8]

2:
    // --------------------------------------------------------
    // kernel_sp always means the original SP_EL1.
    // --------------------------------------------------------

    add     x1, sp, #0x140
    str     x1, [sp, #0x120]

    mrs     x1, elr_el1
    str     x1, [sp, #0x100]

    mrs     x1, esr_el1
    str     x1, [sp, #0x110]

    mrs     x1, far_el1
    str     x1, [sp, #0x118]

    mov     x0, sp
    bl      exception_sync_rust

    b       exception_restore_context


// ============================================================
// IRQ exception
// ============================================================

exception_irq_entry:

    str     x30, [sp, #-16]!
    sub     sp, sp, #0x130

    stp     x0,  x1,  [sp, #0x000]
    stp     x2,  x3,  [sp, #0x010]
    stp     x4,  x5,  [sp, #0x020]
    stp     x6,  x7,  [sp, #0x030]
    stp     x8,  x9,  [sp, #0x040]
    stp     x10, x11, [sp, #0x050]
    stp     x12, x13, [sp, #0x060]
    stp     x14, x15, [sp, #0x070]
    stp     x16, x17, [sp, #0x080]
    stp     x18, x19, [sp, #0x090]
    stp     x20, x21, [sp, #0x0A0]
    stp     x22, x23, [sp, #0x0B0]
    stp     x24, x25, [sp, #0x0C0]
    stp     x26, x27, [sp, #0x0D0]
    stp     x28, x29, [sp, #0x0E0]

    ldr     x1, [sp, #0x130]
    str     x1, [sp, #0x0F0]

    mrs     x1, spsr_el1
    str     x1, [sp, #0x108]

    and     x2, x1, #0xf
    cbnz    x2, 3f

    // EL0 origin.
    mrs     x1, sp_el0
    str     x1, [sp, #0x0F8]
    b       4f

3:
    // EL1 origin.
    add     x1, sp, #0x140
    str     x1, [sp, #0x0F8]

4:
    // Original SP_EL1.
    add     x1, sp, #0x140
    str     x1, [sp, #0x120]

    mrs     x1, elr_el1
    str     x1, [sp, #0x100]

    mrs     x1, esr_el1
    str     x1, [sp, #0x110]

    mrs     x1, far_el1
    str     x1, [sp, #0x118]

    // --------------------------------------------------------
    // GIC interrupt ID
    // --------------------------------------------------------

    mov     x2, #0x8010
    lsl     x2, x2, #12

    ldr     w19, [x2, #0x0C]
    mov     w1, w19

    mov     x0, sp
    bl      exception_irq_rust

    mov     x2, #0x8010
    lsl     x2, x2, #12

    str     w19, [x2, #0x10]

    b       exception_restore_context


// ============================================================
// FIQ exception
// ============================================================

exception_fiq_entry:

    str     x30, [sp, #-16]!
    sub     sp, sp, #0x130

    stp     x0,  x1,  [sp, #0x000]
    stp     x2,  x3,  [sp, #0x010]
    stp     x4,  x5,  [sp, #0x020]
    stp     x6,  x7,  [sp, #0x030]
    stp     x8,  x9,  [sp, #0x040]
    stp     x10, x11, [sp, #0x050]
    stp     x12, x13, [sp, #0x060]
    stp     x14, x15, [sp, #0x070]
    stp     x16, x17, [sp, #0x080]
    stp     x18, x19, [sp, #0x090]
    stp     x20, x21, [sp, #0x0A0]
    stp     x22, x23, [sp, #0x0B0]
    stp     x24, x25, [sp, #0x0C0]
    stp     x26, x27, [sp, #0x0D0]
    stp     x28, x29, [sp, #0x0E0]

    ldr     x1, [sp, #0x130]
    str     x1, [sp, #0x0F0]

    mrs     x1, spsr_el1
    str     x1, [sp, #0x108]

    and     x2, x1, #0xf
    cbnz    x2, 5f

    // EL0 origin.
    mrs     x1, sp_el0
    str     x1, [sp, #0x0F8]
    b       6f

5:
    // EL1 origin.
    add     x1, sp, #0x140
    str     x1, [sp, #0x0F8]

6:
    // Original SP_EL1.
    add     x1, sp, #0x140
    str     x1, [sp, #0x120]

    mrs     x1, elr_el1
    str     x1, [sp, #0x100]

    mrs     x1, esr_el1
    str     x1, [sp, #0x110]

    mrs     x1, far_el1
    str     x1, [sp, #0x118]

    mov     x0, sp
    bl      exception_fiq_rust

    b       exception_restore_context


// ============================================================
// SError
// ============================================================

exception_serror_entry:
    bl      exception_serror_rust
    b       exception_serror_entry