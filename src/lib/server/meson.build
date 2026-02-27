libserver = static_library(
    'server',
    'crt.c',
    'string.c',
    'vdso.c',
    c_args: ['-ffreestanding', '-nostdinc'],
    include_directories: common_includes,
)
