// SPDX-License-Identifier: BSD-3-Clause

#pragma once

#include <stdint.h>

int64_t atol(const char* str, int base);
uint64_t atolu(const char* str, int base);
char* lutoa(uint64_t value, char* str, int base);
char* ltoa(int64_t value, char* str, int base);
