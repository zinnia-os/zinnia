#include <zinnia/status.h>
#include <common/compiler.h>
#include <common/utils.h>
#include <kernel/sched.h>
#include <kernel/syscall_numbers.h>
#include <kernel/syscalls.h>
#include <kernel/types.h>

void syscall_dispatch(struct arch_context* ctx) {
    zn_status_t result = ZN_ERR_BAD_SYSCALL;

    switch (ctx->ARCH_CTX_NUM) {
    case ZN_SYSCALL_LOG:
        result = syscall_log(ctx);
        break;
    case ZN_SYSCALL_ARCHCTL:
        result = syscall_archctl(ctx);
        break;
    case ZN_SYSCALL_PAGE_SIZE:
        result = syscall_page_size(ctx);
        break;

    case ZN_SYSCALL_HANDLE_VALIDATE:
        result = syscall_handle_validate(ctx);
        break;
    case ZN_SYSCALL_HANDLE_DROP:
        result = syscall_handle_drop(ctx);
        break;
    case ZN_SYSCALL_HANDLE_CLONE:
        result = syscall_handle_clone(ctx);
        break;

    case ZN_SYSCALL_VAS_CREATE:
        result = syscall_vas_create(ctx);
        break;

    case ZN_SYSCALL_VMO_CREATE:
        result = syscall_vmo_create(ctx);
        break;
    case ZN_SYSCALL_VMO_CREATE_PHYS:
        result = syscall_vmo_create_phys(ctx);
        break;
    case ZN_SYSCALL_VMO_MAP:
        result = syscall_vmo_map(ctx);
        break;

    case ZN_SYSCALL_CHANNEL_CREATE:
        result = syscall_channel_create(ctx);
        break;
    case ZN_SYSCALL_CHANNEL_WAIT:
        result = syscall_channel_wait(ctx);
        break;
    case ZN_SYSCALL_CHANNEL_READ:
        result = syscall_channel_read(ctx);
        break;
    case ZN_SYSCALL_CHANNEL_WRITE:
        result = syscall_channel_write(ctx);
        break;

    case ZN_SYSCALL_THREAD_CREATE:
        result = syscall_thread_create(ctx);
        break;
    case ZN_SYSCALL_THREAD_EXIT:
        result = syscall_thread_exit(ctx);
        break;

    case ZN_SYSCALL_RANDOM_GET:
        result = syscall_random_get(ctx);
        break;
    case ZN_SYSCALL_RANDOM_ENTROPY:
        result = syscall_random_entropy(ctx);
        break;
    }

    ctx->ARCH_CTX_RET0 = result;
}
