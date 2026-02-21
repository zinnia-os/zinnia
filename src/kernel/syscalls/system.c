#include <zinnia/status.h>
#include <kernel/alloc.h>
#include <kernel/console.h>
#include <kernel/syscalls.h>
#include <kernel/usercopy.h>

zn_status_t syscall_log(struct arch_context* ctx) {
    __user const char* msg = (__user const char*)ctx->ARCH_CTX_A0;
    size_t len = ctx->ARCH_CTX_A1;

    char* buf = mem_alloc(len, ALLOC_NOZERO);
    if (!buf) {
        return ZN_ERR_NO_MEMORY;
    }

    if (usercopy_read(buf, msg, len)) {
        console_write(buf, len);

        mem_free(buf);
        return ZN_OK;
    }

    mem_free(buf);
    return ZN_ERR_BAD_BUFFER;
}

zn_status_t syscall_page_size(struct arch_context* ctx) {
    __user size_t* out = (__user size_t*)ctx->ARCH_CTX_A0;

    size_t size = arch_mem_page_size();
    if (usercopy_write(out, &size, sizeof(size)))
        return ZN_ERR_BAD_BUFFER;

    return ZN_OK;
}
