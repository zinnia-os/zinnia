subdir('block')
subdir('char')
subdir('dt')
subdir('tty')
subdir('video')

if kconfig.get('CONFIG_PCI', 'n') == 'y'
    subdir('pci')
endif

if kconfig.get('CONFIG_ACPI', 'n') == 'y'
    subdir('acpi')
endif

module_lds = meson.current_source_dir() / 'module.ld'
