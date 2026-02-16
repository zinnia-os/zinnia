# The virtual dynamic shared object (vDSO) contains the syscall interface and
# some freestanding helper functions.

vdso_lds = meson.current_source_dir() / 'vdso.ld'

vdso_lib = shared_library(
    'vdso',
    'archctl.c',
    'channel.c',
    'handle.c',
    'mem.c',
    'namespace.c',
    'random.c',
    'rights.c',
    'status.c',
    'system.c',
    'thread.c',
    klibc_sources,
    c_args: [
        '-ffreestanding',
        '-nostdinc',
    ],
    link_args: [
        '-ffreestanding',
        '-nostdlib',
        '-Wl,-nostdlib',
        '-T' + vdso_lds,
    ],
    include_directories: [
        common_includes,
        kernel_includes,
    ],
    link_depends: vdso_lds,
)

vdso_stripped = custom_target(
    'vdso_stripped.so',
    input: vdso_lib,
    output: 'vdso_stripped.so',
    command: [strip, '--strip-unneeded', '@INPUT@', '-o', '@OUTPUT@'],
)

vdso = custom_target(
    'vdso_blob.o',
    input: vdso_stripped,
    output: 'vdso_blob.o',
    command: [
        objcopy,
        '--input-target=binary',
        '--output-target=elf64-x86-64',
        '--rename-section', '.data=.vdso,alloc,load,readonly,data,contents',
        '@INPUT@',
        '@OUTPUT@',
    ],
)
