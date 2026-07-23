/* Linker script for kernel builds */

PHDRS
{
    text    PT_LOAD     FLAGS(5);
    rodata  PT_LOAD     FLAGS(4);
    data    PT_LOAD     FLAGS(6);
    dynamic PT_DYNAMIC  FLAGS(6);
    percpu  PT_LOAD     FLAGS(6);
}

SECTIONS
{
    . = 0;
    LD_KERNEL_START = .;

    . = ALIGN(CONSTANT(MAXPAGESIZE));

    LD_TEXT_START = .;

    .boot : {
        KEEP(*(.boot.init))
        KEEP(*(.boot))
        KEEP(*(.boot.fini))
    } :text

    . = ALIGN(CONSTANT(MAXPAGESIZE));

    /* Code */
    .text : {
        *(.text .text.*)
        *(.fini)
        *(.plt .plt.*)
    } :text

    LD_TEXT_END = .;

    . = ALIGN(CONSTANT(MAXPAGESIZE));

    /* Read-only data */
    .rodata : {
        LD_RODATA_START = .;

        LD_INIT_CTORS_START = .;
        *(.initgraph.ctors)
        LD_INIT_CTORS_END = .;

        LD_PERCPU_CTORS_START = .;
        *(.percpu.ctors)
        LD_PERCPU_CTORS_END = .;

        *(.rodata .rodata.*)
        *(.got .got.*)
        LD_RODATA_END = .;
    } :rodata

    . = ALIGN(CONSTANT(MAXPAGESIZE));

    /* Read-Write data */
    .data : {
        LD_DATA_START = .;

        LD_INIT_START = .;
        *(.initgraph.nodes)
        LD_INIT_END = .;

        *(.data .data.*)
    } :data

    /* Dynamic symbols */
    .dynsym : {
        LD_DYNSYM_START = .;
        *(.dynsym)
        LD_DYNSYM_END = .;
    } :data

    /* Dynamic symbol names */
    .dynstr : {
        LD_DYNSTR_START = .;
        *(.dynstr)
        LD_DYNSTR_END = .;
    } :data

    /* Dynamic tags */
    .dynamic : {
        *(.dynamic)
    } :data :dynamic

    /* Zero-initialized memory */
    .bss ALIGN(CONSTANT(MAXPAGESIZE)) : {
        *(.*bss .*bss.*)
        *(COMMON)

        . = ALIGN(16);
        LD_STACK_BOTTOM = .;
        . += 0x4000;
        LD_STACK_TOP = .;
    } :data

    /* Per-CPU data block. MUST always come last. */
    .percpu ALIGN(CONSTANT(MAXPAGESIZE)) : {
        LD_PERCPU_START = .;
        *(.percpu.init)
        *(.percpu)
        . = ALIGN(CONSTANT(MAXPAGESIZE));
        LD_PERCPU_END = .;
        LD_DATA_END = .;
    } :percpu

    LD_KERNEL_END = .;

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
