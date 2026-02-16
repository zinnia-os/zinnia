kernel_c_args += [
    '-masm=intel',
    '-mgeneral-regs-only',
    '-mno-red-zone',
    '-mcmodel=kernel',
]

kernel_sources += files(
    'apic.c',
    'gdt.c',
    'idt.c',
    'mem.c',
    'percpu.c',
    'serial.c',
    'smp.c',
    'sys.c',
    'syscall.c',
    'trampoline.S',
    'tsc.c',
    'usercopy.c',
)
