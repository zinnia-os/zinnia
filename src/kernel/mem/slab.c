#include <zinnia/status.h>
#include <common/compiler.h>
#include <kernel/alloc.h>
#include <kernel/spin.h>
#include <string.h>

typedef struct {
    size_t num_pages; // Amount of pages connected to this slab.
    size_t size;      // Size of this slab.
} slab_info;

struct slab {
    struct spinlock lock; // Access lock.
    size_t ent_size;      // Size of one entry.
    void** head;
};

typedef struct {
    struct slab* slab;
} slab_header;

// Initializes the SLAB structures.

static struct slab slabs[8] = {0};

static zn_status_t slab_new(struct slab* slab, size_t size) {
    slab->lock = (struct spinlock){0};

    // Allocate a new page for the head.
    phys_t addr;
    zn_status_t status = mem_phys_alloc(1, 0, &addr);
    if (status)
        return status;

    slab->head = (void**)(addr + arch_mem_hhdm_addr());
    slab->ent_size = size;

    const size_t offset = ALIGN_UP(sizeof(slab_header), size);
    const size_t available_size = arch_mem_page_size() - offset;

    slab_header* ptr = (slab_header*)slab->head;
    ptr->slab = slab;
    slab->head = (void**)((void*)slab->head + offset);

    void** arr = slab->head;
    const size_t max = available_size / size - 1;
    const size_t fact = size / sizeof(void*);

    for (size_t i = 0; i < max; i++) {
        arr[i * fact] = &arr[(i + 1) * fact];
    }
    arr[max * fact] = nullptr;

    return ZN_OK;
}

void slab_init(void) {
    slab_new(&slabs[0], 16);
    slab_new(&slabs[1], 32);
    slab_new(&slabs[2], 64);
    slab_new(&slabs[3], 128);
    slab_new(&slabs[4], 256);
    slab_new(&slabs[5], 512);
    slab_new(&slabs[6], 1024);
    slab_new(&slabs[7], 2048);
}

static void* slab_do_alloc(struct slab* slab) {
    spin_lock(&slab->lock);

    if (__unlikely(slab->head == nullptr))
        slab_new(slab, slab->ent_size);

    void** old_free = slab->head;
    slab->head = *old_free;
    memset(old_free, 0, slab->ent_size);

    spin_unlock(&slab->lock);
    return old_free;
}

static void slab_do_free(struct slab* slab, void* addr) {
    spin_lock(&slab->lock);

    if (__unlikely(addr == nullptr))
        goto cleanup;

    void** new_head = addr;
    *new_head = slab->head;
    slab->head = new_head;

cleanup:
    spin_unlock(&slab->lock);
}

static inline struct slab* slab_find_size(size_t size) {
    for (size_t i = 0; i < ARRAY_SIZE(slabs); i++) {
        if (slabs[i].ent_size >= size)
            return &slabs[i];
    }
    return nullptr;
}

void* mem_alloc(size_t size, enum alloc_flags flags) {
    if (__unlikely(size == 0))
        return nullptr;

    struct slab* slab = slab_find_size(size);
    if (slab != nullptr) {
        return slab_do_alloc(slab);
    }

    size_t num_pages = ROUND_UP(size, arch_mem_page_size());

    // Allocate the pages plus an additional page for metadata.
    phys_t ret;
    zn_status_t status = mem_phys_alloc(num_pages + 1, 0, &ret);
    if (__unlikely(status != ZN_OK))
        return nullptr;

    ret = ret + (phys_t)arch_mem_hhdm_addr();
    // Write metadata into the first page.
    slab_info* info = (slab_info*)ret;
    info->num_pages = num_pages;
    info->size = size;
    // Skip the first page and return the next one.
    return (void*)(ret + arch_mem_page_size());
}

void mem_free(void* addr) {
    if (__unlikely(addr == nullptr))
        return;

    // If the address is page aligned.
    if ((size_t)addr == ALIGN_DOWN((size_t)addr, arch_mem_page_size())) {
        slab_info* info = (slab_info*)(addr - arch_mem_page_size());
        mem_phys_free(((phys_t)info - (phys_t)arch_mem_hhdm_addr()), info->num_pages + 1);
        return;
    }

    slab_header* header = (slab_header*)(ALIGN_DOWN((size_t)addr, arch_mem_page_size()));
    slab_do_free(header->slab, addr);
}
