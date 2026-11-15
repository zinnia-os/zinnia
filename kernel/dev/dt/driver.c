// SPDX-License-Identifier: BSD-3-Clause

#define LOG_FMT(fmt) "fdt: " fmt

#include <dev/dt/fdt.h>
#include <lib/init.h>
#include <lib/print.h>
#include <lib/spin.h>
#include <lib/vec.h>
#include <mem/alloc.h>
#include <mem/page.h>
#include <string.h>

static struct fdt tree;
static bool tree_valid = false;

static struct spinlock driver_lock = {};
static vec(struct fdt_driver*) drivers = {};

struct fdt* fdt_get() {
    return tree_valid ? &tree : nullptr;
}

static const struct fdt_match* fdt_match_table(
    const struct fdt_match* table,
    size_t num_matches,
    struct fdt_node node
) {
    for (size_t i = 0; i < num_matches; i++) {
        if (fdt_node_is_compatible(node, table[i].compatible))
            return &table[i];
    }
    return nullptr;
}

errno_t fdt_driver_register(struct fdt_driver* driver) {
    if (driver->match == nullptr || driver->num_matches == 0) {
        log_warn("Driver \"%s\" does not define any matches, ignoring\n", driver->name);
        return 0;
    }

    if (!tree_valid)
        return ENODEV;

    spin_lock(&driver_lock);
    vec_foreach(&drivers, i) {
        if (strcmp(vec_elem_get(&drivers, i)->name, driver->name) == 0) {
            spin_unlock(&driver_lock);
            log_warn("Driver \"%s\" is already registered\n", driver->name);
            return EEXIST;
        }
    }
    errno_t status = vec_push(&drivers, driver);
    spin_unlock(&driver_lock);
    if (status != 0)
        return status;

    log("Registered platform driver \"%s\" with %zu match%s\n",
        driver->name,
        driver->num_matches,
        driver->num_matches == 1 ? "" : "es");

    for (size_t i = 0; i < driver->num_matches; i++)
        log("  [%zu] = \"%s\"\n", i, driver->match[i].compatible);

    for (struct fdt_node node = fdt_root(&tree); node.tree; node = fdt_next_node(node)) {
        const struct fdt_match* match = fdt_match_table(driver->match, driver->num_matches, node);
        if (match == nullptr)
            continue;

        log("Matched driver \"%s\" on rule [%zu]\n", driver->name, (size_t)(match - driver->match));

        status = driver->probe(node, match);
        if (status != 0)
            log_warn("%s failed to probe: %i\n", driver->name, status);
    }

    return 0;
}

[[__init]]
static bool fdt_stage_init(struct boot_info* info) {
    if (!info->dtb)
        return false;

    const void* blob = HHDM_PTR(info->dtb);
    if (!fdt_init(&tree, blob, info->dtb_size)) {
        log_warn("Unable to parse the device tree blob\n");
        return false;
    }
    tree_valid = true;

    struct fdt_prop model = fdt_property(fdt_root(&tree), "model");
    if (model.tree && model.length)
        log("Running on \"%.*s\"\n", (int)model.length, (const char*)model.data);

    return true;
}
INIT_TASK(fdt_stage, "dev.dt", fdt_stage_init);
INIT_DEPENDS(fdt_stage, mem_stage);
