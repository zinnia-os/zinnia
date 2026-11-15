// SPDX-License-Identifier: BSD-3-Clause

#pragma once

#include <lib/initgraph.h>
#include <stddef.h>
#include <stdint.h>

void ksym_init();

// Finds the function containing `addr`. Returns false if no symbol is known.
bool ksym_lookup(uintptr_t addr, const char** name, size_t* offset);

// Writes `name+0xoffset` for `addr` to `buf`, or `<???>` if the address can't be resolved.
void ksym_format(uintptr_t addr, char* buf, size_t size);

bool ksym_find(const char* name, uintptr_t* addr);

const char* ksym_kernel_name();

INIT_EXTERN(ksyms_stage);
