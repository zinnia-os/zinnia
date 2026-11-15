// SPDX-License-Identifier: BSD-3-Clause

#define LOG_FMT(fmt) "block: " fmt

#include <dev/block/blockdev.h>
#include <dev/char/chardev.h>
#include <dev/char/devctl.h>
#include <lib/initgraph.h>
#include <lib/print.h>
#include <lib/tailq.h>
#include <lib/utils.h>
#include <lib/vec.h>
#include <mem/alloc.h>
#include <mem/page.h>
#include <mem/usercopy.h>
#include <mem/vma.h>
#include <string.h>
#include <zinnia/block.h>

static size_t lbas_per_page(struct blockdev* dev) {
    return arch_mem_page_size() / dev->lba_size;
}

static errno_t cache_get_pages(struct vmo* vmo, uint64_t index, struct page** pages, size_t count, size_t* out_valid) {
    struct blockdev* dev = containerof(vmo, struct blockdev, cache);
    const size_t page_size = arch_mem_page_size();
    const size_t per_page = lbas_per_page(dev);
    const uint64_t lba = index * per_page;

    if (lba >= dev->lba_count) {
        *out_valid = 0;
        return 0;
    }

    const size_t num_lbas = MIN((uint64_t)count * per_page, dev->lba_count - lba);
    size_t done;
    errno_t status = blockdev_rw_pages(dev, BIO_READ, lba, num_lbas, pages, count, &done);
    if (status)
        return status;

    const size_t valid = done / per_page + (done % per_page != 0);
    if (done % per_page != 0) {
        const size_t tail = (done % per_page) * dev->lba_size;
        memset((uint8_t*)HHDM_PTR(page_addr(pages[valid - 1])) + tail, 0, page_size - tail);
    }
    *out_valid = valid;
    return valid == 0 && num_lbas > 0 ? EIO : 0;
}

static errno_t cache_put_pages(struct vmo* vmo, uint64_t index, struct page** pages, size_t count) {
    struct blockdev* dev = containerof(vmo, struct blockdev, cache);
    const size_t per_page = lbas_per_page(dev);
    const uint64_t lba = index * per_page;
    if (lba >= dev->lba_count)
        return 0;

    const size_t num_lbas = MIN((uint64_t)count * per_page, dev->lba_count - lba);
    size_t done;
    errno_t status = blockdev_rw_pages(dev, BIO_WRITE, lba, num_lbas, pages, count, &done);
    if (status)
        return status;
    return done < num_lbas ? EIO : 0;
}

static void cache_destroy(struct vmo* vmo) {
    struct blockdev* dev = containerof(vmo, struct blockdev, cache);
    if (dev->ops->destroy)
        dev->ops->destroy(dev);
}

static const struct vmo_pager_ops cache_pager = {
    .get_pages = cache_get_pages,
    .put_pages = cache_put_pages,
    .max_cluster = 64,
    .destroy = cache_destroy,
};

void blockdev_init(
    struct blockdev* dev,
    const struct blockdev_ops* ops,
    uint32_t major,
    uint32_t minor,
    size_t lba_size,
    uint64_t lba_count
) {
    vmo_init(&dev->cache, &cache_pager);
    dev->ops = ops;
    dev->dev = dev_make(major, minor);
    dev->lba_size = lba_size;
    dev->lba_count = lba_count;
    atomic_init(&dev->claimed, false);
}

void blockdev_ref(struct blockdev* dev) {
    vmo_ref(&dev->cache);
}

void blockdev_unref(struct blockdev* dev) {
    vmo_unref(&dev->cache);
}

errno_t blockdev_rw(
    struct blockdev* dev,
    enum bio_op op,
    uint64_t lba,
    size_t num_lbas,
    const struct bio_seg* segs,
    size_t num_segs,
    size_t* out_lbas
) {
    struct bio* bio = bio_alloc(op, num_segs);
    if (!bio)
        return ENOMEM;
    memcpy(bio->segs, segs, num_segs * sizeof(struct bio_seg));
    bio->lba = lba;
    bio->num_lbas = num_lbas;
    bio_submit(dev, bio);
    errno_t status = bio_wait(bio, out_lbas);
    bio_free(bio);
    return status;
}

errno_t blockdev_rw_pages(
    struct blockdev* dev,
    enum bio_op op,
    uint64_t lba,
    size_t num_lbas,
    struct page** pages,
    size_t count,
    size_t* out_lbas
) {
    const size_t page_size = arch_mem_page_size();
    size_t bytes = num_lbas * dev->lba_size;
    if (bytes > count * page_size)
        return EINVAL;

    struct bio* bio = bio_alloc(op, count);
    if (!bio)
        return ENOMEM;

    size_t num_segs = 0;
    for (size_t i = 0; i < count && bytes > 0; i++) {
        const size_t len = MIN(bytes, page_size);
        bio->segs[num_segs++] = (struct bio_seg){.addr = page_addr(pages[i]), .len = len};
        bytes -= len;
    }
    bio->num_segs = num_segs;
    bio->lba = lba;
    bio->num_lbas = num_lbas;
    bio_submit(dev, bio);
    errno_t status = bio_wait(bio, out_lbas);
    bio_free(bio);
    return status;
}

errno_t blockdev_flush(struct blockdev* dev) {
    struct bio* bio = bio_alloc(BIO_FLUSH, 0);
    if (!bio)
        return ENOMEM;
    bio_submit(dev, bio);
    errno_t status = bio_wait(bio, nullptr);
    bio_free(bio);
    return status;
}

static struct blockdev* file_dev(struct vfs_file* file) {
    return file->priv;
}

static uint64_t dev_size(struct blockdev* dev) {
    return dev->lba_count * dev->lba_size;
}

static errno_t blockdev_file_read(struct vfs_file* file, iovec_iter_t* buffer, uint64_t offset, ssize_t* out_read) {
    struct blockdev* dev = file_dev(file);
    *out_read = 0;
    if (offset >= dev_size(dev))
        return 0;

    size_t progress;
    errno_t status =
        vmo_read_iter(&dev->cache, offset, buffer, MIN(iovec_iter_len(buffer), dev_size(dev) - offset), &progress);
    *out_read = progress;
    return progress > 0 ? 0 : status;
}

static errno_t blockdev_file_write(struct vfs_file* file, iovec_iter_t* buffer, uint64_t offset, ssize_t* out_written) {
    struct blockdev* dev = file_dev(file);
    *out_written = 0;
    if (iovec_iter_len(buffer) == 0)
        return 0;
    if (offset >= dev_size(dev))
        return ENOSPC;

    size_t progress;
    errno_t status =
        vmo_write_iter(&dev->cache, offset, buffer, MIN(iovec_iter_len(buffer), dev_size(dev) - offset), &progress);
    *out_written = progress;
    return progress > 0 ? 0 : status;
}

static errno_t blockdev_file_ioctl(struct vfs_file* file, uintptr_t request, uintptr_t arg, uintptr_t* out_result) {
    struct blockdev* dev = file_dev(file);
    switch (request) {
    case BLKGETSIZE64: {
        const uint64_t size = dev_size(dev);
        if (!usercopy_write((__user void*)arg, &size, sizeof(size)))
            return EFAULT;
        *out_result = 0;
        return 0;
    }
    case BLKSSZGET: {
        const unsigned int lba_size = dev->lba_size;
        if (!usercopy_write((__user void*)arg, &lba_size, sizeof(lba_size)))
            return EFAULT;
        *out_result = 0;
        return 0;
    }
    case BLKRRPART: {
        errno_t status = blockdev_rescan(dev);
        if (!status)
            *out_result = 0;
        return status;
    }
    default:
        if (dev->ops->ioctl)
            return dev->ops->ioctl(dev, file, request, arg, out_result);
        return ENOTTY;
    }
}

static errno_t blockdev_file_sync(struct vfs_file* file, bool data_only) {
    struct blockdev* dev = file_dev(file);
    errno_t status = vmo_sync(&dev->cache, 0, UINT64_MAX);
    if (status)
        return status;
    return blockdev_flush(dev);
}

static errno_t blockdev_file_mmap(
    struct vfs_file* file,
    struct vma* vma,
    uintptr_t addr,
    size_t len,
    enum prot_flags prot,
    enum map_flags flags,
    off_t offset,
    uintptr_t* out_addr
) {
    errno_t status =
        vma_map(vma, &file_dev(file)->cache, addr, len, prot, (flags & MAP_SHARED) ? VMA_SHARED : 0, offset);
    if (status)
        return status;
    *out_addr = addr;
    return 0;
}

static void blockdev_file_release(struct vfs_file* file) {
    blockdev_unref(file_dev(file));
}

static const struct vfs_file_ops blockdev_file_ops = {
    .read = blockdev_file_read,
    .write = blockdev_file_write,
    .ioctl = blockdev_file_ioctl,
    .sync = blockdev_file_sync,
    .mmap = blockdev_file_mmap,
    .release = blockdev_file_release,
};

static errno_t blockdev_node_open(struct vfs_node* node, struct vfs_file* file) {
    struct blockdev* dev = node->priv;
    blockdev_ref(dev);
    file->priv = dev;
    file->ops = &blockdev_file_ops;
    return 0;
}

static void blockdev_node_destroy(struct vfs_node* node) {
    blockdev_unref(node->priv);
}

static const struct vfs_node_ops blockdev_node_ops = {
    .destroy = blockdev_node_destroy,
    .device = {
        .open = blockdev_node_open,
    },
};

errno_t devtmpfs_add_blockdev(const char* name, struct blockdev* dev, mode_t mode) {
    struct vfs_mknod_target target = {
        .type = VNODE_BLOCKDEV,
        .ops = &blockdev_node_ops,
        .priv = dev,
        .rdev = dev->dev,
    };

    blockdev_ref(dev);
    struct vfs_path root = devtmpfs_root();
    errno_t status = vfs_mknod(root, root, name, mode, &target, ident_kernel(), nullptr);
    vfs_path_drop(root);
    if (status) {
        blockdev_unref(dev);
        return status;
    }
    devctl_notify_create(name);
    return 0;
}

errno_t blockdev_from_node(struct vfs_node* node, struct blockdev** out) {
    if (node->type != VNODE_BLOCKDEV || node->ops != &blockdev_node_ops)
        return ENOTBLK;

    struct blockdev* dev = node->priv;
    blockdev_ref(dev);
    *out = dev;
    return 0;
}

errno_t blockdev_claim(struct blockdev* dev) {
    bool expected = false;
    if (!atomic_compare_exchange(&dev->claimed, &expected, true, ATOMIC_ACQUIRE, ATOMIC_RELAXED))
        return EBUSY;
    return 0;
}

void blockdev_release(struct blockdev* dev) {
    atomic_store(&dev->claimed, false, ATOMIC_RELEASE);
}

struct disk_node {
    char name[64];
    struct blockdev* dev;
};

struct disk {
    char name[32];
    struct blockdev* dev;
    vec(struct disk_node) nodes;
    tailq_link(struct disk) link;
};

static tailq_head(struct disk) disks;
static mutex_t disks_lock;

static void add_node(struct disk* disk, const char* name, struct blockdev* dev) {
    struct disk_node node = {.dev = dev};
    ksnprintf(node.name, sizeof(node.name), "%s", name);
    if (vec_push(&disk->nodes, node) && dev)
        blockdev_unref(dev);
}

static void scan_partitions(struct disk* disk) {
    struct gpt_partition* parts;
    size_t count;
    size_t capacity;
    if (gpt_scan(disk->dev, &parts, &count, &capacity))
        return;

    for (size_t i = 0; i < count; i++) {
        const struct gpt_partition* part = &parts[i];
        if (part->end_lba < part->start_lba || part->end_lba >= disk->dev->lba_count) {
            log_warn("Ignoring partition %zu of \"%s\" with invalid range\n", i + 1, disk->name);
            continue;
        }

        char part_name[48];
        ksnprintf(part_name, sizeof(part_name), "%sp%zu", disk->name, i + 1);

        struct blockdev* dev;
        errno_t status = partition_new(disk->dev, part->start_lba, part->end_lba - part->start_lba + 1, &dev);
        if (status) {
            log_warn("Unable to create partition \"%s\" (%i)\n", part_name, status);
            continue;
        }

        char path[64];
        ksnprintf(path, sizeof(path), "block/%s", part_name);
        status = devtmpfs_add_blockdev(path, dev, 0660);
        if (status) {
            blockdev_unref(dev);
            log_warn("Unable to register \"%s\" (%i)\n", path, status);
            continue;
        }
        add_node(disk, path, dev);

        char type_str[GUID_STR_LEN];
        char uuid_str[GUID_STR_LEN];
        guid_format(&part->type_guid, type_str);
        guid_format(&part->unique_guid, uuid_str);

        ksnprintf(path, sizeof(path), "block/parttype-%s", type_str);
        status = devtmpfs_symlink(path, part_name);
        if (!status)
            add_node(disk, path, nullptr);
        else if (status == EEXIST)
            log("Partition type %s already claimed, no \"%s\" link for \"%s\"\n", type_str, path, part_name);

        ksnprintf(path, sizeof(path), "block/partuuid-%s", uuid_str);
        status = devtmpfs_symlink(path, part_name);
        if (!status)
            add_node(disk, path, nullptr);

        log("Partition %zu: \"%s\" Type: %s UUID: %s\n", i + 1, part_name, type_str, uuid_str);
    }

    mem_free(parts, capacity * sizeof(*parts));
}

errno_t blockdev_register(const char* name, struct blockdev* dev) {
    struct disk* disk = mem_alloc(sizeof(struct disk), 0);
    if (!disk)
        return ENOMEM;
    ksnprintf(disk->name, sizeof(disk->name), "%s", name);
    vec_init(&disk->nodes);

    char path[64];
    ksnprintf(path, sizeof(path), "block/%s", name);
    errno_t status = devtmpfs_add_blockdev(path, dev, 0660);
    if (status) {
        mem_free(disk, sizeof(*disk));
        return status;
    }

    blockdev_ref(dev);
    disk->dev = dev;
    log("Registered block device \"%s\"\n", name);

    mutex_lock(&disks_lock);
    tailq_insert_tail(&disks, disk, link);
    scan_partitions(disk);
    mutex_unlock(&disks_lock);
    return 0;
}

errno_t blockdev_rescan(struct blockdev* dev) {
    mutex_lock(&disks_lock);
    struct disk* disk;
    for (disk = tailq_first(&disks); disk; disk = tailq_next(disk, link)) {
        if (disk->dev == dev)
            break;
    }
    if (!disk) {
        mutex_unlock(&disks_lock);
        return ENOTTY;
    }

    for (size_t i = 0; i < vec_length(&disk->nodes); i++) {
        struct disk_node* node = &vec_elem_get(&disk->nodes, i);
        errno_t status = devtmpfs_remove(node->name);
        if (status)
            log_warn("Unable to remove stale node \"%s\" (%i)\n", node->name, status);
        if (node->dev)
            blockdev_unref(node->dev);
    }
    disk->nodes.length = 0;

    scan_partitions(disk);
    mutex_unlock(&disks_lock);
    return 0;
}

errno_t blockdev_sync_all() {
    errno_t result = 0;
    mutex_lock(&disks_lock);
    for (struct disk* disk = tailq_first(&disks); disk; disk = tailq_next(disk, link)) {
        for (size_t i = 0; i < vec_length(&disk->nodes); i++) {
            struct blockdev* part = vec_elem_get(&disk->nodes, i).dev;
            errno_t status = part ? vmo_sync(&part->cache, 0, UINT64_MAX) : 0;
            if (status && !result)
                result = status;
        }
        errno_t status = vmo_sync(&disk->dev->cache, 0, UINT64_MAX);
        if (!status)
            status = blockdev_flush(disk->dev);
        if (status && !result)
            result = status;
    }
    mutex_unlock(&disks_lock);
    return result;
}

static void block_stage_init(struct boot_info* info) {
    tailq_init(&disks);
    mutex_init(&disks_lock);
    errno_t status = devtmpfs_mkdir("block", 0755);
    if (status && status != EEXIST)
        log_warn("Unable to create /dev/block (%i)\n", status);
}

INIT_TASK(block_stage, "dev.block", block_stage_init);
INIT_DEPENDS(block_stage, devtmpfs_stage);
