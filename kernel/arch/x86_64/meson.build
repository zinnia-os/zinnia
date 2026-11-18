kernel_c_args += [
    '-mgeneral-regs-only',
    '-mno-red-zone',
    '-masm=intel',
]

kernel_sources += files(
    'apic.c',
    'archctl.c',
    'gdt.c',
    'hpet.c',
    'idt.c',
    'ioapic.c',
    'irq.c',
    'percpu.c',
    'pmap.c',
    'random.c',
    'rtc.c',
    'sched.c',
    'serial.c',
    'smp.c',
    'start.c',
    'syscall.c',
    'tsc.c',
    'usercopy.c',
)

if kconfig.get('CONFIG_PCI', 'n') == 'y'
    kernel_sources += files('pci.c')
endif

if kconfig.get('CONFIG_ACPI', 'n') == 'y'
    kernel_sources += files('acpi.c')
endif
