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
// frame.sp contains the SP of the context being resumed.
//
// If the destination SPSR says EL0t, frame.sp is written to
// SP_EL0.
//
// If it says EL1h/EL1t, frame.sp is written to SP.
//
// x20 is temporarily used to hold the destination SP.
// TPIDR_EL1 temporarily holds the destination x20.
// ============================================================
exception_restore_context:

    // --------------------------------------------------------
    // Save destination SP.
    // --------------------------------------------------------
    ldr     x20, [sp, #0x0F8]

    // --------------------------------------------------------
    // Save destination x20 in TPIDR_EL1.
    // --------------------------------------------------------
    ldr     x1, [sp, #0x0A0]
    msr     tpidr_el1, x1

    // --------------------------------------------------------
    // Restore ELR/SPSR.
    // --------------------------------------------------------
    ldr     x1, [sp, #0x100]
    msr     elr_el1, x1

    ldr     x1, [sp, #0x108]
    msr     spsr_el1, x1

    // --------------------------------------------------------
    // Determine destination mode NOW, before x1 is restored.
    //
    // 0x0 = EL0t
    // 0x4 = EL1t
    // 0x5 = EL1h
    // --------------------------------------------------------
    mrs     x1, spsr_el1
    and     x1, x1, #0xF
    cmp     x1, #0
    b.eq    .Lrestore_el0

    // ========================================================
    // Destination = EL1
    // ========================================================

    // --------------------------------------------------------
    // Restore registers except x20.
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
    // Remove exception frame and saved x30.
    // --------------------------------------------------------
    add     sp, sp, #0x120
    add     sp, sp, #0x010

    // --------------------------------------------------------
    // Switch to destination EL1 stack.
    // --------------------------------------------------------
    mov     sp, x20

    // --------------------------------------------------------
    // Restore destination x20.
    // --------------------------------------------------------
    mrs     x20, tpidr_el1

    eret


.Lrestore_el0:

    // ========================================================
    // Destination = EL0
    // ========================================================

    // --------------------------------------------------------
    // Install destination user stack.
    //
    // DO NOT modify SP here. SP is still the EL1 exception
    // stack and must remain valid until eret.
    // --------------------------------------------------------
    msr     sp_el0, x20

    // --------------------------------------------------------
    // Restore registers except x20.
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
    // Remove exception frame and saved x30.
    //
    // Remain on the EL1 exception stack.
    // --------------------------------------------------------
    add     sp, sp, #0x120
    add     sp, sp, #0x010

    // --------------------------------------------------------
    // Restore destination x20.
    // --------------------------------------------------------
    mrs     x20, tpidr_el1

    eret
// ============================================================
// Common exception-frame construction
//
// On entry:
//   SP = SP_EL1
//
// Before changing SP we save x30.
// ============================================================

exception_sync_entry:
    str     x30, [sp, #-16]!
    sub     sp, sp, #288

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

    // x30 was saved before allocating the frame.
    ldr     x1, [sp, #0x120]
    str     x1, [sp, #0x0F0]

    // --------------------------------------------------------
    // Determine the stack pointer belonging to the interrupted
    // context.
    //
    // SPSR_EL1.M == 0 → EL0t
    // Otherwise      → EL1
    // --------------------------------------------------------
    mrs     x1, spsr_el1
    and     x1, x1, #0xF
    cmp     x1, #0
    b.eq    .Lsync_from_el0

    // Exception originated from EL1.
    add     x1, sp, #0x130
    str     x1, [sp, #0x0F8]
    b       .Lsync_save_state

.Lsync_from_el0:
    // Exception originated from EL0.
    mrs     x1, sp_el0
    str     x1, [sp, #0x0F8]

.Lsync_save_state:

    mrs     x1, elr_el1
    str     x1, [sp, #0x100]

    mrs     x1, spsr_el1
    str     x1, [sp, #0x108]

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
    sub     sp, sp, #288

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

    ldr     x1, [sp, #0x120]
    str     x1, [sp, #0x0F0]

    // Save interrupted context's SP.
    mrs     x1, spsr_el1
    and     x1, x1, #0xF
    cmp     x1, #0
    b.eq    .Lirq_from_el0

    add     x1, sp, #0x130
    str     x1, [sp, #0x0F8]
    b       .Lirq_save_state

.Lirq_from_el0:
    mrs     x1, sp_el0
    str     x1, [sp, #0x0F8]

.Lirq_save_state:

    mrs     x1, elr_el1
    str     x1, [sp, #0x100]

    mrs     x1, spsr_el1
    str     x1, [sp, #0x108]

    mrs     x1, esr_el1
    str     x1, [sp, #0x110]

    mrs     x1, far_el1
    str     x1, [sp, #0x118]

    // Timer/GIC handling.
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
    sub     sp, sp, #288

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

    ldr     x1, [sp, #0x120]
    str     x1, [sp, #0x0F0]

    mrs     x1, spsr_el1
    and     x1, x1, #0xF
    cmp     x1, #0
    b.eq    .Lfiq_from_el0

    add     x1, sp, #0x130
    str     x1, [sp, #0x0F8]
    b       .Lfiq_save_state

.Lfiq_from_el0:
    mrs     x1, sp_el0
    str     x1, [sp, #0x0F8]

.Lfiq_save_state:

    mrs     x1, elr_el1
    str     x1, [sp, #0x100]

    mrs     x1, spsr_el1
    str     x1, [sp, #0x108]

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