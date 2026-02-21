#pragma once

#include <zinnia/handle.h>
#include <kernel/channel.h>
#include <kernel/mutex.h>
#include <kernel/spin.h>
#include <kernel/vas.h>
#include <kernel/vmo.h>
#include <stddef.h>

enum namespace_desc_type {
    NAMESPACE_DESC_NS,
    NAMESPACE_DESC_TASK,
    NAMESPACE_DESC_VAS,
    NAMESPACE_DESC_VMO,
};

struct namespace_desc {
    enum namespace_desc_type type;
};

// Translates between handles and objects.
struct namespace {
    size_t next_handle;
    struct mutex mutex;
};

struct namespace_desc_ns {
    enum namespace_desc_type type;
    struct namespace* namespace;
};

struct namespace_desc_channel {
    struct namespace_desc desc;
    struct channel* channel;
};

struct namespace_desc_vas {
    struct namespace_desc desc;
    struct vas* vas;
};

struct namespace_desc_vmo {
    struct namespace_desc desc;
    struct vmo* vmo;
};

// Creates a new namespace with no contents.
zn_status_t namespace_new(struct namespace** out);

// Adds a descriptor to this namespace.
zn_status_t namespace_add_desc(struct namespace* namespace, struct namespace_desc* desc, zn_handle_t* out);

// Deletes a descriptor from this namespace.
zn_status_t namespace_get(struct namespace* namespace, zn_handle_t handle, struct namespace_desc** desc);

// Deletes a descriptor from this namespace.
zn_status_t namespace_del_desc(struct namespace* namespace, zn_handle_t handle);
