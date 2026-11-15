/* SPDX-License-Identifier: BSD-3-Clause */

/* Linker script for kernel modules */

ENTRY(__module_entry)

PHDRS
{
    text        PT_LOAD     FLAGS(5);
    rodata      PT_LOAD     FLAGS(4);
    data        PT_LOAD     FLAGS(6);
    dynamic     PT_DYNAMIC  FLAGS(6);
    modversion  0x60000001;
    modauthor   0x60000002;
    moddesc     0x60000003;
    modlicense  0x60000004;
    modcmdline  0x60000005;
    modexit     0x60000006;
    modmatch    0x60000007;
}

SECTIONS
{
    . = ALIGN(CONSTANT(MAXPAGESIZE));

    /* Code */
    .text : {
        *(.init .init.*)
        *(.text .text.*)
        *(.fini .fini.*)
        *(.plt .plt.*)
    } :text

    . = ALIGN(CONSTANT(MAXPAGESIZE));

    /* Read-only data */
    .hash : {
        *(.hash)
    } :rodata

    .rodata : {
        *(.rodata .rodata.*)
        *(.got .got.*)
        *(.got.plt .got.plt.*)
    } :rodata

    .mod.version : {
        KEEP(*(.mod.version))
    } :rodata :modversion

    .mod.author : {
        KEEP(*(.mod.author))
    } :rodata :modauthor

    .mod.desc : {
        KEEP(*(.mod.desc))
    } :rodata :moddesc

    .mod.license : {
        KEEP(*(.mod.license))
    } :rodata :modlicense

    .cmdline : {
        KEEP(*(.cmdline))
    } :rodata :modcmdline

    .mod.exit : {
        KEEP(*(.mod.exit))
    } :rodata :modexit

    .mod.match : {
        KEEP(*(.mod.match))
    } :rodata :modmatch

    . = ALIGN(CONSTANT(MAXPAGESIZE));

    /* Read-Write data */
    .data : {
        *(.data)
        *(.data.*)
    } :data

    /* Zero-initialized memory */
    .bss : {
        *(.*bss .*bss.*)
        *(COMMON)
    } :data

    .dynsym : {
        *(.dynsym .dynsym.*)
    } :data

    .dynstr : {
        *(.dynstr .dynstr.*)
    } :data

    .rela.dyn : {
        *(.rela.dyn .rela.dyn.*)
    } :data

    .relr.dyn : {
        *(.relr.dyn .relr.dyn.*)
    } :data

    /* Dynamic tags */
    .dynamic : {
        *(.dynamic)
    } :data :dynamic

    /DISCARD/ :
    {
        *(.comment*)
        *(.eh_frame*)
        *(.rel.eh_frame*)
        *(.gcc_except_table*)
        *(.note*)
        *(.interp)
    }
}
