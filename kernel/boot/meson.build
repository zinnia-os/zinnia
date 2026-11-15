objcopy = find_program('objcopy')
zinnia_bootdir = zinnia_datadir / 'boot'

loader_sources = files(
    'loader/elf.c',
    'loader/lib.c',
    'loader/loader.c',
)

loader_c_args = [
    '-ffreestanding',
    '-nostdinc',
    '-fno-stack-protector',
    '-fno-omit-frame-pointer',
    '-mgeneral-regs-only',
]

loader_objcopy_format = 'unknown'

subdir('loader' / target_machine.cpu_family())

loader_kernel = custom_target(
    'loader-kernel.o',
    input: kernel,
    output: 'loader-kernel.o',
    command: [
        objcopy,
        '-I', 'binary',
        '-O', loader_objcopy_format,
        '--rename-section', '.data=.kernel,alloc,load,readonly,data,contents',
        '@INPUT@',
        '@OUTPUT@',
    ],
)

if kconfig.get('CONFIG_BOOT_EFISTUB', 'n') == 'y'
    subdir('efi')
endif

if kconfig.get('CONFIG_BOOT_LIMINE', 'n') == 'y'
    subdir('limine')
endif
