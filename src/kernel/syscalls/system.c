#include <zinnia/status.h>
#include <kernel/alloc.h>
#include <kernel/console.h>
#include <kernel/syscalls.h>
#include <kernel/usercopy.h>

zn_status_t syscall_log(__user const char* msg, size_t len) {
    char* buf = mem_alloc(len, ALLOC_NOZERO);
    if (!buf)
        return ZN_ERR_NO_MEMORY;

    usercopy_read(buf, msg, len);
    console_write(buf, len);

    mem_free(buf);
    return ZN_OK;
}
