#pragma once

#include <kernel/init.h>

[[__init]]
void idt_init();

// Loads the IDT on this CPU.
void idt_load();

void interrupt_return();
