#include <zinnia/archctl.h>
#include <kernel/syscalls.h>

zn_status_t arch_archctl(zn_archctl_t op, uintptr_t value);

zn_status_t syscall_archctl(zn_archctl_t op, __user void* value) {
    return arch_archctl(op, (uintptr_t)value);
}
