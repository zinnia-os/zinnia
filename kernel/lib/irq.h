// SPDX-License-Identifier: BSD-3-Clause

#pragma once

#include <lib/compiler.h>
#include <lib/errno.h>
#include <lib/list.h>
#include <lib/types.h>
#include <stddef.h>
#include <stdint.h>

enum irq_status : uint8_t {
    IRQ_STATUS_IGNORED,
    IRQ_STATUS_HANDLED,
    IRQ_STATUS_WAKE_THREAD,
};

enum irq_polarity : uint8_t {
    IRQ_POLARITY_LOW,
    IRQ_POLARITY_HIGH,
};

enum irq_trigger_mode : uint8_t {
    IRQ_TRIGGER_EDGE,
    IRQ_TRIGGER_LEVEL,
};

enum irq_mode : uint8_t {
    IRQ_MODE_EOI = 1 << 0,
    IRQ_MODE_MASKABLE = 1 << 1,
};

struct irq_line;
struct irq_task;

struct irq_handler {
    enum irq_status (*raise)(struct irq_handler* self);
    void (*thread)(struct irq_handler* self);
    list_link(struct irq_handler) next;

    struct irq_line* line;
    _Atomic(bool) need;
};

struct irq_line {
    enum irq_mode (*set_config)(struct irq_line* self, enum irq_trigger_mode trigger, enum irq_polarity polarity);
    void (*mask)(struct irq_line* self);
    void (*unmask)(struct irq_line* self);
    void (*eoi)(struct irq_line* self);

    bool is_busy;
    list_head(struct irq_handler) handlers;
    enum irq_mode mode;
    enum irq_trigger_mode trigger;
    struct irq_task* task;
};

struct msi_line {
    struct irq_line line;
    phys_t (*msg_addr)(struct msi_line* self);
    uint32_t (*msg_data)(struct msi_line* self);
};

struct irq_percpu {
    size_t depth;
    bool saved_state;
};

extern struct irq_percpu irq_data;

void irq_line_program(struct irq_line* line, enum irq_trigger_mode trigger, enum irq_polarity polarity);
void irq_line_raise(struct irq_line* line);
void irq_line_attach(struct irq_line* line, struct irq_handler* handler);
void irq_line_detach(struct irq_line* line, struct irq_handler* handler);
errno_t irq_line_attach_task(struct irq_line* line, struct irq_handler* handler, const char* name);

void irq_lock();
void irq_unlock();

// Hints to stop execution on this CPU until an interrupt happens.
void irq_wait();
bool irq_get_state();
bool irq_set_state(bool state);
