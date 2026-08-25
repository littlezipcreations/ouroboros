ENTRY(_start)
SECTIONS
{
    . = 0x40000000;
    _kernel_start = .;
    .text : ALIGN(4096)
    {
        *(.text.boot)
        *(.text.exceptions)
        *(.text*)
    }
    .rodata : ALIGN(4096)
    {
        *(.rodata*)
    }
    .data : ALIGN(4096)
    {
        *(.data*)
    }
    .bss : ALIGN(4096)
    {
        *(.bss*)
        *(COMMON)
    }
    .scheduler : ALIGN(4096)
    {
        KEEP(*(.scheduler))
    }
    .kernel_stack : ALIGN(4096)
    {
        KEEP(*(.kernel_stack))
    }
    _kernel_end = .;
}