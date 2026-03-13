#include <kernel/clock.h>
#include <kernel/compiler.h>
#include <kernel/print.h>
#include <kernel/spin.h>
#include <stdatomic.h>
#include <stdint.h>

static struct clock* active_clock = nullptr;
static uint64_t counter_base = 0;
static struct spinlock lock = {};

bool clock_available() {
    return active_clock != nullptr;
}

uint64_t clock_get_elapsed_ns() {
    struct clock* active = atomic_load(&active_clock);
    if (__unlikely(!active))
        return 0;

    return atomic_load(&counter_base) + active->get_elapsed_ns(active);
}

bool clock_switch(struct clock* clock) {
    spin_lock(&lock);

    if (active_clock) {
        if (clock->priority <= active_clock->priority) {
            spin_unlock(&lock);
            return false;
        }
    }

    kprintf("Switching to clock source \"%s\"\n", clock->name);

    uint64_t elapsed = clock_get_elapsed_ns();
    counter_base += elapsed;

    active_clock = clock;
    spin_unlock(&lock);
    return true;
}

void clock_spin_ns(uint64_t ns) {
    if (active_clock == nullptr) {
        kprintf("Unable to sleep for %lu ns, no clock available!\n", ns);
        return;
    }

    uint64_t target = clock_get_elapsed_ns() + ns;
    while (clock_get_elapsed_ns() < target) {
        // TODO: Maybe pause?
    }
}
