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

    /* Page tables: explicitly place each at a unique 4KB-aligned address */
    . = ALIGN(4096);
    .page_table_l0 : ALIGN(4096)
    {
        KEEP(*(.page_table_l0))
    }

    . = ALIGN(4096);
    .page_table_l1_low : ALIGN(4096)
    {
        KEEP(*(.page_table_l1_low))
    }

    . = ALIGN(4096);
    .page_table_l1_ram : ALIGN(4096)
    {
        KEEP(*(.page_table_l1_ram))
    }

    . = ALIGN(4096);
    .page_table_l2_device : ALIGN(4096)
    {
        KEEP(*(.page_table_l2_device))
    }

    . = ALIGN(4096);
    .page_table_l2_ram : ALIGN(4096)
    {
        KEEP(*(.page_table_l2_ram))
    }
    . = ALIGN(4096);
    .page_table_l3 : ALIGN(4096)
    {
        KEEP(*(.page_table_l3))
    }

    . = ALIGN(4096);
    .page_bitmap : ALIGN(4096)
    {
        KEEP(*(.page_bitmap))
    }

    .scheduler : ALIGN(4096)
    {
        KEEP(*(.scheduler))
    }

    .kernel_stack : ALIGN(4096)
    {
        KEEP(*(.kernel_stack))
    }

    .bss : ALIGN(4096)
    {
        *(.bss*)
        *(COMMON)
    }

    _kernel_end = .;
}