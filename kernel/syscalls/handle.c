#include <menix/handle.h>
#include <menix/status.h>
#include <kernel/percpu.h>
#include <kernel/syscalls.h>

menix_status_t menix_handle_clone(menix_handle_t handle, menix_rights_t cloned_rights, menix_handle_t* cloned) {
    struct task* current = percpu_get()->sched.current;
    current->process;
    return MENIX_ERR_UNSUPPORTED;
}

menix_status_t menix_handle_drop(menix_handle_t handle) {
    struct task* current = percpu_get()->sched.current;

    return MENIX_ERR_UNSUPPORTED;
}

menix_status_t menix_handle_validate(menix_handle_t handle) {
    struct task* current = percpu_get()->sched.current;

    return MENIX_ERR_UNSUPPORTED;
}
