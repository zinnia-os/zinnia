loader_sources += files(
    'enter.S',
    'paging.c',
)

loader_c_args += [
    '-mno-red-zone',
    '-masm=intel',
]

loader_objcopy_format = 'elf64-x86-64'
