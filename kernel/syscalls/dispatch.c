#include <menix/status.h>
#include <menix/syscall_numbers.h>
#include <kernel/compiler.h>
#include <kernel/syscalls.h>
#include <kernel/types.h>

menix_status_t syscall_dispatch(reg_t num, reg_t a0, reg_t a1, reg_t a2, reg_t a3, reg_t a4, reg_t a5, reg_t a6) {
    switch (num) {
    case MENIX_SYSCALL_PANIC:
        syscall_panic((menix_status_t)a0);
    case MENIX_SYSCALL_LOG:
        return syscall_log((__user const char*)a0, a1);
    case MENIX_SYSCALL_ARCHCTL:
        return syscall_archctl(a0, a1);
    }

    return MENIX_ERR_BAD_SYSCALL;
}
