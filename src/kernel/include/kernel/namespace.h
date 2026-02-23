#pragma once

#include <zinnia/handle.h>
#include <kernel/channel.h>
#include <kernel/mutex.h>
#include <kernel/spin.h>
#include <kernel/vas.h>
#include <kernel/vmo.h>

enum namespace_desc_type {
    NAMESPACE_DESC_NS,
    NAMESPACE_DESC_TASK,
    NAMESPACE_DESC_VAS,
    NAMESPACE_DESC_VMO,
};

struct namespace_desc {
    zn_handle_t handle;
    enum namespace_desc_type type;
    union {
        struct namespace* namespace;
        struct channel* channel;
        struct vas* vas;
        struct vmo* vmo;
    };
    SLIST_LINK(struct namespace_desc*) next;
};

// Translates between handles and objects.
struct namespace {
    zn_handle_t next_handle;
    struct mutex mutex;
    SLIST_HEAD(struct namespace_desc*) descriptors;
};

// Creates a new namespace with no contents.
zn_status_t namespace_new(struct namespace** out);

// Adds a descriptor to this namespace.
zn_status_t namespace_add_desc(struct namespace* namespace, struct namespace_desc desc, zn_handle_t* out);

// Gets the descriptor referenced by the given handle.
zn_status_t namespace_get(struct namespace* namespace, zn_handle_t handle, struct namespace_desc* out);

// Deletes a descriptor from this namespace.
zn_status_t namespace_del_desc(struct namespace* namespace, zn_handle_t handle);
