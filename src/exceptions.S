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

exception_vector:

    // --------------------------------------------------------
    // Current EL with SP0
    // --------------------------------------------------------

    .align 7
    b exception_sync_entry
    .align 7
    b exception_irq_entry
    .align 7
    b exception_fiq_entry
    .align 7
    b exception_serror_entry

    // --------------------------------------------------------
    // Current EL with SPx
    // --------------------------------------------------------

    .align 7
    b exception_sync_entry
    .align 7
    b exception_irq_entry
    .align 7
    b exception_fiq_entry
    .align 7
    b exception_serror_entry

    // --------------------------------------------------------
    // Lower EL using AArch64
    // --------------------------------------------------------

    .align 7
    b exception_sync_entry
    .align 7
    b exception_irq_entry
    .align 7
    b exception_fiq_entry
    .align 7
    b exception_serror_entry

    // --------------------------------------------------------
    // Lower EL using AArch32
    // --------------------------------------------------------

    .align 7
    b exception_sync_entry
    .align 7
    b exception_irq_entry
    .align 7
    b exception_fiq_entry
    .align 7
    b exception_serror_entry


// ============================================================
// Synchronous exception
//
// ExceptionFrame:
//
//   +0    x0
//   +8    x1
//   ...
//   +240  x30
//   +248  original SP
//   +256  ELR_EL1
//   +264  SPSR_EL1
//   +272  ESR_EL1
//   +280  FAR_EL1
//
// Total: 288 bytes
// ============================================================

exception_sync_entry:

    // --------------------------------------------------------
    // Preserve original x30.
    //
    // Current SP -> original SP
    // After this:
    //
    //   [original SP - 16] = x30
    // --------------------------------------------------------

    str x30, [sp, #-16]!

    // Allocate ExceptionFrame.
    sub sp, sp, #288

    // --------------------------------------------------------
    // Save x0-x29
    // --------------------------------------------------------

    stp x0,  x1,  [sp, #0]
    stp x2,  x3,  [sp, #16]
    stp x4,  x5,  [sp, #32]
    stp x6,  x7,  [sp, #48]
    stp x8,  x9,  [sp, #64]
    stp x10, x11, [sp, #80]
    stp x12, x13, [sp, #96]
    stp x14, x15, [sp, #112]
    stp x16, x17, [sp, #128]
    stp x18, x19, [sp, #144]
    stp x20, x21, [sp, #160]
    stp x22, x23, [sp, #176]
    stp x24, x25, [sp, #192]
    stp x26, x27, [sp, #208]
    stp x28, x29, [sp, #224]

    // --------------------------------------------------------
    // Recover original x30.
    // Current SP = original SP - 304
    // Saved x30 = current SP + 288
    // --------------------------------------------------------

    ldr x1, [sp, #288]
    str x1, [sp, #240]

    // Original SP = current SP + 304.
    add x1, sp, #304
    str x1, [sp, #248]

    // ELR_EL1
    mrs x1, elr_el1
    str x1, [sp, #256]

    // SPSR_EL1
    mrs x1, spsr_el1
    str x1, [sp, #264]

    // ESR_EL1
    mrs x1, esr_el1
    str x1, [sp, #272]

    // FAR_EL1
    mrs x1, far_el1
    str x1, [sp, #280]

    // --------------------------------------------------------
    // exception_sync_rust(&mut ExceptionFrame)
    // --------------------------------------------------------

    mov x0, sp
    bl exception_sync_rust

    // --------------------------------------------------------
    // Restore ELR/SPSR
    // --------------------------------------------------------

    ldr x1, [sp, #256]
    msr elr_el1, x1

    ldr x1, [sp, #264]
    msr spsr_el1, x1

    // --------------------------------------------------------
    // Restore x0-x29
    // --------------------------------------------------------

    ldp x0,  x1,  [sp, #0]
    ldp x2,  x3,  [sp, #16]
    ldp x4,  x5,  [sp, #32]
    ldp x6,  x7,  [sp, #48]
    ldp x8,  x9,  [sp, #64]
    ldp x10, x11, [sp, #80]
    ldp x12, x13, [sp, #96]
    ldp x14, x15, [sp, #112]
    ldp x16, x17, [sp, #128]
    ldp x18, x19, [sp, #144]
    ldp x20, x21, [sp, #160]
    ldp x22, x23, [sp, #176]
    ldp x24, x25, [sp, #192]
    ldp x26, x27, [sp, #208]
    ldp x28, x29, [sp, #224]

    // Restore x30.
    ldr x30, [sp, #240]

    // Discard ExceptionFrame and temporary x30 slot.
    add sp, sp, #288
    add sp, sp, #16

    eret


// ============================================================
// IRQ exception
// ============================================================

exception_irq_entry:

    // --------------------------------------------------------
    // Preserve original x30.
    // --------------------------------------------------------

    str x30, [sp, #-16]!

    // Allocate ExceptionFrame.
    sub sp, sp, #288

    // --------------------------------------------------------
    // Save x0-x29
    // --------------------------------------------------------

    stp x0,  x1,  [sp, #0]
    stp x2,  x3,  [sp, #16]
    stp x4,  x5,  [sp, #32]
    stp x6,  x7,  [sp, #48]
    stp x8,  x9,  [sp, #64]
    stp x10, x11, [sp, #80]
    stp x12, x13, [sp, #96]
    stp x14, x15, [sp, #112]
    stp x16, x17, [sp, #128]
    stp x18, x19, [sp, #144]
    stp x20, x21, [sp, #160]
    stp x22, x23, [sp, #176]
    stp x24, x25, [sp, #192]
    stp x26, x27, [sp, #208]
    stp x28, x29, [sp, #224]

    // --------------------------------------------------------
    // Recover original x30.
    // --------------------------------------------------------

    ldr x1, [sp, #288]
    str x1, [sp, #240]

    // Original SP.
    add x1, sp, #304
    str x1, [sp, #248]

    // ELR_EL1.
    mrs x1, elr_el1
    str x1, [sp, #256]

    // SPSR_EL1.
    mrs x1, spsr_el1
    str x1, [sp, #264]

    // ESR_EL1.
    mrs x1, esr_el1
    str x1, [sp, #272]

    // FAR_EL1.
    mrs x1, far_el1
    str x1, [sp, #280]

    // --------------------------------------------------------
    // Read GIC interrupt acknowledge register.
    //
    // GICC base = 0x08010000
    // GICC_IAR  = +0x0C
    // --------------------------------------------------------

    mov x2, #0x8010
    lsl x2, x2, #12

    ldr w19, [x2, #0x0C]

    // --------------------------------------------------------
    // exception_irq_rust(
    //     &mut ExceptionFrame,
    //     interrupt_id
    // )
    //
    // x0 = frame
    // w1 = interrupt ID
    // --------------------------------------------------------

    mov x0, sp
    mov w1, w19

    bl exception_irq_rust

    // --------------------------------------------------------
    // Rebuild GICC base.
    // --------------------------------------------------------

    mov x2, #0x8010
    lsl x2, x2, #12

    // --------------------------------------------------------
    // Complete interrupt.
    //
    // GICC_EOIR = +0x10
    // --------------------------------------------------------

    str w19, [x2, #0x10]

        // --------------------------------------------------------
    // Rust may have replaced the frame with the NEXT task's
    // context.
    //
    // We need the new SP while also restoring the new task's
    // x20. Use TPIDR_EL1 as temporary storage so we don't
    // touch the task's stack.
    // --------------------------------------------------------

    // Save the new task's x20 in TPIDR_EL1.
    ldr x1, [sp, #160]
    msr tpidr_el1, x1

    // Save the new task's SP in x20.
    ldr x20, [sp, #248]

    // --------------------------------------------------------
    // Restore ELR/SPSR.
    // --------------------------------------------------------

    ldr x1, [sp, #256]
    msr elr_el1, x1

    ldr x1, [sp, #264]
    msr spsr_el1, x1

    // --------------------------------------------------------
    // Restore all general-purpose registers except x20.
    // x20 currently contains the new task's SP.
    // --------------------------------------------------------

    ldp x0,  x1,  [sp, #0]
    ldp x2,  x3,  [sp, #16]
    ldp x4,  x5,  [sp, #32]
    ldp x6,  x7,  [sp, #48]
    ldp x8,  x9,  [sp, #64]
    ldp x10, x11, [sp, #80]
    ldp x12, x13, [sp, #96]
    ldp x14, x15, [sp, #112]
    ldp x16, x17, [sp, #128]
    ldp x18, x19, [sp, #144]

    // x20 intentionally skipped.

    ldp x21, x22, [sp, #168]
    ldp x23, x24, [sp, #184]
    ldp x25, x26, [sp, #200]
    ldp x27, x28, [sp, #216]

    ldr x29, [sp, #232]
    ldr x30, [sp, #240]

    // --------------------------------------------------------
    // Destroy the OLD exception frame.
    // --------------------------------------------------------

    add sp, sp, #288
    add sp, sp, #16

    // --------------------------------------------------------
    // Switch to NEW task's stack.
    // --------------------------------------------------------

    mov sp, x20

    // --------------------------------------------------------
    // Recover the NEW task's x20.
    // --------------------------------------------------------

    mrs x20, tpidr_el1

    // --------------------------------------------------------
    // Return to NEW task.
    // --------------------------------------------------------

    eret

// ============================================================
// FIQ exception
// ============================================================

exception_fiq_entry:

    // Preserve original x30.
    str x30, [sp, #-16]!

    // Allocate ExceptionFrame.
    sub sp, sp, #288

    // Save x0-x29.
    stp x0,  x1,  [sp, #0]
    stp x2,  x3,  [sp, #16]
    stp x4,  x5,  [sp, #32]
    stp x6,  x7,  [sp, #48]
    stp x8,  x9,  [sp, #64]
    stp x10, x11, [sp, #80]
    stp x12, x13, [sp, #96]
    stp x14, x15, [sp, #112]
    stp x16, x17, [sp, #128]
    stp x18, x19, [sp, #144]
    stp x20, x21, [sp, #160]
    stp x22, x23, [sp, #176]
    stp x24, x25, [sp, #192]
    stp x26, x27, [sp, #208]
    stp x28, x29, [sp, #224]

    // Recover original x30.
    ldr x1, [sp, #288]
    str x1, [sp, #240]

    // Original SP.
    add x1, sp, #304
    str x1, [sp, #248]

    // ELR.
    mrs x1, elr_el1
    str x1, [sp, #256]

    // SPSR.
    mrs x1, spsr_el1
    str x1, [sp, #264]

    // ESR.
    mrs x1, esr_el1
    str x1, [sp, #272]

    // FAR.
    mrs x1, far_el1
    str x1, [sp, #280]

    // Call Rust.
    mov x0, sp
    bl exception_fiq_rust

    // Restore ELR/SPSR.
    ldr x1, [sp, #256]
    msr elr_el1, x1

    ldr x1, [sp, #264]
    msr spsr_el1, x1

    // Restore registers.
    ldp x0,  x1,  [sp, #0]
    ldp x2,  x3,  [sp, #16]
    ldp x4,  x5,  [sp, #32]
    ldp x6,  x7,  [sp, #48]
    ldp x8,  x9,  [sp, #64]
    ldp x10, x11, [sp, #80]
    ldp x12, x13, [sp, #96]
    ldp x14, x15, [sp, #112]
    ldp x16, x17, [sp, #128]
    ldp x18, x19, [sp, #144]
    ldp x20, x21, [sp, #160]
    ldp x22, x23, [sp, #176]
    ldp x24, x25, [sp, #192]
    ldp x26, x27, [sp, #208]
    ldp x28, x29, [sp, #224]

    // Restore x30.
    ldr x30, [sp, #240]

    // Destroy frame and temporary x30 storage.
    add sp, sp, #288
    add sp, sp, #16

    eret


// ============================================================
// SError exception
// ============================================================

exception_serror_entry:

    bl exception_serror_rust