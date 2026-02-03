#include <menix/handle.h>
#include <menix/syscall_numbers.h>
#include "syscall_stubs.h"

menix_status_t menix_handle_validate(menix_handle_t handle) {
    return syscall1(MENIX_SYSCALL_HANDLE_VALIDATE, (arg_t)handle);
}

menix_status_t menix_handle_drop(menix_handle_t handle) {
    return syscall1(MENIX_SYSCALL_HANDLE_DROP, (arg_t)handle);
}

menix_status_t menix_handle_clone(menix_handle_t object, menix_rights_t cloned_rights, menix_handle_t* cloned) {
    return syscall3(MENIX_SYSCALL_HANDLE_CLONE, (arg_t)object, (arg_t)cloned_rights, (arg_t)cloned);
}
