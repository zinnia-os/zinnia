// SPDX-License-Identifier: BSD-3-Clause

#pragma once

#include <stdint.h>

#define EFIAPI __attribute__((ms_abi))

constexpr uint16_t EFI_ARCH_PE_MACHINE = 0x8664;
