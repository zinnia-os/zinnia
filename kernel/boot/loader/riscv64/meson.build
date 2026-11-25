loader_sources += files(
    'enter.S',
    'paging.c',
)

loader_c_args += [
    '-march=rv64imac_zicsr_zifencei',
    '-mabi=lp64',
    '-mcmodel=medany',
]

loader_objcopy_format = 'elf64-littleriscv'
