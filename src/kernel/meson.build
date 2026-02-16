kernel_c_args = [
    '-ffreestanding',
    '-fno-stack-protector',
    '-fno-omit-frame-pointer',
    '-fno-pie',
    '-nostdinc',
    '-D__KERNEL__=1',
]

kernel_lds = meson.current_source_dir() / 'kernel.ld'

kernel_link_args = [
    '-ffreestanding',
    '-nostdlib',
    '-Wl,-nostdlib',
    '-static',
    '-T' + kernel_lds,
]

kernel_includes = include_directories(
    'arch' / target_machine.cpu_family() / 'include',
    'include',
)

kernel_sources = []

configure_file(configuration: cfg, output: 'config.h')

subdir('arch' / target_machine.cpu_family())
subdir('boot')
subdir('mem')
subdir('core')
subdir('syscalls')

subdir('klibc')
subdir('vdso')

executable(
    'zinnia',
    include_directories: [common_includes, kernel_includes],
    c_args: kernel_c_args,
    link_args: kernel_link_args,
    link_depends: kernel_lds,
    sources: [kernel_sources, klibc_sources],
    objects: vdso,
    install: true,
    install_dir: '/boot',
)
