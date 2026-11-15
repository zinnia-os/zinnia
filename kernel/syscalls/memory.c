// SPDX-License-Identifier: BSD-3-Clause

#include <arch/mem.h>
#include <fs/vfs.h>
#include <lib/atomic.h>
#include <mem/mman.h>
#include <mem/vma.h>
#include <mem/vmo.h>
#include <syscalls/private.h>
#include <stdckdint.h>
#include <zinnia/fcntl.h>

static struct vma* space() {
    return percpu_get()->sched.current->space;
}

constexpr enum prot_flags PROT_MASK = (PROT_READ | PROT_WRITE | PROT_EXEC);

static bool is_user_range(uintptr_t addr, size_t len) {
    uintptr_t last;
    if (len == 0 || ckd_add(&last, addr, len - 1))
        return false;
    return arch_mem_addr_is_user(addr) && arch_mem_addr_is_user(last);
}

static errno_t map_anon(struct vma* vma, uintptr_t addr, size_t len, uint32_t prot, uint32_t flags) {
    if (!(flags & MAP_SHARED))
        return vma_map(vma, nullptr, addr, len, prot, 0, 0);

    struct vmo* anon;
    errno_t status = vmo_new_anon(&anon);
    if (status)
        return status;
    status = vma_map(vma, anon, addr, len, prot, VMA_SHARED, 0);
    vmo_unref(anon);
    return status;
}

static errno_t get_map_file(int fd, uint32_t prot, uint32_t flags, struct vfs_file** out) {
    struct vfs_file* file = process_fd_get(self(), fd);
    if (!file)
        return EBADF;
    if (!(atomic_load(&file->flags, ATOMIC_RELAXED) & O_RDONLY) ||
        ((flags & MAP_SHARED) && (prot & PROT_WRITE) && !(atomic_load(&file->flags, ATOMIC_RELAXED) & O_WRONLY))) {
        vfs_file_unref(file);
        return EACCES;
    }
    *out = file;
    return 0;
}

struct syscall_ret sys_mmap(uintptr_t addr, size_t len, uint32_t prot, uint32_t flags, int fd, off_t offset) {
    struct syscall_ret ret = {};

    const size_t page_size = arch_mem_page_size();
    const uint32_t kind = flags & (MAP_SHARED | MAP_PRIVATE);

    if (len == 0 || (prot & ~PROT_MASK) || (kind != MAP_SHARED && kind != MAP_PRIVATE)) {
        ret.error = EINVAL;
        return ret;
    }
    if (offset < 0 || (uint64_t)offset % page_size != 0) {
        ret.error = EINVAL;
        return ret;
    }
    if ((flags & MAP_FIXED) && (addr % page_size != 0 || !is_user_range(addr, len))) {
        ret.error = EINVAL;
        return ret;
    }

    struct vfs_file* file = nullptr;
    if (!(flags & MAP_ANON)) {
        ret.error = get_map_file(fd, prot, flags, &file);
        if (ret.error)
            return ret;
    }

    struct vma* vma = space();
    mutex_lock(&vma->map_lock);

    if (!(flags & MAP_FIXED))
        ret.error = vma_find_free(vma, addr, len, &addr);

    if (!ret.error) {
        if (file)
            ret.error = vfs_file_mmap(file, vma, addr, len, prot, flags, offset, &addr);
        else
            ret.error = map_anon(vma, addr, len, prot, flags);
    }

    mutex_unlock(&vma->map_lock);
    if (file)
        vfs_file_unref(file);
    if (!ret.error)
        ret.value = addr;
    return ret;
}

struct syscall_ret sys_mprotect(uintptr_t addr, size_t len, uint32_t prot) {
    struct syscall_ret ret = {};

    if ((prot & ~PROT_MASK) || !is_user_range(addr, len)) {
        ret.error = EINVAL;
        return ret;
    }

    struct vma* vma = space();
    mutex_lock(&vma->map_lock);
    ret.error = vma_protect(vma, addr, len, prot);
    mutex_unlock(&vma->map_lock);
    return ret;
}

struct syscall_ret sys_munmap(uintptr_t addr, size_t len) {
    struct syscall_ret ret = {};

    if (!is_user_range(addr, len)) {
        ret.error = EINVAL;
        return ret;
    }

    struct vma* vma = space();
    mutex_lock(&vma->map_lock);
    ret.error = vma_unmap(vma, addr, len);
    mutex_unlock(&vma->map_lock);
    return ret;
}

struct syscall_ret sys_madvise(uintptr_t addr, size_t len, int advice) {
    struct syscall_ret ret = {};

    if (addr % arch_mem_page_size() != 0)
        ret.error = EINVAL;
    return ret;
}

struct syscall_ret sys_msync(uintptr_t addr, size_t len, int flags) {
    struct syscall_ret ret = {};

    if (flags & ~(MS_ASYNC | MS_INVALIDATE | MS_SYNC)) {
        ret.error = EINVAL;
        return ret;
    }
    if ((flags & MS_ASYNC) && (flags & MS_SYNC)) {
        ret.error = EINVAL;
        return ret;
    }
    if (addr % arch_mem_page_size() != 0 || !is_user_range(addr, len)) {
        ret.error = EINVAL;
        return ret;
    }
    if (flags & MS_ASYNC) {
        if (!vma_range_mapped(space(), addr, len))
            ret.error = ENOMEM;
        return ret;
    }
    ret.error = vma_sync(space(), addr, len);
    return ret;
}
