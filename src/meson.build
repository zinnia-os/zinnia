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
        'b_lto=true',
    ],
)

if get_option('headers_only')
    install_subdir('include/zinnia', install_dir: get_option('includedir'))
    pkg = import('pkgconfig')
    pkg.generate(name: 'zinnia', description: 'Zinnia syscalls')
    subdir_done()
endif

add_languages('c', native: false)
c_compiler = meson.get_compiler('c')

add_project_arguments(
    '-Wno-unused',
    '-Wno-unused-parameter',
    '-fno-stack-protector',
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

strip = find_program('strip')
objcopy = find_program('objcopy')

build_kernel = get_option('build_kernel')
build_servers = get_option('build_servers')

uacpi = subproject('uacpi')

if build_kernel
    subdir('kernel')
endif

if build_servers
    subdir('lib')
    subdir('servers')
endif
