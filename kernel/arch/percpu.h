// SPDX-License-Identifier: BSD-3-Clause

#pragma once

#include <lib/percpu.h>

struct percpu* arch_percpu_get();
void arch_percpu_bsp_init();
void arch_percpu_init();
