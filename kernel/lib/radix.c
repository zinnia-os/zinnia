// SPDX-License-Identifier: BSD-3-Clause

#include <lib/assert.h>
#include <lib/radix.h>
#include <lib/utils.h>
#include <mem/alloc.h>

static bool covers(const struct radix_node* node, uint64_t index) {
    return node->shift + RADIX_BITS >= 64 || (index >> (node->shift + RADIX_BITS)) == 0;
}

static size_t slot_of(const struct radix_node* node, uint64_t index) {
    return (index >> node->shift) & (RADIX_SLOTS - 1);
}

static uint64_t bit(size_t slot) {
    return (uint64_t)1 << slot;
}

static struct radix_node* node_alloc(uint8_t shift, struct radix_node* parent, uint8_t offset) {
    struct radix_node* node = mem_alloc(sizeof(struct radix_node), 0);
    if (!node)
        return nullptr;
    node->shift = shift;
    node->parent = parent;
    node->offset = offset;
    return node;
}

static void node_free(struct epoch_head* head) {
    mem_free(containerof(head, struct radix_node, epoch), sizeof(struct radix_node));
}

static void node_retire(struct radix_node* node) {
    epoch_call(&node->epoch, node_free);
}

static void free_subtree(struct radix_node* node) {
    if (node->shift > 0) {
        for (size_t i = 0; i < RADIX_SLOTS; i++) {
            struct radix_node* child = atomic_load(&node->slots[i], ATOMIC_RELAXED);
            if (child)
                free_subtree(child);
        }
    }
    mem_free(node, sizeof(*node));
}

void radix_init(struct radix_tree* tree) {
    atomic_init(&tree->root, nullptr);
}

void radix_destroy(struct radix_tree* tree) {
    struct radix_node* root = atomic_load(&tree->root, ATOMIC_RELAXED);
    if (root)
        free_subtree(root);
    atomic_store(&tree->root, nullptr, ATOMIC_RELAXED);
}

bool radix_empty(struct radix_tree* tree) {
    return atomic_load(&tree->root, ATOMIC_RELAXED) == nullptr;
}

static struct radix_node* leaf_of(struct radix_tree* tree, uint64_t index) {
    struct radix_node* node = epoch_deref(&tree->root);
    if (!node || !covers(node, index))
        return nullptr;
    while (node->shift > 0) {
        node = epoch_deref(&node->slots[slot_of(node, index)]);
        if (!node)
            return nullptr;
    }
    return node;
}

void* radix_lookup(struct radix_tree* tree, uint64_t index) {
    struct radix_node* leaf = leaf_of(tree, index);
    return leaf ? epoch_deref(&leaf->slots[slot_of(leaf, index)]) : nullptr;
}

void* radix_replace(struct radix_tree* tree, uint64_t index, void* value) {
    assert(value, "Tried to replace a radix slot with nullptr");
    struct radix_node* leaf = leaf_of(tree, index);
    if (!leaf)
        return nullptr;
    _Atomic(void*)* slot = &leaf->slots[slot_of(leaf, index)];
    void* old = atomic_load(slot, ATOMIC_RELAXED);
    if (old)
        epoch_publish(slot, value);
    return old;
}

static errno_t grow(struct radix_tree* tree, uint64_t index) {
    struct radix_node* root = atomic_load(&tree->root, ATOMIC_RELAXED);
    if (!root) {
        uint8_t shift = 0;
        while (shift + RADIX_BITS < 64 && (index >> (shift + RADIX_BITS)) != 0)
            shift += RADIX_BITS;
        root = node_alloc(shift, nullptr, 0);
        if (!root)
            return ENOMEM;
        epoch_publish(&tree->root, root);
        return 0;
    }

    while (!covers(root, index)) {
        struct radix_node* top = node_alloc(root->shift + RADIX_BITS, nullptr, 0);
        if (!top)
            return ENOMEM;
        atomic_store(&top->slots[0], root, ATOMIC_RELAXED);
        top->count = 1;
        for (size_t t = 0; t < RADIX_NUM_TAGS; t++) {
            if (atomic_load(&root->tags[t], ATOMIC_RELAXED))
                atomic_store(&top->tags[t], 1, ATOMIC_RELAXED);
        }
        root->parent = top;
        root->offset = 0;
        epoch_publish(&tree->root, top);
        root = top;
    }
    return 0;
}

errno_t radix_insert(struct radix_tree* tree, uint64_t index, void* value) {
    errno_t status = grow(tree, index);
    if (status)
        return status;

    struct radix_node* node = atomic_load(&tree->root, ATOMIC_RELAXED);
    while (node->shift > 0) {
        const size_t slot = slot_of(node, index);
        struct radix_node* child = atomic_load(&node->slots[slot], ATOMIC_RELAXED);
        if (!child) {
            child = node_alloc(node->shift - RADIX_BITS, node, slot);
            if (!child)
                return ENOMEM;
            epoch_publish(&node->slots[slot], child);
            node->count++;
        }
        node = child;
    }

    const size_t slot = slot_of(node, index);
    if (atomic_load(&node->slots[slot], ATOMIC_RELAXED))
        return EEXIST;
    epoch_publish(&node->slots[slot], value);
    node->count++;
    return 0;
}

static void clear_tag_upwards(struct radix_node* node, size_t slot, enum radix_tag tag) {
    while (node) {
        const uint64_t tags = atomic_load(&node->tags[tag], ATOMIC_RELAXED) & ~bit(slot);
        atomic_store(&node->tags[tag], tags, ATOMIC_RELAXED);
        if (tags)
            return;
        slot = node->offset;
        node = node->parent;
    }
}

void* radix_remove(struct radix_tree* tree, uint64_t index) {
    struct radix_node* node = leaf_of(tree, index);
    if (!node)
        return nullptr;

    size_t slot = slot_of(node, index);
    void* value = atomic_load(&node->slots[slot], ATOMIC_RELAXED);
    if (!value)
        return nullptr;

    for (size_t t = 0; t < RADIX_NUM_TAGS; t++) {
        if (atomic_load(&node->tags[t], ATOMIC_RELAXED) & bit(slot))
            clear_tag_upwards(node, slot, t);
    }

    atomic_store(&node->slots[slot], nullptr, ATOMIC_RELAXED);
    node->count--;
    while (node->count == 0) {
        struct radix_node* parent = node->parent;
        if (!parent) {
            atomic_store(&tree->root, nullptr, ATOMIC_RELAXED);
            node_retire(node);
            break;
        }
        slot = node->offset;
        atomic_store(&parent->slots[slot], nullptr, ATOMIC_RELAXED);
        parent->count--;
        node_retire(node);
        node = parent;
    }
    return value;
}

void radix_tag_set(struct radix_tree* tree, uint64_t index, enum radix_tag tag) {
    struct radix_node* node = leaf_of(tree, index);
    if (!node)
        return;
    size_t slot = slot_of(node, index);
    if (!atomic_load(&node->slots[slot], ATOMIC_RELAXED))
        return;
    while (node) {
        const uint64_t tags = atomic_load(&node->tags[tag], ATOMIC_RELAXED);
        if (tags & bit(slot))
            return;
        atomic_store(&node->tags[tag], tags | bit(slot), ATOMIC_RELAXED);
        slot = node->offset;
        node = node->parent;
    }
}

void radix_tag_clear(struct radix_tree* tree, uint64_t index, enum radix_tag tag) {
    struct radix_node* node = leaf_of(tree, index);
    if (!node)
        return;
    const size_t slot = slot_of(node, index);
    if (atomic_load(&node->tags[tag], ATOMIC_RELAXED) & bit(slot))
        clear_tag_upwards(node, slot, tag);
}

bool radix_tag_get(struct radix_tree* tree, uint64_t index, enum radix_tag tag) {
    struct radix_node* node = leaf_of(tree, index);
    return node && (atomic_load(&node->tags[tag], ATOMIC_RELAXED) & bit(slot_of(node, index)));
}

bool radix_tagged(struct radix_tree* tree, enum radix_tag tag) {
    struct radix_node* root = epoch_deref(&tree->root);
    return root && atomic_load(&root->tags[tag], ATOMIC_RELAXED);
}

struct gang {
    size_t max;
    size_t found;
    void** out;
    uint64_t* out_index;
    int tag;
};

static void gang_walk(struct radix_node* node, uint64_t base, uint64_t first, struct gang* g) {
    size_t start = 0;
    if (first > base) {
        const uint64_t delta = (first - base) >> node->shift;
        if (delta >= RADIX_SLOTS)
            return;
        start = delta;
    }

    const uint64_t tags = g->tag >= 0 ? atomic_load(&node->tags[g->tag], ATOMIC_RELAXED) : UINT64_MAX;
    for (size_t i = start; i < RADIX_SLOTS && g->found < g->max; i++) {
        if (!(tags & bit(i)))
            continue;
        void* entry = epoch_deref(&node->slots[i]);
        if (!entry)
            continue;
        const uint64_t index = base + ((uint64_t)i << node->shift);
        if (node->shift == 0) {
            if (g->out)
                g->out[g->found] = entry;
            if (g->out_index)
                g->out_index[g->found] = index;
            g->found++;
        } else {
            gang_walk(entry, index, first > index ? first : index, g);
        }
    }
}

static size_t gang_lookup(struct radix_tree* tree, uint64_t first, int tag, size_t max, void** out, uint64_t* idx) {
    struct radix_node* root = epoch_deref(&tree->root);
    if (!root || max == 0 || !covers(root, first))
        return 0;
    struct gang g = {.max = max, .out = out, .out_index = idx, .tag = tag};
    gang_walk(root, 0, first, &g);
    return g.found;
}

size_t radix_gang_lookup(struct radix_tree* tree, uint64_t first, size_t max, void** out, uint64_t* out_index) {
    return gang_lookup(tree, first, -1, max, out, out_index);
}

size_t radix_gang_lookup_tag(
    struct radix_tree* tree,
    uint64_t first,
    enum radix_tag tag,
    size_t max,
    void** out,
    uint64_t* out_index
) {
    return gang_lookup(tree, first, tag, max, out, out_index);
}
