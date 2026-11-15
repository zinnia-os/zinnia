/* SPDX-License-Identifier: BSD-3-Clause */

ENTRY(_start)

PHDRS
{
    text    PT_LOAD    FLAGS(5);
    rodata  PT_LOAD    FLAGS(4);
    data    PT_LOAD    FLAGS(6);
    dynamic PT_DYNAMIC FLAGS(6);
}

SECTIONS
{
    . = 0;

    .header : {
        __image_base = .;
        KEEP(*(.header))
    } :text

    . = ALIGN(0x1000);
    .text : {
        __text_start = .;
        *(.text .text.*)
        . = ALIGN(0x1000);
        __text_end = .;
    } :text

    .reloc : {
        __reloc_start = .;
        KEEP(*(.reloc))
        __reloc_data_end = .;
        . = ALIGN(0x1000);
        __reloc_end = .;
    } :rodata

    .rodata : {
        __rodata_start = .;
        *(.rodata .rodata.*)
        . = ALIGN(16);
        __kernel_start = .;
        KEEP(*(.kernel))
        __kernel_end = .;
    } :rodata

    .hash : { *(.hash) } :rodata
    .gnu.hash : { *(.gnu.hash) } :rodata
    .dynsym : { *(.dynsym) } :rodata
    .dynstr : { *(.dynstr) } :rodata
    .rela.dyn : { *(.rela.*) } :rodata
    .relr.dyn : { *(.relr.dyn) } :rodata

    .rodata.end : {
        . = ALIGN(0x1000);
        __rodata_end = .;
    } :rodata

    .dynamic : {
        __data_start = .;
        *(.dynamic)
    } :data :dynamic

    .data : {
        *(.data .data.*)
        *(.got .got.*)
        . = ALIGN(0x1000);
        __data_raw_end = .;
    } :data

    .bss (NOLOAD) : {
        __bss_start = .;
        *(.bss .bss.*)
        *(COMMON)
        . = ALIGN(0x1000);
        __bss_end = .;
    } :data

    __image_end = .;

    __pe_code_size = ABSOLUTE((__text_end - __text_start) << 32);
    __pe_data_sizes = ABSOLUTE(((__reloc_end - __reloc_start) + (__rodata_end - __rodata_start) +
                                (__data_raw_end - __data_start)) | ((__bss_end - __bss_start) << 32));
    __pe_entry = ABSOLUTE(_start | (__text_start << 32));
    __pe_image_sizes = ABSOLUTE(__image_end | (__text_start << 32));
    __pe_base_relocs = ABSOLUTE(__reloc_start | ((__reloc_data_end - __reloc_start) << 32));
    __pe_text_virtual = ABSOLUTE((__text_end - __text_start) | (__text_start << 32));
    __pe_text_raw = ABSOLUTE((__text_end - __text_start) | (__text_start << 32));
    __pe_reloc_virtual = ABSOLUTE((__reloc_data_end - __reloc_start) | (__reloc_start << 32));
    __pe_reloc_raw = ABSOLUTE((__reloc_end - __reloc_start) | (__reloc_start << 32));
    __pe_rodata_virtual = ABSOLUTE((__rodata_end - __rodata_start) | (__rodata_start << 32));
    __pe_rodata_raw = ABSOLUTE((__rodata_end - __rodata_start) | (__rodata_start << 32));
    __pe_data_virtual = ABSOLUTE((__bss_end - __data_start) | (__data_start << 32));
    __pe_data_raw = ABSOLUTE((__data_raw_end - __data_start) | (__data_start << 32));

    /DISCARD/ : {
        *(.comment*)
        *(.eh_frame*)
        *(.note*)
        *(.interp)
    }
}
