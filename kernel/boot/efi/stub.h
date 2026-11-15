// SPDX-License-Identifier: BSD-3-Clause

#pragma once

#include <boot/efi/efi.h>
#include <boot/loader/loader.h>
#include <stddef.h>
#include <stdint.h>

constexpr enum efi_memory_type MEM_STATIC = (enum efi_memory_type)0x8000'0000;
constexpr enum efi_memory_type MEM_RECLAIMABLE = (enum efi_memory_type)0x8000'0001;

extern struct efi_boot_services* bs;
