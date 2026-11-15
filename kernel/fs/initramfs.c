// SPDX-License-Identifier: BSD-3-Clause

#include <dev/block/blockdev.h>
#include <fs/initramfs.h>
#include <fs/vfs.h>
#include <lib/assert.h>
#include <lib/errno.h>
#include <lib/init.h>
#include <lib/print.h>
#include <mem/alloc.h>
#include <mem/page.h>
#include <zinnia/limits.h>
#include <zinnia/stat.h>

enum ustar_type : uint8_t {
    USTAR_REGULAR = 0,
    USTAR_NORMAL = '0',
    USTAR_SYM_LINK = '2',
    USTAR_DIRECTORY = '5',
    USTAR_CONTIGUOUS = '7',
    USTAR_LONG_LINK = 'L',
};

struct ustar_header {
    char name[100];
    char mode[8];
    char uid[8];
    char gid[8];
    char size[12];
    char mtime[12];
    char checksum[8];
    enum ustar_type type;
    char linkname[100];
    char signature[6];
    char version[2];
    char owner[32];
    char group[32];
    char devmajor[8];
    char devminor[8];
    char prefix[155];
    char pad[12];
};

static_assert(sizeof(struct ustar_header) == 512, "USTAR header must be 512 bytes");

static size_t oct2bin(const char* str, size_t len) {
    size_t value = 0;
    for (size_t i = 0; i < len; i++) {
        if (str[i] == '\0' || str[i] == ' ')
            break;
        value *= 8;
        value += (size_t)(str[i] - '0');
    }
    return value;
}

static void copy_field(char* out, const char* field, size_t field_len, size_t out_len) {
    size_t i = 0;
    while (i < field_len && field[i] != '\0' && i + 1 < out_len) {
        out[i] = field[i];
        i++;
    }
    out[i] = '\0';
}

static errno_t create_dirs(
    struct vfs_path root,
    struct vfs_path at,
    const char* path,
    mode_t mode,
    struct vfs_path* out_dir,
    char* name_out,
    size_t name_out_len
) {
    size_t path_len = strlen(path);

    size_t dir_len = 0;
    const char* name = path;
    for (size_t i = path_len; i > 0; i--) {
        if (path[i - 1] == '/') {
            dir_len = i - 1;
            name = path + i;
            break;
        }
    }

    strncpy(name_out, name, name_out_len - 1);
    name_out[name_out_len - 1] = '\0';

    struct vfs_path current = vfs_path_clone(at);

    size_t index = 0;
    while (index < dir_len) {
        while (index < dir_len && path[index] == '/')
            index++;
        if (index >= dir_len)
            break;

        size_t start = index;
        while (index < dir_len && path[index] != '/')
            index++;

        char component[__NAME_MAX + 1];
        size_t length = MIN(index - start, __NAME_MAX);
        memcpy(component, path + start, length);
        component[length] = '\0';

        errno_t status = vfs_mkdir(root, current, component, mode, ident_kernel());
        if (status && status != EEXIST) {
            vfs_path_drop(current);
            return status;
        }

        struct vfs_file* dir;
        status = vfs_file_open(root, current, component, O_DIRECTORY, 0, ident_kernel(), &dir);
        if (status) {
            vfs_path_drop(current);
            return status;
        }

        struct vfs_path next = vfs_path_clone(dir->path);
        vfs_file_unref(dir);
        vfs_path_drop(current);
        current = next;
    }

    *out_dir = current;
    return 0;
}

bool initramfs_is_valid(const void* data, size_t len) {
    const struct ustar_header* header = data;
    return len >= sizeof(*header) && !memcmp(header->signature, "ustar", 5);
}

// Sets all timestamps of the node behind `file` to `mtime`.
static void set_times(struct vfs_file* file, size_t mtime) {
    struct vfs_node* node = file->vnode;
    timespec_t time = time_from_sec(mtime);

    mutex_lock(&node->lock);
    node->atime = time;
    node->mtime = time;
    node->ctime = time;
    mutex_unlock(&node->lock);
}

errno_t initramfs_load(struct vfs_path root, struct vfs_path target, const void* data, size_t len) {
    const uint8_t* base = data;
    size_t offset = 0;
    size_t files_loaded = 0;
    errno_t status = 0;

    // Paths can be much longer than what fits on the stack.
    char* override = mem_alloc(__PATH_MAX + 1, ALLOC_NOZERO);
    char* name = mem_alloc(__PATH_MAX + 1, ALLOC_NOZERO);
    if (!override || !name) {
        status = ENOMEM;
        goto out;
    }
    bool has_override = false;

    while (offset + sizeof(struct ustar_header) <= len) {
        const struct ustar_header* header = (const struct ustar_header*)(base + offset);
        if (memcmp(header->signature, "ustar", 5) != 0)
            break;

        mode_t file_mode = (mode_t)oct2bin(header->mode, sizeof(header->mode));
        size_t file_size = oct2bin(header->size, sizeof(header->size));
        size_t mtime = oct2bin(header->mtime, sizeof(header->mtime));

        size_t data_offset = offset + sizeof(struct ustar_header);
        if (file_size > len - data_offset) {
            log_warn("vfs: Truncated initramfs entry at offset %zu\n", offset);
            status = EINVAL;
            goto out;
        }

        if (has_override) {
            memcpy(name, override, __PATH_MAX + 1);
            has_override = false;
        } else {
            copy_field(name, header->name, sizeof(header->name), __PATH_MAX + 1);
        }

        // Directory entries usually end with a slash, which would leave us with an empty file name.
        size_t name_len = strlen(name);
        while (name_len > 1 && name[name_len - 1] == '/')
            name[--name_len] = '\0';

        struct vfs_path dir;
        char file_name[__NAME_MAX + 1];

        switch (header->type) {
        case USTAR_REGULAR:
        case USTAR_NORMAL:
        case USTAR_CONTIGUOUS: {
            status = create_dirs(root, target, name, file_mode, &dir, file_name, sizeof(file_name));
            if (status)
                goto out;

            struct vfs_file* file;
            status = vfs_file_open(root, dir, file_name, O_CREAT, file_mode, ident_kernel(), &file);
            vfs_path_drop(dir);
            if (status)
                goto out;

            if (file_size > 0) {
                kvec_t vec = {
                    .iov_base = (void*)(base + data_offset),
                    .iov_len = file_size,
                };
                iovec_iter_t iter = iovec_iter_kernel(&vec, 1);
                ssize_t written = 0;
                status = vfs_file_pwrite(file, &iter, 0, &written);
                if (status) {
                    vfs_file_unref(file);
                    goto out;
                }
            }

            set_times(file, mtime);
            vfs_file_unref(file);
            files_loaded++;
            break;
        }
        case USTAR_SYM_LINK: {
            status = create_dirs(root, target, name, file_mode, &dir, file_name, sizeof(file_name));
            if (status)
                goto out;

            char link_target[101];
            copy_field(link_target, header->linkname, sizeof(header->linkname), sizeof(link_target));

            status = vfs_symlink(root, dir, file_name, link_target, ident_kernel());
            vfs_path_drop(dir);
            if (status)
                goto out;

            files_loaded++;
            break;
        }
        case USTAR_DIRECTORY: {
            status = create_dirs(root, target, name, file_mode, &dir, file_name, sizeof(file_name));
            if (status)
                goto out;

            status = vfs_mkdir(root, dir, file_name, file_mode, ident_kernel());
            vfs_path_drop(dir);
            if (status && status != EEXIST)
                goto out;
            status = 0;

            files_loaded++;
            break;
        }
        case USTAR_LONG_LINK: {
            size_t copy_len = MIN(file_size, __PATH_MAX);
            memcpy(override, base + data_offset, copy_len);
            override[copy_len] = '\0';
            has_override = true;
            break;
        }
        default:
            break;
        }

        offset += sizeof(struct ustar_header) + ROUND_UP(file_size, 512);
    }

    log("vfs: Loaded %zu files from initramfs\n", files_loaded);

out:
    mem_free(override, __PATH_MAX + 1);
    mem_free(name, __PATH_MAX + 1);
    return status;
}

void initramfs_load_boot_modules(struct boot_info* info) {
    struct vfs_path root = vfs_get_root();
    size_t ramdisk_index = 0;

    for (size_t i = 0; i < info->num_files; i++) {
        struct boot_file* file = &info->files[i];
        const void* data = HHDM_PTR(file->data);

        if (!initramfs_is_valid(data, file->length)) {
            char name[16];
            ksnprintf(name, sizeof(name), "ram%zu", ramdisk_index);
            struct blockdev* disk;
            errno_t status = ramdisk_new(file->data, file->length, &disk);
            if (!status) {
                status = blockdev_register(name, disk);
                blockdev_unref(disk);
            }
            if (status) {
                log_warn("initramfs: Unable to expose module \"%s\" as a RAM disk (%i)\n", file->path, status);
                continue;
            }
            log("initramfs: Exposed module \"%s\" as RAM disk /dev/block/%s\n", file->path, name);
            ramdisk_index++;
            continue;
        }

        errno_t status = initramfs_load(root, root, data, file->length);
        assert(status == 0, "Failed to load initramfs \"%s\" (%i)", file->path, status);
    }

    vfs_path_drop(root);
}
