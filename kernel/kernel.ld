ENTRY(_start)

PHDRS {
    text    PT_LOAD     FLAGS(5);
    rodata  PT_LOAD     FLAGS(4);
    data    PT_LOAD     FLAGS(6);
    dynamic PT_DYNAMIC  FLAGS(6);
    percpu  PT_LOAD     FLAGS(6);
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

    .rodata ALIGN(CONSTANT(MAXPAGESIZE)) : {
        __ld_rodata_start = .;

        __ld_cmdline_start = .;
        KEEP(*(.cmdline))
        __ld_cmdline_end = .;

        *(.rodata .rodata.*)
        *(.got .got.*)

        __ld_rodata_end = .;
    } :rodata

    .data ALIGN(CONSTANT(MAXPAGESIZE)) : {
        __ld_data_start = .;

        __ld_console_start = .;
        KEEP(*(.console))
        __ld_console_end = .;

        KEEP(*(SORT_BY_NAME(.init.data.*)))
        KEEP(*(.init.data))
        *(.data .data.*)
    } :data

    .dynsym : {
        __ld_dynsym_start = .;
        *(.dynsym)
        __ld_dynsym_end = .;
    } :data

    .dynstr : {
        __ld_dynstr_start = .;
        *(.dynstr)
        __ld_dynstr_end = .;
    } :data

    .dynamic : {
        *(.dynamic)
    } :data :dynamic

    .bss : {
        /* Small stack to bootstrap the allocator */
        __ld_stack_bottom = .;
        . += 0x2000;
        __ld_stack_top = .;
        *(.*bss .*bss.*)
        *(COMMON)
    } :data

    /* Per-CPU data block. MUST always come last. */
    .percpu ALIGN(CONSTANT(MAXPAGESIZE)) : {
        __ld_percpu_start = .;
        KEEP(*(.percpu))
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
