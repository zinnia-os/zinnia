project(
    'menix',
    'c',
    license: 'MIT',
    license_files: ['LICENSE'],
    version: '0.1.0',
    meson_version: '>=1.1.0',
)

common_c_args = [
    '-Wall',
    '-Werror',
    '-Wextra',
    '-Wno-unused',
    '-std=gnu23',
]

common_link_args = [
    '-znoexecstack',
    '-Wl,--gc-sections',
    '-Wl,--build-id=none',
]

common_includes = include_directories('include')

# Config file
common_cfg = configuration_data()
common_cfg.set_quoted('MENIX_VERSION', meson.project_version())
common_cfg.set_quoted('MENIX_ARCH', target_machine.cpu_family())
c_compiler = meson.get_compiler('c')
common_cfg.set_quoted('MENIX_COMPILER_ID', c_compiler.get_id() + ' ' + c_compiler.version())
common_cfg.set_quoted('MENIX_LINKER_ID', c_compiler.get_linker_id())

subdir('kernel')
subdir('lib')
subdir('servers')
