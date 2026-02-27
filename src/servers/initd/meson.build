executable(
    'initd',
    'main.c',
    c_args: ['-ffreestanding'],
    link_args: ['-nostdlib', '-static'],
    include_directories: common_includes,
    link_with: libserver,
    install: true,
)
