#pragma once

#include <kernel/types.h>
#include <stddef.h>

void* mmio_new(phys_t addr, size_t length);
void mmio_free(void* ptr, size_t length);
