#include <zinnia/status.h>
#include <common/compiler.h>
#include <common/syscall_numbers.h>
#include <kernel/syscalls.h>
#include <kernel/types.h>

zn_status_t syscall_dispatch(
    reg_t num,
    reg_t a0,
    reg_t a1,
    reg_t a2,
    reg_t a3,
    reg_t a4,
    reg_t a5,
    reg_t a6,
    reg_t a7
) {
    switch (num) {
    case ZN_SYSCALL_LOG:
        return syscall_log((__user const char*)a0, a1);
    case ZN_SYSCALL_ARCHCTL:
        return syscall_archctl(a0, (__user void*)a1);
    default:
        return ZN_ERR_BAD_SYSCALL;
    }
}
