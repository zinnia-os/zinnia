executable(
    'posixd',
    'main.c',
    c_args: [common_c_args, '-static'],
    include_directories: [common_includes, 'include'],
    link_with: libmenix,
    install: true,
)
