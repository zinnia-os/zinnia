#pragma once

#include <kernel/hashmap.h>
#include <kernel/identity.h>
#include <kernel/iovec.h>
#include <kernel/vm_space.h>
#include <uapi/errno.h>
#include <uapi/fcntl.h>
#include <uapi/statvfs.h>
#include <uapi/time.h>
#include <uapi/types.h>
#include <stddef.h>
#include <stdint.h>

struct inode;
struct path;
struct file;
struct file_system;
struct super_block;

struct file_ops {
    errno_t (*open)(struct file* file, enum open_flags flags);
    errno_t (*close)(struct file* file);
    errno_t (*read)(
        struct file* file,
        size_t nbyte,
        off_t offset,
        iovec_iter_t* iter,
        struct identity* identity,
        ssize_t* out_read
    );
    errno_t (*write)(
        struct file* file,
        size_t nbyte,
        off_t offset,
        iovec_iter_t* iter,
        struct identity* identity,
        ssize_t* out_written
    );
    errno_t (*devctl)(struct file* file, uint32_t cmd, void* data, size_t num, int* out_info);
    errno_t (*poll)(struct file* file, int16_t mask, int16_t* out_mask); // TODO: This is probably not correct.
    errno_t (*mmap)(
        struct file* file,
        struct vm_space* space,
        uintptr_t addr,
        size_t len,
        enum prot_flags prot,
        enum map_flags flags,
        off_t offset
    );
};

struct file {
    size_t refcount;
    const struct file_ops* ops;
    void* priv; // Usable by whoever provides `ops`.
    struct inode* inode;
    uint32_t mode;
    uint32_t flags;
};

errno_t file_read(struct file* file, void* buf, size_t nbyte, off_t offset, struct identity* ident, ssize_t* out_read);

errno_t file_readv(
    struct file* file,
    size_t nbyte,
    off_t offset,
    iovec_iter_t* iter,
    struct identity* identity,
    ssize_t* out_read
);

errno_t file_mmap(
    struct file* file,
    struct vm_space* space,
    uintptr_t addr,
    size_t len,
    enum prot_flags prot,
    enum map_flags flags,
    off_t offset
);

struct file_description {
    struct file* file;
    bool close_on_exec;
};

struct fd_table {
    HASHMAP(int, struct file_description) inner;
};

struct file_description fd_get_desc(struct fd_table* table, int fd);

// Opens a file
int fd_open(struct fd_table* table, int base, struct file* file);

struct inode_ops {
    union {
        struct {
            errno_t (*lookup)(struct inode* inode, struct path* path);
        } dir;
        struct {
            errno_t (*truncate)(struct inode* inode);
        } reg;
    };
};

struct inode_attr {
    struct timespec atime, mtime, ctime;
    uid_t uid;
    gid_t gid;
};

struct inode {
    size_t refcount;
    const struct inode_ops* ops;
    struct inode_attr attr;
};

struct entry {
    size_t refcount;
    const char* name;
    struct inode* inode;
    uint8_t state;
};

struct path {
    struct entry* entry;
    struct mount* mount;
};

extern struct path vfs_root;

struct mount {
    size_t refcount;
    struct entry* root;
    struct path mount_point;
    uint32_t flags;
};

struct file_system_ops {
    errno_t (*mount)(struct file_system* fs);
};

struct file_system {
    const char* name;
    const struct file_system_ops* ops;
};

struct super_block_ops {
    errno_t (*sync)(struct super_block* sb);
    errno_t (*statvfs)(struct super_block* sb, struct statvfs* out_buf);
};

struct super_block {
    const struct super_block_ops* ops;
};

void vfs_init();
