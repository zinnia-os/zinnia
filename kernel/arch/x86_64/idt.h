// SPDX-License-Identifier: BSD-3-Clause

#pragma once

#include <lib/init.h>

[[__init]]
void idt_init();

// Loads the IDT on this CPU.
void idt_load();

void interrupt_return();
