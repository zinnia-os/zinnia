/* SPDX-License-Identifier: BSD-3-Clause */

ENTRY(_start)

PHDRS {
    text    PT_LOAD     FLAGS(5);
    rodata  PT_LOAD     FLAGS(4);
    data    PT_LOAD     FLAGS(6);
    dynamic PT_DYNAMIC  FLAGS(6);
    percpu  PT_LOAD     FLAGS(6);
    cmdline 0x60000005;
    boot    0x60000008;
}

SECTIONS
{
    . = 0xffffffff80000000;
    __ld_kernel_start = .;

    .text ALIGN(CONSTANT(MAXPAGESIZE)) : {
        __ld_text_start = .;

        KEEP(*(.init.text))
        *(.text .text.*)
        *(.plt .plt.*)

        __ld_text_end = .;
    } :text

    .cmdline ALIGN(CONSTANT(MAXPAGESIZE)) : {
        __ld_rodata_start = .;
        __ld_cmdline_start = .;
        KEEP(*(.cmdline))
        __ld_cmdline_end = .;
    } :rodata :cmdline

    .rodata : {
        *(.rodata .rodata.*)

        /* Builtin module entry points */
        . = ALIGN(8);
        __ld_mod_entry_start = .;
        KEEP(*(.mod.entry .mod.entry.*))
        __ld_mod_entry_end = .;

        . = ALIGN(8);
        __ld_initgraph_nodes_start = .;
        KEEP(*(.initgraph.nodes))
        __ld_initgraph_nodes_end = .;

        __ld_initgraph_edges_start = .;
        KEEP(*(.initgraph.edges))
        __ld_initgraph_edges_end = .;

        . = ALIGN(8);
        __ld_ktest_start = .;
        KEEP(*(.ktest))
        __ld_ktest_end = .;

        *(.got .got.*)

        __ld_rodata_end = .;
    } :rodata

    .data ALIGN(CONSTANT(MAXPAGESIZE)) : {
        __ld_data_start = .;

        KEEP(*(SORT_BY_NAME(.init.data.*)))
        KEEP(*(.init.data))
        *(.data .data.*)
    } :data

    .boot : {
        KEEP(*(.boot.info))
    } :data :boot

    .dynsym : {
        __ld_dynsym_start = .;
        KEEP(*(.dynsym*))
        __ld_dynsym_end = .;
    } :data

    .dynstr : {
        __ld_dynstr_start = .;
        KEEP(*(.dynstr*))
        __ld_dynstr_end = .;
    } :data

    .dynamic : {
        *(.dynamic*)
    } :data :dynamic

    .bss ALIGN(CONSTANT(MAXPAGESIZE)) (NOLOAD) : {
        *(.*bss .*bss.*)
        *(COMMON)

        /* Small stack to bootstrap the allocator */
        . = ALIGN(16);
        __ld_stack_bottom = .;
        . += 0x2000;
        __ld_stack_top = .;
    } :data

    /* Per-CPU data block. MUST always come last for it to work. */
    .percpu ALIGN(CONSTANT(MAXPAGESIZE)) : {
        __ld_percpu_start = .;
        KEEP(*(.percpu.init .percpu.init*))
        KEEP(*(.percpu.data .percpu.data*))
        . = ALIGN(CONSTANT(MAXPAGESIZE));
        __ld_percpu_end = .;

        __ld_data_end = .;
    } :percpu

    __ld_kernel_end = .;

    /DISCARD/ :
    {
        *(.comment*)
        *(.eh_frame*)
        *(.rel.eh_frame*)
        *(.gcc_except_table*)
        *(.note.gnu.property)
        *(.interp)
    }
}
