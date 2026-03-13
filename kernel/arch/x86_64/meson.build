kernel_c_args += [
    '-mgeneral-regs-only',
    '-mno-red-zone',
    '-mcmodel=kernel',
]

kernel_sources += files(
    'apic.c',
    'gdt.c',
    'hpet.c',
    'idt.c',
    'percpu.c',
    'pmap.c',
    'sched.c',
    'serial.c',
    'smp.c',
    'sys.c',
    'syscall.c',
    'trampoline.S',
    'tsc.c',
    'usercopy.c',
)
