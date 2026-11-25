kernel_c_args += [
    '-march=rv64imac_zicsr_zifencei',
    '-mabi=lp64',
    '-mcmodel=medany',
]

kernel_sources += files(
    'start.c',
    'stubs.c',
    'sbi.c',
    'percpu.c',
    'trap.c',
    'sched.c',
    'signal.c',
    'timer.c',
    'usercopy.c',
)

if kconfig.get('CONFIG_ACPI', 'n') == 'y'
    kernel_sources += files('acpi.c')
endif
