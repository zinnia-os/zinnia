fs = import('fs')

assert(
    kconfig.get('CONFIG_BOOT_EFISTUB', 'n') == 'y',
    'CONFIG_KTEST depends on CONFIG_BOOT_EFISTUB',
)

assert(
    target_machine.cpu_family() == 'x86_64',
    'CONFIG_KTEST is only supported on x86_64',
)

# Userspace tests.
utest_init = executable(
    'init',
    sources: files(
        'user/lib/crt0.S',
        'user/lib/utest.c',
        'user/memory.c',
        'user/system.c',
    ),
    include_directories: include_directories('..', '../include', '../kernel', '../kernel/include'),
    c_args: [
        '-ffreestanding',
        '-nostdinc',
        '-fno-pic',
        '-fno-stack-protector',
        '-masm=intel',
    ],
    link_args: [
        '-nostdlib',
        '-static',
        '-Wl,--no-dynamic-linker',
        '-Wl,-z,max-page-size=0x1000',
        '-Wl,-z,noexecstack',
        '-Wl,--build-id=none',
    ],
    pie: false,
)

test_initramfs = custom_target(
    'initramfs.tar',
    input: utest_init,
    output: 'initramfs.tar',
    command: [
        find_program('tar'),
        '--format=ustar',
        '-cf', '@OUTPUT@',
        '-C', '@OUTDIR@',
        'init',
    ],
)

# Try to find an OVMF.
ovmf = get_option('ovmf')
if ovmf == ''
    foreach path : [
        '/usr/share/qemu/ovmf-x86_64-4m.bin',
        '/usr/share/qemu/ovmf-x86_64.bin',
        '/usr/share/ovmf/OVMF.fd',
        '/usr/share/edk2/x64/OVMF.4m.fd',
        '/usr/share/edk2/x64/OVMF.fd',
        '/usr/share/edk2/ovmf/OVMF.fd',
    ]
        if ovmf == '' and fs.is_file(path)
            ovmf = path
        endif
    endforeach
endif
assert(ovmf != '', 'CONFIG_KTEST needs an OVMF image, set it with -Dovmf=')

qemu = [
    find_program('qemu-system-x86_64'),
    '-M', 'q35',
    '-m', '512M',
    '-smp', '4',
    '-accel', 'kvm',
    '-accel', 'tcg',
    '-cpu', 'max',
    '-nic', 'none',
    '-display', 'none',
    '-monitor', 'none',
    '-serial', 'stdio',
    '-no-reboot',
    '-bios', ovmf,
]

test_images = {
    'kernel': {
        'cmdline': 'ktest=1',
        'files': [],
    },
    'user': {
        'cmdline': 'init=/init initrd=/initramfs.tar',
        'files': [test_initramfs],
    },
}

# Build an ESP with the cmdline file and an initramfs.
foreach name, image : test_images
    conf = configure_file(
        input: 'cmdline.in',
        output: name + '.cmdline',
        configuration: {'CMDLINE': image['cmdline']},
    )

    esp = [
        zinnia_efi.full_path() + '=EFI/BOOT/BOOTX64.EFI',
        conf.full_path() + '=EFI/zinnia/cmdline',
    ]

    foreach file : image['files']
        esp += file.full_path() + '=' + fs.name(file.full_path())
    endforeach

    test(
        name,
        python,
        args: [files('run.py'), esp, '--', qemu],
        depends: [zinnia_efi, image['files']],
        protocol: 'tap',
        suite: 'qemu',
        timeout: 60,
        verbose: true,
    )
endforeach
