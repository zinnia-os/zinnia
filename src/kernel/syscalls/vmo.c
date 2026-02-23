#include <zinnia/handle.h>
#include <zinnia/status.h>
#include <common/utils.h>
#include <kernel/alloc.h>
#include <kernel/namespace.h>
#include <kernel/percpu.h>
#include <kernel/print.h>
#include <kernel/sched.h>
#include <kernel/syscalls.h>
#include <kernel/usercopy.h>
#include <kernel/vas.h>
#include <kernel/vmo.h>
#include <stdatomic.h>

zn_status_t syscall_vmo_create(struct arch_context* ctx) {
    size_t length = ctx->ARCH_CTX_A0;
    __user zn_handle_t* out = (__user zn_handle_t*)ctx->ARCH_CTX_A1;

    // Allocate new VMO.
    struct paged_vmo* vmo;
    zn_status_t status = vmo_new_phys(&vmo);
    if (status != ZN_OK)
        return status;

    struct namespace_desc desc = {
        .type = NAMESPACE_DESC_VMO,
        .vmo = &vmo->object,
    };

    struct task* current = percpu_get()->sched.current;
    zn_handle_t handle;
    status = namespace_add_desc(current->namespace, desc, &handle);
    if (status != ZN_OK)
        return status;

    if (!usercopy_write(out, &handle, sizeof(handle)))
        return ZN_ERR_BAD_BUFFER;

    return ZN_OK;
}

zn_status_t syscall_vmo_create_phys(struct arch_context* ctx) {
    return ZN_ERR_UNSUPPORTED;
}

zn_status_t syscall_vmo_map(struct arch_context* ctx) {
    zn_handle_t vmo_handle = ctx->ARCH_CTX_A0;
    zn_handle_t vas_handle = ctx->ARCH_CTX_A1;
    uintptr_t vmo_offset = ctx->ARCH_CTX_A2;
    __user uintptr_t* addr = (__user uintptr_t*)ctx->ARCH_CTX_A3;
    size_t bytes = ctx->ARCH_CTX_A4;
    enum zn_vm_flags flags = ctx->ARCH_CTX_A5;

    struct task* current = percpu_get()->sched.current;

    // Get VAS.
    struct vas* vas;
    if (vas_handle == ZN_HANDLE_THIS_VAS) {
        vas = current->space;
    } else {
        struct namespace_desc vas_desc;
        zn_status_t status = namespace_get(current->namespace, vmo_handle, &vas_desc);
        if (status != ZN_OK)
            return status;
        if (vas_desc.type != NAMESPACE_DESC_VAS)
            return ZN_ERR_BAD_HANDLE;
        vas = vas_desc.vas;
    }

    // Get VMO.
    struct namespace_desc vmo_desc;
    zn_status_t status = namespace_get(current->namespace, vmo_handle, &vmo_desc);
    if (status != ZN_OK)
        return status;
    if (vmo_desc.type != NAMESPACE_DESC_VMO)
        return ZN_ERR_BAD_HANDLE;
    struct vmo* vmo = vmo_desc.vmo;

    // Determine mapping address.
    uintptr_t target_addr;
    if (!usercopy_read(&target_addr, addr, sizeof(target_addr)))
        return ZN_ERR_BAD_BUFFER;

    // TODO: Use actual allocator for user task.
    if (target_addr == 0) {
        static uintptr_t map_offset = 0xA000'0000;
        target_addr = atomic_fetch_add(&map_offset, ALIGN_UP(bytes, arch_mem_page_size()));
    }

    if (!usercopy_write(addr, &target_addr, sizeof(target_addr)))
        return ZN_ERR_BAD_BUFFER;

    vas_map_vmo(vas, vmo, target_addr, bytes, flags, vmo_offset);

    return ZN_OK;
}
