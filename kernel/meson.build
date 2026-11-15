add_project_arguments(
    '-Wno-unused',
    '-Wno-unused-parameter',
    language: 'c',
)

common_c_args = [
    '-fno-stack-protector',
    '-fno-omit-frame-pointer',
    '-ffunction-sections',
    '-fdata-sections',
    '-ffreestanding',
    '-nostdinc',
    '-D__IN_KERNEL',
]

common_link_args = [
    '-ffreestanding',
    '-nostdlib',
    '-Wl,-z,noexecstack',
    '-Wl,--gc-sections',
    '-Wl,--build-id=none',
    '-Wl,--hash-style=both',
    '-Wl,-Bdynamic',
]

common_includes = [
    include_directories('.'),
    include_directories('include'),
    uapi_includes,
]

kernel_c_args = []

kernel_lds = meson.current_source_dir() / 'boot' / 'kernel.ld'
kernel_link_args = [
    '-T' + kernel_lds,
    '-shared',
    '-Wl,-shared',
    '-Wl,-Bsymbolic',
    '-Wl,-soname,zinnia',
]

kernel_includes = []
kernel_sources = []
modules = {}

# These dirs go first so we can add arch-specific flags to drivers.
subdir('arch' / target_machine.cpu_family())
subdir('lib')
subdir('mem')
subdir('syscalls')

# Device and file system drivers.
subdir('dev')
subdir('fs')

if kconfig.get('CONFIG_KTEST', 'n') == 'y'
    kernel_sources += ktest_sources
endif

configure_file(
    configuration: cfg,
    output: 'config.h',
)

zinnia_datadir = get_option('datadir') / 'zinnia'

mod_summary = {}
modular_modules = {}
builtin_libs = []

foreach name, sources : modules
    state = kconfig.get('CONFIG_' + name.to_upper(), 'n')
    if state == 'n'
        continue
    endif
    mod_summary += {name: state == 'm' ? 'module' : 'builtin'}
    if state == 'm'
        modular_modules += {name: sources}
        continue
    endif
    builtin_libs += static_library(
        name,
        sources: sources,
        c_args: [common_c_args, kernel_c_args, '-D__MODULE_NAME="' + name + '"'],
        include_directories: [common_includes, kernel_includes],
        pic: true,
    )
endforeach

# Kernel
kernel = executable(
    'zinnia',
    include_directories: [
        common_includes,
        kernel_includes,
    ],
    c_args: [common_c_args, kernel_c_args],
    link_args: [common_link_args, kernel_link_args],
    link_depends: kernel_lds,
    sources: kernel_sources,
    link_whole: builtin_libs,
    pie: true,
    install: true,
    install_dir: zinnia_datadir,
)

subdir('boot')

foreach name, sources : modular_modules
    shared_library(
        name,
        sources: sources,
        c_args: [
            common_c_args,
            kernel_c_args,
            '-fPIC',
            '-D__IN_MODULE',
            '-D__MODULE_NAME="' + name + '"',
        ],
        link_args: [
            common_link_args,
            '-Wl,-shared',
            '-T' + module_lds,
            kernel.full_path(),
        ],
        link_depends: [module_lds, kernel],
        include_directories: [common_includes, kernel_includes],
        name_prefix: '',
        name_suffix: 'kso',
        install: true,
        install_dir: zinnia_datadir / 'modules',
    )
endforeach
