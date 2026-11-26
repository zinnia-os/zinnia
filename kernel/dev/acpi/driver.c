// SPDX-License-Identifier: BSD-3-Clause

#define LOG_FMT(fmt) "acpi: " fmt

#include <dev/acpi/acpi.h>
#include <lib/mutex.h>
#include <lib/print.h>
#include <lib/vec.h>
#include <mem/alloc.h>
#include <string.h>
#include <uacpi/namespace.h>
#include <uacpi/status.h>
#include <uacpi/types.h>
#include <uacpi/utilities.h>

static mutex_t driver_lock;
static vec(struct acpi_driver*) drivers = {};
static vec(uacpi_namespace_node*) claimed = {};

static const struct acpi_match* acpi_match_table(
    const struct acpi_match* table,
    size_t num_matches,
    uacpi_namespace_node* node
) {
    for (size_t i = 0; i < num_matches; i++) {
        const char* const ids[] = {table[i].hid, nullptr};
        if (uacpi_device_matches_pnp_id(node, ids))
            return &table[i];
    }
    return nullptr;
}

static bool is_claimed_locked(uacpi_namespace_node* node) {
    vec_foreach(&claimed, i) {
        if (vec_elem_get(&claimed, i) == node)
            return true;
    }
    return false;
}

static uacpi_iteration_decision probe_one(void* user, uacpi_namespace_node* node, uacpi_u32) {
    struct acpi_driver* driver = user;
    const struct acpi_match* match = acpi_match_table(driver->match, driver->num_matches, node);
    if (match == nullptr)
        return UACPI_ITERATION_DECISION_CONTINUE;

    mutex_lock(&driver_lock);
    errno_t status = is_claimed_locked(node) ? EBUSY : vec_push(&claimed, node);
    mutex_unlock(&driver_lock);
    if (status != 0)
        return UACPI_ITERATION_DECISION_CONTINUE;

    struct acpi_device* dev = (struct acpi_device*)node;
    char name[5];
    acpi_device_name(dev, name);
    log("%s: Matched driver \"%s\" on rule [%zu]\n", name, driver->name, (size_t)(match - driver->match));

    status = driver->probe(dev, match);

    // Unclaim the node if we failed to probe.
    if (status != 0) {
        log_warn("%s: %s failed to probe: %i\n", name, driver->name, status);
        mutex_lock(&driver_lock);
        vec_foreach(&claimed, i) {
            if (vec_elem_get(&claimed, i) == node) {
                claimed.data[i] = claimed.data[claimed.length - 1];
                claimed.length--;
                break;
            }
        }
        mutex_unlock(&driver_lock);
    }
    return UACPI_ITERATION_DECISION_CONTINUE;
}

errno_t acpi_driver_register(struct acpi_driver* driver) {
    if (driver->match == nullptr || driver->num_matches == 0) {
        log_warn("Driver \"%s\" does not define any matches, ignoring\n", driver->name);
        return 0;
    }

    if (!acpi_namespace_ready())
        return ENODEV;

    const size_t hids_size = (driver->num_matches + 1) * sizeof(const char*);
    const char** hids = mem_alloc(hids_size, 0);
    if (!hids)
        return ENOMEM;
    for (size_t i = 0; i < driver->num_matches; i++)
        hids[i] = driver->match[i].hid;
    hids[driver->num_matches] = nullptr;

    mutex_lock(&driver_lock);
    vec_foreach(&drivers, i) {
        if (strcmp(vec_elem_get(&drivers, i)->name, driver->name) == 0) {
            mutex_unlock(&driver_lock);
            mem_free(hids, hids_size);
            log_warn("Driver \"%s\" is already registered\n", driver->name);
            return EEXIST;
        }
    }
    errno_t status = vec_push(&drivers, driver);
    mutex_unlock(&driver_lock);
    if (status != 0) {
        mem_free(hids, hids_size);
        return status;
    }

    log("Registered ACPI driver \"%s\" with %zu match%s\n",
        driver->name,
        driver->num_matches,
        driver->num_matches == 1 ? "" : "es");
    for (size_t i = 0; i < driver->num_matches; i++)
        log("  [%zu] = \"%s\"\n", i, driver->match[i].hid);

    uacpi_status ret = uacpi_find_devices_at(uacpi_namespace_root(), hids, probe_one, driver);
    mem_free(hids, hids_size);
    if (ret != UACPI_STATUS_OK)
        log_warn("Device enumeration for \"%s\" failed: %s\n", driver->name, uacpi_status_to_string(ret));
    return 0;
}
