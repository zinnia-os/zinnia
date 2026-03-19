#include <kernel/alloc.h>
#include <kernel/exec.h>
#include <kernel/hashmap.h>
#include <kernel/print.h>
#include <kernel/spin.h>
#include <uapi/errno.h>
#include <string.h>

static struct spinlock formats_lock = {0};
static HASHMAP(const char*, const struct exec_format*) formats = {0};

errno_t exec_register(const char* name, const struct exec_format* format) {
    spin_lock(&formats_lock);

    const size_t name_len = strlen(name) + 1;
    char* cloned_name = mem_alloc(name_len, 0);
    if (!cloned_name) {
        spin_unlock(&formats_lock);
        return ENOMEM;
    }
    memcpy(cloned_name, name, name_len);

    errno_t res = HASHMAP_INSERT(&formats, cloned_name, format, hashmap_hash_string, hashmap_eq_string);
    spin_unlock(&formats_lock);
    return res;
}

errno_t exec_file(struct exec_info* info, struct task** result) {
    const struct exec_format** fmt = HASHMAP_GET(&formats, "elf", hashmap_hash_string, hashmap_eq_string);
    if (!fmt)
        kprintf("Format not found!\n");
    else {
        kprintf("%p\n", (*fmt)->identify);
    }

    HASHMAP_FOREACH(&formats, f) {
        auto name = formats.keys[f];
        kprintf("Format: %s\n", name);
    }
    return 0;
}
