project(
    'zinnia',
    license: 'BSD-3-Clause',
    version: '0.1.0',
    meson_version: '>=1.6.0',
    default_options: [
        'c_std=gnu23',
        'werror=true',
        'warning_level=2',
        'strip=true',
    ],
)

if get_option('headers_only')
    install_subdir('include/zinnia', install_dir: get_option('includedir'))
    pkg = import('pkgconfig')
    pkg.generate(name: 'zinnia', description: 'Zinnia user API')
    subdir_done()
endif

add_languages('c', native: false, required: true)

# Check if we're using a sane compiler.
c_compiler = meson.get_compiler('c')
python = import('python').find_installation('python3')

# We require a compiler that supports the GNU C dialect.
assert(c_compiler.has_define('__GNUC__'))

if get_option('tools_only')
    subdir('tools')
    subdir_done()
endif

subdir('tests/kernel')

cfg = configuration_data()
cfg.set_quoted('ZINNIA_VERSION', meson.project_version())
cfg.set_quoted('ZINNIA_ARCH', target_machine.cpu_family())
cfg.set_quoted('ZINNIA_COMPILER_ID', c_compiler.get_id() + ' ' + c_compiler.version())
cfg.set_quoted('ZINNIA_LINKER_ID', c_compiler.get_linker_id())

# Parse kernel config
kv = import('keyval')
kconfig = kv.load(get_option('config_path'))
foreach k, v : kconfig
    if v == 'y'
        cfg.set10(k, true)
    elif v == 'n'
        cfg.set10(k, false)
    else
        cfg.set(k, v)
    endif
endforeach

uapi_includes = include_directories('include')

subdir('kernel')

# Print a summary
summary(mod_summary, section: 'Driver configuration')
summary(kconfig, bool_yn: true, section: 'Kernel configuration')

if kconfig.get('CONFIG_KTEST', 'n') == 'y'
    subdir('tests')
endif
