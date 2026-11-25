// SPDX-License-Identifier: BSD-3-Clause

#pragma once

#include <fs/ident.h>
#include <fs/poll.h>
#include <lib/atomic.h>
#include <lib/epoch.h>
#include <lib/errno.h>
#include <lib/initgraph.h>
#include <lib/iovec.h>
#include <lib/mutex.h>
#include <lib/refcount.h>
#include <lib/spin.h>
#include <lib/tailq.h>
#include <lib/time.h>
#include <lib/types.h>
#include <lib/vec.h>
#include <mem/mman.h>
#include <mem/vma.h>
#include <mem/vmo.h>
#include <stddef.h>
#include <stdint.h>
#include <zinnia/dirent.h>
#include <zinnia/fcntl.h>
#include <zinnia/limits.h>
#include <zinnia/mount.h>
#include <zinnia/stat.h>
#include <zinnia/statvfs.h>
#include <zinnia/time.h>

struct vfs_mount;
struct vfs_entry;
struct vfs_path;
struct vfs_node;
struct vfs_file;
struct vfs_super_block;
struct vfs_filesystem;
struct vfs_node_ops;
struct vfs_file_ops;
struct pipe;

enum open_flags : uint32_t {
    O_RDONLY = __O_RDONLY,
    O_WRONLY = __O_WRONLY,
    O_RDWR = __O_RDWR,
    O_CREAT = __O_CREAT,
    O_EXCL = __O_EXCL,
    O_NOCTTY = __O_NOCTTY,
    O_TRUNC = __O_TRUNC,
    O_APPEND = __O_APPEND,
    O_NONBLOCK = __O_NONBLOCK,
    O_DSYNC = __O_DSYNC,
    O_ASYNC = __O_ASYNC,
    O_DIRECT = __O_DIRECT,
    O_LARGEFILE = __O_LARGEFILE,
    O_DIRECTORY = __O_DIRECTORY,
    O_NOFOLLOW = __O_NOFOLLOW,
    O_NOATIME = __O_NOATIME,
    O_CLOEXEC = __O_CLOEXEC,
    O_PATH = __O_PATH,
    O_TMPFILE = __O_TMPFILE,
    O_SYNC = __O_SYNC,
    O_RSYNC = __O_RSYNC,
    O_EXEC = __O_EXEC,
    O_SEARCH = __O_SEARCH,
    O_ACCMODE = __O_ACCMODE,
};

enum access_flags : uint32_t {
    F_OK = __F_OK,
    X_OK = __X_OK,
    W_OK = __W_OK,
    R_OK = __R_OK,
};

enum mount_flags : uint32_t {
    MNT_RDONLY = __MNT_RDONLY,
    MNT_NOSUID = __MNT_NOSUID,
    MNT_NOEXEC = __MNT_NOEXEC,
    MNT_RELATIME = __MNT_RELATIME,
    MNT_NOATIME = __MNT_NOATIME,
    MNT_REMOUNT = __MNT_REMOUNT,
    MNT_FORCE = __MNT_FORCE,
};

enum vfs_lookup_flags : uint32_t {
    VFS_LOOKUP_FOLLOW_SYMLINKS = 1 << 0,
    VFS_LOOKUP_MUST_EXIST = 1 << 1,
    VFS_LOOKUP_MUST_NOT_EXIST = 1 << 2,
    VFS_LOOKUP_REAL_ID = 1 << 3,
};

enum vfs_node_type {
    VNODE_REGULAR,
    VNODE_DIRECTORY,
    VNODE_SYMLINK,
    VNODE_FIFO,
    VNODE_BLOCKDEV,
    VNODE_CHARDEV,
    VNODE_SOCKET,
    VNODE_ANONYMOUS,
};

enum vfs_entry_state {
    VFS_ENTRY_NOT_CACHED,  // Entry hasn't been looked up yet.
    VFS_ENTRY_PRESENT,     // Entry is positive and contains a link to the inode.
    VFS_ENTRY_NOT_PRESENT, // Entry is negative.
};

// A namecache entry.
struct vfs_entry {
    struct refcount rc;
    // The name of this entry.
    char* name;
    size_t name_len;
    mutex_t lock;

    enum vfs_entry_state state;
    // The inode this entry points to when present. Holds a reference.
    struct vfs_node* inode;
    // The parent of this entry, strong reference. `nullptr` for file system roots.
    struct vfs_entry* parent;

    // All cached children of this entry. Each child in this list holds a reference.
    tailq_head(struct vfs_entry) children;
    tailq_link(struct vfs_entry) child_link;

    // A list of mounts on this entry.
    vec(struct vfs_mount*) mounts;
};

struct vfs_path {
    struct vfs_mount* mount;
    struct vfs_entry* entry;
};

struct vfs_path vfs_path_clone(struct vfs_path path);
void vfs_path_drop(struct vfs_path path);

// Allocates a new, uncached entry with the given `name`. `parent` gets referenced if non-null.
errno_t vfs_entry_new(const char* name, size_t name_len, struct vfs_entry* parent, struct vfs_entry** out);

// Makes `entry` positive and points it to `node`. Takes a new reference to `node`.
void vfs_entry_set_inode(struct vfs_entry* entry, struct vfs_node* node);

// Returns a new reference to the inode of `entry`, or `nullptr` if the entry is negative.
struct vfs_node* vfs_entry_get_inode(struct vfs_entry* entry);

// Makes `entry` negative and drops its reference to the inode.
void vfs_entry_set_negative(struct vfs_entry* entry);

void vfs_entry_replace_children(struct vfs_entry* dst, struct vfs_entry* src);
void vfs_entry_prune(struct vfs_entry* entry);

// A mount point.
struct vfs_mount {
    struct refcount rc;
    enum mount_flags flags;
    struct vfs_entry* root;
    struct vfs_path mount_point;
    bool has_mount_point;
};

void vfs_mount_ref(struct vfs_mount* mount);
void vfs_mount_unref(struct vfs_mount* mount);

// Creates a unique instance of a mount point.
errno_t vfs_mount(const char* fs_name, enum mount_flags flags, void* arg, struct vfs_mount** out);
errno_t vfs_unmount(struct vfs_mount* mount);

// Allocates a new mount with `root` as its root entry. Takes a new reference to `root`.
errno_t vfs_mount_new(enum mount_flags flags, struct vfs_entry* root, struct vfs_mount** out);

// Attaches `mount` on top of the entry referenced by `at`.
errno_t vfs_path_mount(struct vfs_path at, struct vfs_mount* mount);
errno_t vfs_path_unmount(struct vfs_path path);

struct vfs_super_block_ops {
    errno_t (*sync)(struct vfs_super_block* sb);
    errno_t (*statvfs)(struct vfs_super_block* sb, struct __statvfs* out);
    void (*destroy)(struct vfs_super_block* sb);
};

struct vfs_super_block {
    struct refcount rc;
    const struct vfs_super_block_ops* ops;
    enum mount_flags flags;
};

// TODO: Maybe not like this
struct vfs_mknod_target {
    enum vfs_node_type type;
    const struct vfs_node_ops* ops;
    void* priv;
    dev_t rdev;
};

struct vfs_node_ops {
    // Synchronizes the node metadata back to the underlying file system.
    errno_t (*sync)(struct vfs_node* node);
    // Destroys private state and drops references.
    void (*destroy)(struct vfs_node* node);
    // Operations that can be performed on an open file of this node.
    const struct vfs_file_ops* file_ops;

    union {
        // VNODE_REGULAR
        struct {
            // Truncates the file down to `new_length` bytes.
            errno_t (*truncate)(struct vfs_node* node, uint64_t new_length);
            // Returns a new reference to a memory object holding the file contents.
            errno_t (*get_vmo)(struct vfs_node* node, struct vmo** out);
        } regular;

        // VNODE_SYMLINK
        struct {
            // Reads the value of the symlink into `buf`.
            errno_t (*read_link)(struct vfs_node* node, iovec_iter_t* iter, size_t* out_read);
        } symlink;

        // VNODE_CHARDEV or VNODE_BLOCKDEV
        struct {
            errno_t (*open)(struct vfs_node* node, struct vfs_file* file);
        } device;

        // VNODE_DIR
        struct {
            errno_t (*lookup)(struct vfs_node* node, struct vfs_path* child);
            errno_t (*open)(
                struct vfs_node* node,
                struct vfs_path* path,
                enum open_flags flags,
                const struct ident* identity,
                struct vfs_file** out_file
            );
            errno_t (*create)(
                struct vfs_node* node,
                struct vfs_entry* entry,
                mode_t mode,
                const struct ident* identity
            );
            errno_t (*mkdir)(struct vfs_node* node, struct vfs_path* path, mode_t mode, const struct ident* identity);
            errno_t (*symlink)(
                struct vfs_node* node,
                struct vfs_path* path,
                const char* target,
                size_t target_len,
                const struct ident* identity
            );
            errno_t (*link)(
                struct vfs_node* node,
                struct vfs_path* path,
                struct vfs_node* target,
                const struct ident* identity
            );
            errno_t (*unlink)(struct vfs_node* node, struct vfs_path* path, const struct ident* identity);
            errno_t (*rmdir)(struct vfs_node* node, struct vfs_path* path, const struct ident* identity);
            errno_t (*rename)(
                struct vfs_node* node,
                struct vfs_path* path,
                struct vfs_node* target,
                struct vfs_path* target_path,
                const struct ident* identity
            );
            errno_t (*mknod)(
                struct vfs_node* node,
                struct vfs_path* path,
                mode_t mode,
                struct vfs_mknod_target* target,
                const struct ident* identity
            );
            errno_t (*get_dir_entries)(
                struct vfs_node* node,
                struct vfs_entry* entry,
                off_t offset,
                struct __dirent* buffer,
                size_t buffer_len,
                const struct ident* identity,
                size_t* out_count
            );
        } dir;
    };
};

struct vfs_node {
    struct refcount rc;
    enum vfs_node_type type;

    const struct vfs_node_ops* ops;
    const struct vfs_file_ops* file_ops;
    struct vfs_super_block* sb;

    mutex_t lock;
    void* priv;
    struct pipe* pipe;

    ino_t id;
    dev_t rdev;
    size_t size;
    blkcnt_t blocks;
    nlink_t nlink;
    mode_t mode;
    uid_t uid;
    gid_t gid;
    timespec_t atime;
    timespec_t mtime;
    timespec_t ctime;
};

struct vfs_file_ops {
    errno_t (*read)(struct vfs_file* file, iovec_iter_t* buffer, uint64_t offset, ssize_t* out_read);
    errno_t (*write)(struct vfs_file* file, iovec_iter_t* buffer, uint64_t offset, ssize_t* out_written);
    errno_t (*ioctl)(struct vfs_file* file, uintptr_t request, uintptr_t arg, uintptr_t* out_result);
    errno_t (*sync)(struct vfs_file* file, bool data_only);
    errno_t (*mmap)(
        struct vfs_file* file,
        struct vma* vma,
        uintptr_t addr,
        size_t len,
        enum prot_flags prot,
        enum map_flags flags,
        off_t offset,
        uintptr_t* out_addr
    );
    enum poll_flags (*poll)(struct vfs_file* file, enum poll_flags events, struct poll_table* table);
    void (*release)(struct vfs_file* file);
};

enum vfs_file_position {
    VFS_POSITION_STREAM,   // The file has no meaningful position.
    VFS_POSITION_POSITION, // The file has a position, but accesses do not serialize the whole operation.
    VFS_POSITION_ATOMIC,   // Accesses through the shared file position are serialized.
};

enum vfs_seek_anchor {
    VFS_SEEK_START,
    VFS_SEEK_CURRENT,
    VFS_SEEK_END,
};

// The kernel representation of an open file.
struct vfs_file {
    struct refcount rc;
    const struct vfs_file_ops* ops;
    struct vfs_node* vnode;

    // Absolute path resolved during open.
    char* absolute_path;
    _Atomic(enum open_flags) flags;

    enum vfs_file_position position_kind;
    off_t position;
    mutex_t lock;

    struct vfs_path path;
    bool has_path;

    // Driver data.
    void* priv;

    struct epoch_head epoch;
};

static inline bool vfs_file_nonblocking(struct vfs_file* file) {
    return atomic_load(&file->flags, ATOMIC_RELAXED) & O_NONBLOCK;
}

// A file system driver.
struct vfs_filesystem {
    const char* name;
    errno_t (*mount)(const struct vfs_filesystem* fs, enum mount_flags flags, void* arg, struct vfs_mount** out);
};

// Allocates a new inode in `sb` with `type`, `ops` and permission bits `mode`.
// The inode takes a reference to `sb` if it's not `nullptr`.
errno_t vfs_node_new(
    struct vfs_super_block* sb,
    enum vfs_node_type type,
    const struct vfs_node_ops* ops,
    mode_t mode,
    struct vfs_node** out
);

void vfs_node_ref(struct vfs_node* node);
void vfs_node_unref(struct vfs_node* node);
void vfs_entry_ref(struct vfs_entry* entry);
void vfs_entry_unref(struct vfs_entry* entry);
void vfs_file_ref(struct vfs_file* file);
void vfs_file_unref(struct vfs_file* file);
void vfs_super_block_ref(struct vfs_super_block* sb);
void vfs_super_block_unref(struct vfs_super_block* sb);

// Gets the kernel path representation of the absolute root.
struct vfs_path vfs_get_root();

errno_t vfs_register_filesystem(const struct vfs_filesystem* fs);

errno_t vfs_sync_all();

// Resolves a `path` to a VFS path, relative to `root` and `cwd`.
// Returns the result in `out` if the lookup with the given flags was successful.
errno_t vfs_path_lookup(
    struct vfs_path root,
    struct vfs_path cwd,
    const char* path,
    const struct ident* identity,
    enum vfs_lookup_flags flags,
    struct vfs_path* out
);

errno_t vfs_path_lookup_parent(struct vfs_path path, struct vfs_path* out);

errno_t vfs_path_lookup_child(
    struct vfs_path path,
    struct vfs_path* root,
    const char* name,
    size_t name_len,
    struct vfs_path* out
);

// Builds the absolute path of `path` from `root` and stores the result in `buf`.
// Returns the amount of actually written bytes in `out_len`.
errno_t vfs_path_absolute(struct vfs_path path, struct vfs_path* root, char* buf, size_t len, size_t* out_len);

// Creates a new directory at `path`, relative to `root` and `cwd` with `mode`.
errno_t vfs_mkdir(
    struct vfs_path root,
    struct vfs_path cwd,
    const char* path,
    mode_t mode,
    const struct ident* identity
);

// Creates a new symbolic link at `path`, relative to `root` and `cwd`, pointing to `target`.
errno_t vfs_symlink(
    struct vfs_path root,
    struct vfs_path cwd,
    const char* path,
    const char* target,
    const struct ident* identity
);

// Unlinks a file at `path`, relative to `root` and `cwd`.
errno_t vfs_unlink(struct vfs_path root, struct vfs_path cwd, const char* path, const struct ident* identity);

// Removes a directory at `path`, relative to `root` and `cwd`.
errno_t vfs_rmdir(struct vfs_path root, struct vfs_path cwd, const char* path, const struct ident* identity);

errno_t vfs_mknod(
    struct vfs_path root,
    struct vfs_path cwd,
    const char* path,
    mode_t mode,
    struct vfs_mknod_target* target,
    const struct ident* identity,
    struct vfs_node** out
);

errno_t vfs_rename(
    struct vfs_path root,
    struct vfs_path old_cwd,
    const char* old_path,
    struct vfs_path new_cwd,
    const char* new_path,
    const struct ident* identity
);

errno_t vfs_link(
    struct vfs_path root,
    struct vfs_path old_cwd,
    const char* old_path,
    struct vfs_path new_cwd,
    const char* new_path,
    bool follow_symlinks,
    const struct ident* identity
);

void vfs_stat(struct vfs_node* node, struct __stat* out);
errno_t vfs_statvfs(struct vfs_node* node, struct __statvfs* out);
errno_t vfs_chmod(struct vfs_node* node, mode_t mode, const struct ident* identity);
errno_t vfs_chown(struct vfs_node* node, uid_t uid, gid_t gid, const struct ident* identity);
errno_t vfs_utimens(struct vfs_node* node, const timespec_t* times, const struct ident* identity);
errno_t vfs_node_check_exec(struct vfs_node* node, const struct ident* identity);
errno_t vfs_truncate(struct vfs_node* node, uint64_t length);
errno_t vfs_read_link(struct vfs_node* node, iovec_iter_t* iter, size_t* out_read);

// Checks if an access to a `node` with `access` using `identity` is allowed.
// Returns 0 if the access is allowed.
errno_t vfs_node_try_access(
    struct vfs_node* node,
    const struct ident* identity,
    enum access_flags access,
    bool use_real
);

// Opens a node at `path`, relative to `root` and `cwd`.
// Returns the opened node as a file in `out`.
errno_t vfs_file_open(
    struct vfs_path root,
    struct vfs_path cwd,
    const char* path,
    enum open_flags flags,
    mode_t mode,
    const struct ident* identity,
    struct vfs_file** out
);

// Creates a new open file backed by `node`, which must not be `nullptr`.
// If `path` is not `nullptr`, the file remembers it and takes a reference to it.
errno_t vfs_file_new(struct vfs_node* node, const struct vfs_path* path, enum open_flags flags, struct vfs_file** out);

// Changes the file's position mode to a stream.
void vfs_stream_open(struct vfs_file* file);

// Gets a new reference to a memory object holding the contents of `file`.
errno_t vfs_file_get_object(struct vfs_file* file, struct vmo** out);

// Reads from a `file` into a `buffer` from the current offset.
// Returns the actual bytes read in `out_read`.
errno_t vfs_file_read(struct vfs_file* file, iovec_iter_t* buffer, ssize_t* out_read);

// Same as `vfs_file_read`, but from a given offset.
errno_t vfs_file_pread(struct vfs_file* file, iovec_iter_t* buffer, uint64_t offset, ssize_t* out_read);

// Wites to a `file` from a `buffer` from the current offset.
// Returns the actual bytes written in `out_written`.
errno_t vfs_file_write(struct vfs_file* file, iovec_iter_t* buffer, ssize_t* out_written);

// Same as `vfs_file_write`, but from a given offset.
errno_t vfs_file_pwrite(struct vfs_file* file, iovec_iter_t* buffer, uint64_t offset, ssize_t* out_written);

// Performs a generic `request` on a `file`.
// Returns the result in `out_result`.
errno_t vfs_file_ioctl(struct vfs_file* file, uintptr_t request, uintptr_t arg, uintptr_t* out_result);

errno_t vfs_file_sync(struct vfs_file* file, bool data_only);

errno_t vfs_file_seek(struct vfs_file* file, enum vfs_seek_anchor anchor, off_t offset, off_t* out_position);
off_t vfs_file_tell(struct vfs_file* file);

// Maps `len` bytes of a `file` from `offset` into `vma` at `addr`.
// Returns the start of the mapped region in `out_addr`.
errno_t vfs_file_mmap(
    struct vfs_file* file,
    struct vma* vma,
    uintptr_t addr,
    size_t len,
    enum prot_flags prot,
    enum map_flags flags,
    off_t offset,
    uintptr_t* out_addr
);

enum poll_flags vfs_poll(struct vfs_file* file, enum poll_flags events, struct poll_table* table);

// Directory listing.
errno_t vfs_get_dir_entries(
    struct vfs_file* file,
    struct __dirent* buffer,
    size_t buffer_len,
    const struct ident* identity,
    size_t* out_count
);

// Generic directory listing implementation which walks the cached children of `entry`.
errno_t vfs_dir_get_cached_entries(
    struct vfs_node* node,
    struct vfs_entry* entry,
    off_t offset,
    struct __dirent* buffer,
    size_t buffer_len,
    const struct ident* identity,
    size_t* out_count
);

// Creates an inode belonging to the anonymous pseudo file system.
errno_t vfs_create_anon_inode(
    const struct vfs_file_ops* file_ops,
    enum vfs_node_type type,
    mode_t mode,
    const struct ident* identity,
    struct vfs_node** out
);

INIT_EXTERN(vfs_stage);
