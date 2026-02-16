PHDRS {
    /* We only want to have a single PT_LOAD segment */
    text    PT_LOAD    FLAGS(5) FILEHDR PHDRS;
    dynamic PT_DYNAMIC FLAGS(4);
    note    PT_NOTE    FLAGS(4);
}

VERSION {
    ZINNIA_1 {
        global:
            __vdso_*;
        local: *;
    };
}

SECTIONS
{
    . = SIZEOF_HEADERS;

    .rodata : {
        *(.rodata .rodata.*)
        *(.data .data.*)
        *(.got .got.*)
    } :text

    .note : { *(.note.*) } :text :note

    .text : {
        *(.text .text.*)
        *(.*bss .*bss.*)
        *(COMMON)
    } :text

    .dynsym : { *(.dynsym) }
    .dynstr : { *(.dynstr) }
    .dynamic : { *(.dynamic) } :text :dynamic

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
