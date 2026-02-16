configure_file(configuration: cfg, output: 'config.h')

executable(
    'posixd',
    'main.c',
    'uname.c',
    include_directories: [common_includes, 'include'],
    install: true,
)
