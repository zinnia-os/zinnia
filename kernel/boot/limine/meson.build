limine_lds = meson.current_source_dir() / 'loader.ld'

limine_c_args = [loader_c_args, '-fno-pic']
if target_machine.cpu_family() == 'x86_64'
    limine_c_args += '-mcmodel=kernel'
endif

executable(
    'zinnia-limine',
    sources: [loader_sources, files('main.c'), loader_kernel],
    include_directories: common_includes,
    c_args: limine_c_args,
    link_args: [
        '-nostdlib',
        '-static',
        '-Wl,-T,' + limine_lds,
        '-Wl,-z,noexecstack',
        '-Wl,-z,max-page-size=0x1000',
        '-Wl,--build-id=none',
    ],
    link_depends: limine_lds,
    pie: false,
    install: true,
    install_dir: zinnia_bootdir,
)
