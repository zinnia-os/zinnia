#include <kernel/iovec.h>
#include <kernel/vfs.h>

errno_t file_readv(struct file* file, iovec_iter_t* iter, size_t nbyte, off_t offset, ssize_t* out_read) {
    return ENOSYS;
}

errno_t file_read(struct file* file, void* buf, size_t nbyte, off_t offset, ssize_t* out_read) {
    iovec_t iovec = {
        .base = buf,
        .len = nbyte,
    };
    iovec_iter_t iter;
    iovec_iter_init(&iter, &iovec, 1);
    return file_readv(file, &iter, nbyte, offset, out_read);
}

errno_t file_mmap(
    struct file* file,
    struct vm_space* space,
    uintptr_t addr,
    size_t len,
    enum prot_flags prot,
    enum map_flags flags,
    off_t offset
) {
    return ENOSYS;
}
