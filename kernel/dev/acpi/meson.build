uacpi = subproject('uacpi')
kernel_c_args += ['-DUACPI_SIZED_FREES']

kernel_sources += uacpi.get_variable('sources')
kernel_sources += files(
    'init.c',
    'uacpi_glue.c',
)

if kconfig.get('CONFIG_PCI', 'n') == 'y'
    kernel_sources += files('mcfg.c')
endif

kernel_includes += uacpi.get_variable('includes')
kernel_includes += include_directories('include')

kernel_c_args += [
    '-DUACPI_FORMATTED_LOGGING',
    '-DUACPI_OVERRIDE_TYPES',
]
