// SPDX-License-Identifier: BSD-3-Clause

#define LOG_FMT(fmt) "init: " fmt

#include <lib/assert.h>
#include <lib/cmdline.h>
#include <lib/initgraph.h>
#include <lib/print.h>

extern struct init_node* const __ld_initgraph_nodes_start[];
extern struct init_node* const __ld_initgraph_nodes_end[];
extern const struct init_edges* const __ld_initgraph_edges_start[];
extern const struct init_edges* const __ld_initgraph_edges_end[];

static bool dump_graph = false;
static void initgraph_option(bool value) {
    dump_graph = value;
}
CMDLINE_OPTION("initgraph", "Print the init graph in DOT format after running", initgraph_option);

static bool edges_linked = false;
static struct init_node* failed_node = nullptr;

struct init_queue {
    struct init_node* head;
    struct init_node* tail;
};

static void queue_push(struct init_queue* queue, struct init_node* node) {
    node->queue_next = nullptr;
    if (queue->tail)
        queue->tail->queue_next = node;
    else
        queue->head = node;
    queue->tail = node;
}

static struct init_node* queue_pop(struct init_queue* queue) {
    struct init_node* node = queue->head;
    if (node) {
        queue->head = node->queue_next;
        if (!queue->head)
            queue->tail = nullptr;
        node->queue_next = nullptr;
    }
    return node;
}

static void link_edges() {
    for (auto cur = __ld_initgraph_edges_start; cur < __ld_initgraph_edges_end; cur++) {
        const struct init_edges* group = *cur;
        for (size_t i = 0; i < group->count; i++) {
            struct init_edge* edge = &group->edges[i];
            edge->source = group->depends ? group->nodes[i] : group->node;
            edge->target = group->depends ? group->node : group->nodes[i];
            edge->propagates_disable = group->depends;

            edge->next_out = edge->source->out_edges;
            edge->source->out_edges = edge;
            edge->next_in = edge->target->in_edges;
            edge->target->in_edges = edge;
            edge->target->unsatisfied++;
        }
    }
    edges_linked = true;
}

static void mark_wanted(struct init_node* goal) {
    if (!goal) {
        for (auto cur = __ld_initgraph_nodes_start; cur < __ld_initgraph_nodes_end; cur++)
            (*cur)->wanted = true;
        return;
    }

    struct init_queue queue = {};

    if (!goal->wanted) {
        goal->wanted = true;
        queue_push(&queue, goal);
    }

    struct init_node* node;
    while ((node = queue_pop(&queue))) {
        for (struct init_edge* edge = node->in_edges; edge; edge = edge->next_in) {
            if (!edge->source->wanted) {
                edge->source->wanted = true;
                queue_push(&queue, edge->source);
            }
        }
    }
}

static bool is_ready(struct init_node* node) {
    return node->wanted && !node->done && node->unsatisfied == 0;
}

static void reach_node(struct init_node* node, struct boot_info* info) {
    if (node->disabled) {
        log_status("Skipping stage \"%s\"\n", node->name);
    } else {
        log_status("Running stage \"%s\"\n", node->name);
        failed_node = node;
        switch (node->action) {
        case INIT_EMPTY:
            break;
        case INIT_CALLBACK:
            node->callback(info);
            break;
        case INIT_GATE:
            if (!node->gate(info)) {
                node->disabled = true;
                node->failed = true;
            }
            break;
        }
        failed_node = nullptr;
        node->executed = true;
    }
    node->done = true;
}

void initgraph_dump() {
    kprintf("digraph initgraph {\n");
    for (auto cur = __ld_initgraph_nodes_start; cur < __ld_initgraph_nodes_end; cur++) {
        struct init_node* node = *cur;
        const char* style = "";
        if (node->failed || node == failed_node)
            style = ", color=red, fontcolor=red";
        else if (!node->executed)
            style = ", color=gray, fontcolor=gray";
        kprintf("\tn%p [label=\"%s\"%s];\n", node, node->name, style);
        for (struct init_edge* edge = node->in_edges; edge; edge = edge->next_in)
            kprintf(
                "\tn%p -> n%p%s;\n",
                edge->source,
                edge->target,
                edge->source->executed && edge->target->executed ? "" : " [color=gray]"
            );
    }
    kprintf("}\n");
}

void initgraph_panic() {
    static bool dumping = false;
    if (!dump_graph || !failed_node || dumping)
        return;
    dumping = true;
    initgraph_dump();
}

void initgraph_run(struct init_node* goal, struct boot_info* info) {
    if (!edges_linked)
        link_edges();

    mark_wanted(goal);

    struct init_queue pending = {};

    for (auto cur = __ld_initgraph_nodes_start; cur < __ld_initgraph_nodes_end; cur++) {
        if (is_ready(*cur))
            queue_push(&pending, *cur);
    }

    struct init_node* node;
    while ((node = queue_pop(&pending))) {
        reach_node(node, info);

        for (struct init_edge* edge = node->out_edges; edge; edge = edge->next_out) {
            struct init_node* successor = edge->target;
            assert(successor->unsatisfied != 0, "Stage \"%s\" has no unsatisfied dependencies left", successor->name);
            successor->unsatisfied--;

            if (node->disabled && edge->propagates_disable)
                successor->disabled = true;

            if (is_ready(successor))
                queue_push(&pending, successor);
        }
    }

    for (auto cur = __ld_initgraph_nodes_start; cur < __ld_initgraph_nodes_end; cur++) {
        if ((*cur)->wanted && !(*cur)->done)
            failed_node = *cur;
        assert(
            !(*cur)->wanted || (*cur)->done,
            "The dependencies for stage \"%s\" could not be resolved",
            (*cur)->name
        );
    }

    if (!goal) {
        log_status("All stages are complete!\n");
        if (dump_graph)
            initgraph_dump();
    }
}
