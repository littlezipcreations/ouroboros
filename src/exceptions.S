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

    // Current EL with SP0
    .align 7
    b exception_sync_entry
    .align 7
    b exception_irq_entry
    .align 7
    b exception_fiq_entry
    .align 7
    b exception_serror_entry

    // Current EL with SPx
    .align 7
    b exception_sync_entry
    .align 7
    b exception_irq_entry
    .align 7
    b exception_fiq_entry
    .align 7
    b exception_serror_entry

    // Lower EL using AArch64
    .align 7
    b exception_sync_entry
    .align 7
    b exception_irq_entry
    .align 7
    b exception_fiq_entry
    .align 7
    b exception_serror_entry

    // Lower EL using AArch32
    .align 7
    b exception_sync_entry
    .align 7
    b exception_irq_entry
    .align 7
    b exception_fiq_entry
    .align 7
    b exception_serror_entry


// ------------------------------------------------------------
// Synchronous exception
//
// Frame:
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
// ------------------------------------------------------------

exception_sync_entry:

    // Preserve the original x30 before touching it.
    //
    // We temporarily put it immediately below the original SP.
    str x30, [sp, #-16]!

    // Allocate the actual ExceptionFrame.
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

    // Recover the original x30.
    //
    // Current SP = original SP - 304
    // Saved x30  = original SP - 16
    // Therefore saved x30 = current SP + 288.
    ldr x1, [sp, #288]
    str x1, [sp, #240]

    // Original SP = current SP + 304.
    add x1, sp, #304
    str x1, [sp, #248]

    // ELR
    mrs x1, elr_el1
    str x1, [sp, #256]

    // SPSR
    mrs x1, spsr_el1
    str x1, [sp, #264]

    // ESR
    mrs x1, esr_el1
    str x1, [sp, #272]

    // FAR
    mrs x1, far_el1
    str x1, [sp, #280]

    // Rust receives &ExceptionFrame in x0.
        mov x0, sp
    bl exception_sync_rust

    // Restore ELR/SPSR.
    ldr x1, [sp, #256]
    msr elr_el1, x1

    ldr x1, [sp, #264]
    msr spsr_el1, x1

    // Restore x0-x29.
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

    // Restore original SP.
    // Restore original X30.
    ldr x30, [sp, #240]

    // Throw away the frame and temporary X30 slot.
    add sp, sp, #288
    add sp, sp, #16

    eret


// ------------------------------------------------------------
// Other exception types
// ------------------------------------------------------------

exception_irq_entry:

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


    // --------------------------------------------------------
    // Acknowledge the interrupt.
    //
    // GICv2 CPU interface:
    //   GICC_IAR = 0x0801000C
    // --------------------------------------------------------

    mov x2, #0x8010
    lsl x2, x2, #12

    ldr w19, [x2, #0x0C]


    // --------------------------------------------------------
    // Call Rust:
    //
    // x0 = &mut ExceptionFrame
    // x1 = interrupt ID
    // --------------------------------------------------------

    mov x0, sp
    mov w1, w19

    bl exception_irq_rust

    //Rebuild GICC base
    mov x2, #0x8010
    lsl x2, x2, #12

    // --------------------------------------------------------
    // Complete the interrupt.
    //
    // GICC_EOIR = 0x08010010
    // --------------------------------------------------------

    str w19, [x2, #0x10]


    // Restore ELR/SPSR.
    ldr x1, [sp, #256]
    msr elr_el1, x1

    ldr x1, [sp, #264]
    msr spsr_el1, x1

    // Restore x0-x29.
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

    // Restore original x30.
    ldr x30, [sp, #240]

    // Discard frame and temporary x30 slot.
    add sp, sp, #288
    add sp, sp, #16

    eret

exception_fiq_entry:
    b .

exception_serror_entry:
    b .