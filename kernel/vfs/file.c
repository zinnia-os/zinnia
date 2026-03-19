#include <kernel/iovec.h>
#include <kernel/vfs.h>

errno_t file_readv(
    struct file* file,
    size_t nbyte,
    off_t offset,
    iovec_iter_t* iter,
    struct identity* identity,
    ssize_t* out_read
) {
    if (!file)
        return EBADF;

    if (file->ops->read)
        return file->ops->read(file, nbyte, offset, iter, identity, out_read);

    // Fall back to reading from the page cache.
    // TODO

    return ENOSYS;
}

errno_t file_read(struct file* file, void* buf, size_t nbyte, off_t offset, struct identity* ident, ssize_t* out_read) {
    iovec_t iovec = {
        .base = buf,
        .len = nbyte,
    };
    iovec_iter_t iter = iovec_iter_new(&iovec, 1);
    return file_readv(file, nbyte, offset, &iter, ident, out_read);
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
