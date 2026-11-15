// SPDX-License-Identifier: BSD-3-Clause

#pragma once

#include <boot/boot.h>
#include <lib/compiler.h>
#include <lib/types.h>
#include <lib/utils.h>
#include <config.h>
#include <stddef.h>
#include <stdint.h>

#define __init                  __used, __section(".init.text"), __cold
#define __initdata              __used, __section(".init.data")
#define __initdata_sorted(name) __used, __section(".init.data." name)

extern struct boot_info boot_info;

[[__visibility("hidden")]]
extern uint8_t __ld_stack_top[];
[[__visibility("hidden")]]
extern uint8_t __ld_stack_bottom[];

// Entry point for the kernel after arch-specific setup has finished.
void kernel_entry();

// Initializes the early parts of the kernel.
void kernel_early_init();

[[noreturn]]
void kernel_main(struct boot_info* info);
