project(
    'zinnia',
    license: 'MIT',
    version: '0.1.0',
    meson_version: '>=1.1.0',
    default_options: [
        'c_std=gnu23',
        'werror=true',
        'warning_level=2',
        'strip=true',
    ],
)

add_languages('c', native: false)
c_compiler = meson.get_compiler('c')

add_project_arguments(
    '-Wno-unused',
    '-Wno-unused-parameter',
    '-fno-stack-protector',
    '-ffunction-sections',
    '-fdata-sections',
    language: 'c',
)

add_project_link_arguments(
    '-znoexecstack',
    '-Wl,--gc-sections',
    '-Wl,--build-id=none',
    language: 'c',
)

if target_machine.cpu_family() == 'x86_64'
    add_project_arguments('-masm=intel', language: 'c')
endif

common_includes = include_directories('include')

cfg = configuration_data()
cfg.set_quoted('ZINNIA_VERSION', meson.project_version())
cfg.set_quoted('ZINNIA_ARCH', target_machine.cpu_family())
cfg.set_quoted('ZINNIA_COMPILER_ID', c_compiler.get_id() + ' ' + c_compiler.version())
cfg.set_quoted('ZINNIA_LINKER_ID', c_compiler.get_linker_id())
configure_file(configuration: cfg, output: 'config.h')

uacpi = subproject('uacpi')
uacpi_sources = uacpi.get_variable('sources')
uacpi_includes = uacpi.get_variable('includes')

install_dir = get_option('libdir') / 'zinnia'

subdir('kernel')
subdir('drivers')

executable(
    'zinnia',
    include_directories: [
        common_includes,
        kernel_includes,
        uacpi_includes,
    ],
    c_args: kernel_c_args,
    link_args: kernel_link_args,
    link_depends: kernel_lds,
    sources: [
        kernel_sources,
        uacpi_sources,
    ],
    install: true,
    install_dir: install_dir,
)
