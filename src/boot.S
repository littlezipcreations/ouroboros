.section .text.boot
.global _start

_start:
    // Set up the kernel stack.
    adrp x0, STACK
    add  x0, x0, :lo12:STACK
    add  x0, x0, #16384
    mov  sp, x0

    // Now Rust is allowed to use the stack.
    bl rust_start

1:
    wfe
    b 1b