// SPDX-License-Identifier: BSD-3-Clause

#pragma once

#define ckd_add(result, a, b) __builtin_add_overflow(a, b, result)
#define ckd_sub(result, a, b) __builtin_sub_overflow(a, b, result)
#define ckd_mul(result, a, b) __builtin_mul_overflow(a, b, result)
