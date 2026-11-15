// SPDX-License-Identifier: BSD-3-Clause

#pragma once
#include <lib/compiler.h>
#include <lib/errno.h>
#include <lib/initgraph.h>
#include <lib/tailq.h>
#include <lib/types.h>
#include <lib/utils.h>
#include <stdcountof.h>
#include <stddef.h>
#include <stdint.h>

#define MODULE_PROPERTY(name, data) \
    [[__used, __section(".mod." name)]] \
    static const char UNIQUE_IDENT(_name)[] = data

#define MODULE_AUTHOR(author)   MODULE_PROPERTY("author", author)
#define MODULE_DESC(desc)       MODULE_PROPERTY("desc", desc)
#define MODULE_VERSION(version) MODULE_PROPERTY("version", version)
#define MODULE_LICENSE(license) MODULE_PROPERTY("license", license)

struct match_table {
    const char* name;
    const void* entries;
    size_t size;
};

#ifdef __IN_MODULE
#define MATCH_TABLE(match_name, table) \
    [[__used, __section(".mod.match"), __aligned(8)]] \
    static const struct match_table CONCAT(table, _match_table) = { \
        .name = (match_name), \
        .entries = (table), \
        .size = countof(table), \
    }
#else
#define MATCH_TABLE(match_name, table) static_assert(true)
#endif

// Module entries are used
#ifdef __IN_MODULE
#define MODULE_ENTRY(func) \
    [[__used, __visibility("hidden")]] \
    errno_t __module_entry(struct module* __module) { \
        return func(__module); \
    }
#else
#define MODULE_ENTRY(func) \
    [[__used, __section(".mod.entry")]] \
    static errno_t (*const UNIQUE_IDENT(module_entry))(struct module*) = &(func)
#endif

#ifdef __IN_MODULE
#define MODULE_EXIT(func) \
    [[__used, __visibility("hidden")]] \
    errno_t __module_exit(struct module* __module) { \
        return func(__module); \
    } \
    [[__used, __section(".mod.exit")]] \
    static errno_t (*const __exit_fn)(struct module*) = &__module_exit
#else
// Built-in drivers can't be unloaded.
#define MODULE_EXIT(func) static_assert(&(func) != nullptr)
#endif

struct module_segment {
    phys_t phys;
    uintptr_t virt;
    size_t size;
};

struct module {
    const char* name;
    const char* version;
    const char* author;
    const char* desc;
    const char* license;
    uintptr_t base;
    size_t size;
    uintptr_t bias;
    const void* symtab;
    size_t num_syms;
    const char* strtab;
    size_t strtab_size;
    struct module_segment* segments;
    size_t num_segments;
    size_t max_segments;
    errno_t (*exit)(struct module*);
    char* cmdline;
    size_t cmdline_size;
    tailq_link(struct module) link;
};

#define THIS_MODULE (struct module*)nullptr

void module_init();

errno_t module_load(const void* data, size_t length, const char* cmdline, struct module** out);
errno_t module_unload(struct module* mod);
struct module* module_find(const char* name);
