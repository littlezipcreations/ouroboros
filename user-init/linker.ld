ENTRY(_start)
SECTIONS
{
    . = 0x80000000;

    .text : ALIGN(16)
    {
        KEEP(*(.text._start))
        *(.text*)
    }
    .rodata : ALIGN(16){
        *(.rodata*)
    }

    .data : ALIGN(16)
    {
        *(.data*)
    }

    .bss : ALIGN(16)
    {
        *(.bss*)
        *(COMMON)
    }

    /DISCARD/ :
    {
        *(.eh_frame)
        *(.eh_frame_hdr)
        *(.comment)
    }
}