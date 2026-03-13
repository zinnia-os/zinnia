kernel_c_args = [
    '-ffreestanding',
    '-fno-stack-protector',
    '-fno-omit-frame-pointer',
    '-fno-pie',
    '-nostdinc',
    '-DUACPI_OVERRIDE_TYPES',
    '-DUACPI_FORMATTED_LOGGING',
]

kernel_lds = meson.current_source_dir() / 'kernel.ld'

kernel_link_args = [
    '-ffreestanding',
    '-nostdlib',
    '-T' + kernel_lds,
]

kernel_includes = include_directories('arch' / target_machine.cpu_family() / 'include')

kernel_sources = []

subdir('arch' / target_machine.cpu_family())
subdir('boot')
subdir('core')
subdir('klibc')
subdir('mem')
subdir('syscalls')
subdir('vfs')
