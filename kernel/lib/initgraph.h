// SPDX-License-Identifier: BSD-3-Clause

#pragma once

#include <lib/compiler.h>
#include <lib/utils.h>
#include <stdcountof.h>
#include <stddef.h>

enum init_action {
    INIT_EMPTY,
    INIT_CALLBACK,
    INIT_GATE,
};

struct boot_info;
struct init_node;

struct init_edge {
    struct init_node* source;
    struct init_node* target;
    bool propagates_disable;
    struct init_edge* next_in;
    struct init_edge* next_out;
};

struct init_edges {
    struct init_node* node;
    bool depends;
    bool propagates_disable;
    size_t count;
    struct init_node* const* nodes;
    struct init_edge* edges;
};

struct init_node {
    const char* name;
    enum init_action action;
    union {
        void (*callback)(struct boot_info* info);
        bool (*gate)(struct boot_info* info);
        void* func;
    };
    size_t unsatisfied;
    bool wanted;
    bool done;
    bool disabled;
    bool executed;
    bool failed;
    struct init_edge* in_edges;
    struct init_edge* out_edges;
    struct init_node* queue_next;
};

#define INIT_EXTERN(node) extern struct init_node node[1]

#define INIT_TASK(node, display, ...) \
    struct init_node node[1] = { \
        {.name = (display), \
         __VA_OPT__( \
                 .action = _Generic( \
                     (__VA_ARGS__), \
                     void (*)(struct boot_info*): INIT_CALLBACK, \
                     bool (*)(struct boot_info*): INIT_GATE \
                 ), \
                 .func = (void*)(__VA_ARGS__), \
         )} \
    }; \
    [[__used, __section(".initgraph.nodes")]] \
    static struct init_node* const CONCAT(node, _initgraph_node) = node

#define _INIT_EDGES(x, from, is_depends, propagates, ...) \
    static struct init_node* const CONCAT(x, _nodes)[] = {__VA_ARGS__}; \
    static struct init_edge CONCAT(x, _edges)[countof(CONCAT(x, _nodes))]; \
    static const struct init_edges x = { \
        .node = (from), \
        .depends = (is_depends), \
        .propagates_disable = (propagates), \
        .count = countof(CONCAT(x, _nodes)), \
        .nodes = CONCAT(x, _nodes), \
        .edges = CONCAT(x, _edges), \
    }; \
    [[__used, __section(".initgraph.edges")]] \
    static const struct init_edges* const CONCAT(x, _ptr) = &x

// Make `node` depend on a list of previous stages to run and succeed.
// If one of them fails, `node` is skipped.
#define INIT_DEPENDS(node, ...) _INIT_EDGES(UNIQUE_IDENT(init_edges), node, true, true, __VA_ARGS__)

// Like `INIT_DEPENDS`, but doesn't get skipped if a previous stage fails.
#define INIT_WANTS(node, ...) _INIT_EDGES(UNIQUE_IDENT(init_edges), node, true, false, __VA_ARGS__)

// Make `node` entail a list of next stages that will run after it.
// If `node` fails, nothing gets skipped.
#define INIT_ENTAILS(node, ...) _INIT_EDGES(UNIQUE_IDENT(init_edges), node, false, false, __VA_ARGS__)

void initgraph_run(struct init_node* goal, struct boot_info* info);

void initgraph_panic();
void initgraph_dump();
