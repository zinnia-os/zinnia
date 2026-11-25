// SPDX-License-Identifier: BSD-3-Clause

#pragma once

#include <lib/compiler.h>
#include <mem/usercopy.h>
#include <stddef.h>

void arch_usercopy_allow(bool state);
bool arch_usercopy_read(void* dst, const __user void* src, size_t len, struct usercopy_region** region);
bool arch_usercopy_write(__user void* dst, const void* src, size_t len, struct usercopy_region** region);
bool arch_usercopy_strlen(const __user char* str, size_t max, size_t* len, struct usercopy_region** region);
