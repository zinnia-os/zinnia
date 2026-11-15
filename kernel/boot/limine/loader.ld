/* SPDX-License-Identifier: BSD-3-Clause */

ENTRY(loader_main)

PHDRS
{
    text    PT_LOAD FLAGS(5);
    rodata  PT_LOAD FLAGS(4);
    data    PT_LOAD FLAGS(6);
}

SECTIONS
{
    . = 0xffffffffc0000000;

    .text : {
        *(.text .text.*)
    } :text

    . = ALIGN(0x1000);
    .rodata : {
        *(.rodata .rodata.*)
        . = ALIGN(16);
        __kernel_start = .;
        KEEP(*(.kernel))
        __kernel_end = .;
    } :rodata

    . = ALIGN(0x1000);
    .data : {
        KEEP(*(.limine_requests_start))
        KEEP(*(.limine_requests))
        KEEP(*(.limine_requests_end))
        *(.data .data.*)
    } :data

    .bss : {
        *(.bss .bss.*)
        *(COMMON)
    } :data

    /DISCARD/ : {
        *(.comment*)
        *(.eh_frame*)
        *(.note*)
        *(.interp)
    }
}
