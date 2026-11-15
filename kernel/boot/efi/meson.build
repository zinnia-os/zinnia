efistub_sources = files(
    'header.c',
    'main.c',
    'reloc.c',
)

efistub_lds = meson.current_source_dir() / 'stub.ld'

subdir(target_machine.cpu_family())

efistub_elf = executable(
    'efistub.elf',
    sources: [loader_sources, efistub_sources, loader_kernel],
    include_directories: common_includes,
    c_args: loader_c_args,
    link_args: [
        '-nostdlib',
        '-Wl,-pie',
        '-Wl,-T,' + efistub_lds,
        '-Wl,--no-dynamic-linker',
        '-Wl,-z,text',
        '-Wl,-z,norelro',
        '-Wl,-z,noexecstack',
        '-Wl,-z,max-page-size=0x1000',
        '-Wl,--build-id=none',
    ],
    link_depends: efistub_lds,
    pie: true,
)

zinnia_efi = custom_target(
    'zinnia.efi',
    input: efistub_elf,
    output: 'zinnia.efi',
    command: [objcopy, '-O', 'binary', '@INPUT@', '@OUTPUT@'],
    build_by_default: true,
    install: true,
    install_dir: zinnia_bootdir,
)
