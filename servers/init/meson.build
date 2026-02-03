executable(
    'init',
    'main.c',
    # The init process is linked statically because the dynamic loader can't be
    # read before vfsd is up.
    c_args: [common_c_args, '-static'],
    include_directories: common_includes,
    link_with: libmenix,
    install: true,
)
