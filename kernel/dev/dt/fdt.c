// SPDX-License-Identifier: BSD-3-Clause

#include <dev/dt/fdt.h>
#include <string.h>

uint32_t fdt_read_u32(const void* data) {
    const uint8_t* p = data;
    return (uint32_t)p[0] << 24 | (uint32_t)p[1] << 16 | (uint32_t)p[2] << 8 | p[3];
}

uint64_t fdt_read_u64(const void* data) {
    const uint8_t* p = data;
    return (uint64_t)fdt_read_u32(p) << 32 | fdt_read_u32(p + 4);
}

static bool range_valid(size_t offset, size_t length, size_t total) {
    return offset <= total && length <= total - offset;
}

struct token {
    enum fdt_tag kind;
    size_t next;
    const char* name;
    const void* data;
    size_t length;
};

static bool read_token(struct fdt* tree, size_t offset, struct token* token) {
    if (!range_valid(offset, 4, tree->structs_len) || offset % 4)
        return false;

    *token = (struct token){
        .kind = fdt_read_u32(tree->structs + offset),
        .next = offset + 4,
    };

    if (token->kind == FDT_BEGIN_NODE) {
        token->name = (const char*)tree->structs + token->next;
        size_t left = tree->structs_len - token->next;
        size_t length = strnlen(token->name, left);

        if (length == left)
            return false;

        token->next += (length + 1 + 3) & ~(size_t)3;
    } else if (token->kind == FDT_PROP) {
        if (!range_valid(token->next, 8, tree->structs_len))
            return false;

        token->length = fdt_read_u32(tree->structs + token->next);
        size_t name_offset = fdt_read_u32(tree->structs + token->next + 4);
        token->next += 8;

        if (name_offset >= tree->strings_len || !range_valid(token->next, token->length, tree->structs_len))
            return false;

        token->name = (const char*)tree->strings + name_offset;
        size_t left = tree->strings_len - name_offset;

        if (strnlen(token->name, left) == left || !*token->name)
            return false;

        token->data = tree->structs + token->next;
        token->next += (token->length + 3) & ~(size_t)3;
    } else if (token->kind != FDT_END_NODE && token->kind != FDT_NOP && token->kind != FDT_END) {
        return false;
    }

    return token->next <= tree->structs_len;
}

bool fdt_init(struct fdt* tree, const void* data, size_t length) {
    if (!tree)
        return false;
    *tree = (struct fdt){};

    if (!data || length < sizeof(struct fdt_header))
        return false;

    const uint8_t* blob = data;
    size_t total = fdt_read_u32(blob + 4);
    size_t structs = fdt_read_u32(blob + 8);
    size_t strings = fdt_read_u32(blob + 12);
    size_t reserves = fdt_read_u32(blob + 16);
    size_t strings_len = fdt_read_u32(blob + 32);
    size_t structs_len = fdt_read_u32(blob + 36);
    uint32_t version = fdt_read_u32(blob + 20);
    uint32_t compat = fdt_read_u32(blob + 24);

    if (fdt_read_u32(blob) != FDT_MAGIC || total > length || total < sizeof(struct fdt_header) || version < 17 ||
        compat > 17 || compat > version || structs % 4 || reserves % 8 || structs < sizeof(struct fdt_header) ||
        strings < sizeof(struct fdt_header) || reserves < sizeof(struct fdt_header) ||
        !range_valid(structs, structs_len, total) || !range_valid(strings, strings_len, total) ||
        (structs < strings + strings_len && strings < structs + structs_len))
        return false;

    size_t reserved_end = reserves;

    for (;;) {
        if (!range_valid(reserved_end, 16, total))
            return false;
        bool end = !fdt_read_u64(blob + reserved_end) && !fdt_read_u64(blob + reserved_end + 8);
        reserved_end += 16;
        if (end)
            break;
    }

    if ((reserves < structs + structs_len && structs < reserved_end) ||
        (reserves < strings + strings_len && strings < reserved_end))
        return false;

    struct fdt candidate = {
        .blob = blob,
        .total_size = total,
        .version = version,
        .compat_version = compat,
        .boot_cpuid = fdt_read_u32(blob + 28),
        .structs = blob + structs,
        .structs_len = structs_len,
        .strings = blob + strings,
        .strings_len = strings_len,
    };

    size_t depth = 0;
    bool started = false;
    bool children[64] = {};

    for (size_t offset = 0; offset < structs_len;) {
        struct token token;
        if (!read_token(&candidate, offset, &token))
            return false;
        if (token.kind == FDT_BEGIN_NODE) {
            if (depth == 64 || (!depth && (started || *token.name)) || (depth && !*token.name))
                return false;
            if (depth)
                children[depth - 1] = true;
            children[depth++] = false;
            started = true;
        } else if (token.kind == FDT_END_NODE) {
            if (!depth)
                return false;
            depth--;
        } else if (token.kind == FDT_PROP) {
            if (!depth || children[depth - 1])
                return false;
        } else if (token.kind == FDT_END) {
            if (!started || depth || token.next != structs_len)
                return false;
            *tree = candidate;
            return true;
        }
        offset = token.next;
    }
    return false;
}

struct fdt_node fdt_root(struct fdt* tree) {
    struct token token;
    size_t offset = 0;
    if (tree && tree->blob) {
        while (read_token(tree, offset, &token)) {
            if (token.kind == FDT_BEGIN_NODE)
                return (struct fdt_node){tree, offset};
            if (token.kind != FDT_NOP)
                break;
            offset = token.next;
        }
    }
    return (struct fdt_node){};
}

static struct fdt_node find_child(struct fdt_node node, const char* name, size_t length) {
    struct token token;

    if (!node.tree || !read_token(node.tree, node.offset, &token) || token.kind != FDT_BEGIN_NODE)
        return (struct fdt_node){};

    size_t depth = 0;
    for (size_t offset = token.next; read_token(node.tree, offset, &token); offset = token.next) {
        if (token.kind == FDT_BEGIN_NODE) {
            if (!depth && strlen(token.name) == length && !memcmp(token.name, name, length))
                return (struct fdt_node){node.tree, offset};

            depth++;
        } else if (token.kind == FDT_END_NODE) {
            if (!depth)
                break;

            depth--;
        }
    }
    return (struct fdt_node){};
}

struct fdt_node fdt_find(struct fdt* tree, const char* path) {
    if (!path || *path != '/')
        return (struct fdt_node){};

    struct fdt_node node = fdt_root(tree);
    path++;
    while (*path && node.tree) {
        size_t length = 0;
        while (path[length] && path[length] != '/')
            length++;

        if (!length)
            return (struct fdt_node){};

        node = find_child(node, path, length);
        path += length;

        if (*path == '/')
            path++;
    }

    return node;
}

struct fdt_prop fdt_property(struct fdt_node node, const char* name) {
    struct token token;
    if (!node.tree || !name || !read_token(node.tree, node.offset, &token) || token.kind != FDT_BEGIN_NODE)
        return (struct fdt_prop){};

    for (size_t offset = token.next; read_token(node.tree, offset, &token); offset = token.next) {
        if (token.kind == FDT_PROP && !strcmp(token.name, name))
            return (struct fdt_prop){node.tree, token.data, token.length};

        if (token.kind == FDT_BEGIN_NODE || token.kind == FDT_END_NODE)
            break;
    }

    return (struct fdt_prop){};
}

bool fdt_property_u32(struct fdt_node node, const char* name, uint32_t* value) {
    struct fdt_prop property = fdt_property(node, name);

    if (!property.tree || property.length != 4 || !value)
        return false;

    *value = fdt_read_u32(property.data);

    return true;
}

bool fdt_property_u64(struct fdt_node node, const char* name, uint64_t* value) {
    struct fdt_prop property = fdt_property(node, name);

    if (!property.tree || property.length != 8 || !value)
        return false;

    *value = fdt_read_u64(property.data);

    return true;
}

struct fdt_node fdt_next_node(struct fdt_node node) {
    struct token token;

    if (!node.tree || !read_token(node.tree, node.offset, &token))
        return (struct fdt_node){};

    for (size_t offset = token.next; read_token(node.tree, offset, &token); offset = token.next) {
        if (token.kind == FDT_BEGIN_NODE)
            return (struct fdt_node){node.tree, offset};
        if (token.kind == FDT_END)
            break;
    }

    return (struct fdt_node){};
}

const char* fdt_node_name(struct fdt_node node) {
    struct token token;
    if (!node.tree || !read_token(node.tree, node.offset, &token) || token.kind != FDT_BEGIN_NODE)
        return nullptr;
    return token.name;
}

bool fdt_node_is_compatible(struct fdt_node node, const char* compatible) {
    struct fdt_prop property = fdt_property(node, "compatible");
    const char* data = property.data;

    for (size_t i = 0; i < property.length;) {
        size_t length = strnlen(data + i, property.length - i);

        if (length == property.length - i)
            break;

        if (!strcmp(data + i, compatible))
            return true;

        i += length + 1;
    }

    return false;
}

static uint64_t read_cells(const uint8_t* data, uint32_t cells) {
    uint64_t value = 0;

    for (uint32_t i = 0; i < cells; i++)
        value = value << 32 | fdt_read_u32(data + i * 4);

    return value;
}

bool fdt_node_reg(struct fdt_node node, size_t index, phys_t* addr, size_t* size) {
    if (!node.tree)
        return false;

    struct fdt_node root = fdt_root(node.tree);
    uint32_t addr_cells = 2;
    uint32_t size_cells = 2;
    fdt_property_u32(root, "#address-cells", &addr_cells);
    fdt_property_u32(root, "#size-cells", &size_cells);

    if (addr_cells > 2 || size_cells > 2)
        return false;

    struct fdt_prop reg = fdt_property(node, "reg");
    const size_t stride = (addr_cells + size_cells) * 4;
    if (!reg.tree || index >= reg.length / stride)
        return false;

    const uint8_t* entry = (const uint8_t*)reg.data + index * stride;
    if (addr)
        *addr = read_cells(entry, addr_cells);
    if (size)
        *size = read_cells(entry + addr_cells * 4, size_cells);
    return true;
}

struct fdt_node fdt_find_compatible(struct fdt* tree, const char* compatible) {
    if (!tree || !tree->blob || !compatible)
        return (struct fdt_node){};

    for (struct fdt_node node = fdt_root(tree); node.tree; node = fdt_next_node(node)) {
        if (fdt_node_is_compatible(node, compatible))
            return node;
    }

    return (struct fdt_node){};
}
