#pragma once

#include <menix/handle.h>
#include <menix/status.h>
#include <uapi/time.h>
#include <uapi/types.h>
#include <stddef.h>
#include <stdint.h>

struct file;
struct inode;
struct path;

// struct file_ops {
//     menix_status_t (*open)(struct file* self, uint32_t flags);
//     menix_status_t (*close)(struct file* self);
//     menix_status_t (*read)(struct file* self, struct iovec_iter* iter, ssize_t* out_read);
//     menix_status_t (*write)(struct file* self, struct iovec_iter* iter, ssize_t* out_written);
//     menix_status_t (*devctl)(struct file* self, uint32_t dcmd, void* data, size_t num, int* out_info);
//     menix_status_t (*poll)(struct file* self, int16_t mask, int16_t* out_mask); // TODO: This is not correct.
//     menix_status_t (*mmap)(
//         struct file* self,
//         menix_handle_t* space,
//         void* addr,
//         size_t len,
//         int prot,
//         int flags,
//         off_t offset
//     );
// };

struct file {
    const struct file_ops* ops;
    struct inode* inode;
    uint32_t mode;
    uint32_t flags;
};

struct file* file_alloc();
struct file* file_from_fd(int fd);

struct inode_ops {
    union {
        struct {
            menix_status_t (*lookup)(struct inode* inode, struct path* path);
        } dir;
        struct {
            menix_status_t (*truncate)(struct inode* inode);
        } reg;
    };
};

struct inode_attr {
    struct timespec atime, mtime, ctime;
    uid_t uid;
    gid_t gid;
};

struct inode {
    const struct inode_ops* ops;
    struct inode_attr attr;
};

struct entry {
    const char* name;
    struct inode* inode;
    uint8_t state;
};

struct path {
    struct entry* entry;
    struct mount* mount;
};

struct mount {
    uint32_t flags;
    struct entry root;
    struct path mount_point;
};

struct file_system_ops {};

struct file_system {
    const char* name;
    struct file_system_ops ops;
};
