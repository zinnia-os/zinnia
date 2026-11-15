kernel_sources += files(
    'alloc.c',
    'amap.c',
    'dma.c',
    'freelist.c',
    'init.c',
    'page.c',
    'stack.c',
    'usercopy.c',
    'vma.c',
    'vmo.c',
)

if kconfig.get('CONFIG_HAVE_MMU', 'n') == 'y'
    kernel_sources += files(
        'pmap.c',
    )
endif

if kconfig.get('CONFIG_HAVE_TLB_SHOOTDOWN_IPI', 'n') == 'y'
    kernel_sources += files('shootdown.c')
endif
