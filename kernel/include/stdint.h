// SPDX-License-Identifier: BSD-3-Clause

#pragma once

#ifdef __UINT8_TYPE__
typedef __UINT8_TYPE__ uint8_t;
#endif
#ifdef __UINT16_TYPE__
typedef __UINT16_TYPE__ uint16_t;
#endif
#ifdef __UINT32_TYPE__
typedef __UINT32_TYPE__ uint32_t;
#endif
#ifdef __UINT64_TYPE__
typedef __UINT64_TYPE__ uint64_t;
#endif

typedef __UINT_LEAST8_TYPE__ uint_least8_t;
typedef __UINT_LEAST16_TYPE__ uint_least16_t;
typedef __UINT_LEAST32_TYPE__ uint_least32_t;
typedef __UINT_LEAST64_TYPE__ uint_least64_t;

typedef __UINT_FAST8_TYPE__ uint_fast8_t;
typedef __UINT_FAST16_TYPE__ uint_fast16_t;
typedef __UINT_FAST32_TYPE__ uint_fast32_t;
typedef __UINT_FAST64_TYPE__ uint_fast64_t;

#ifdef __INT8_TYPE__
typedef __INT8_TYPE__ int8_t;
#endif
#ifdef __INT16_TYPE__
typedef __INT16_TYPE__ int16_t;
#endif
#ifdef __INT32_TYPE__
typedef __INT32_TYPE__ int32_t;
#endif
#ifdef __INT64_TYPE__
typedef __INT64_TYPE__ int64_t;
#endif

typedef __INT_LEAST8_TYPE__ int_least8_t;
typedef __INT_LEAST16_TYPE__ int_least16_t;
typedef __INT_LEAST32_TYPE__ int_least32_t;
typedef __INT_LEAST64_TYPE__ int_least64_t;

typedef __INT_FAST8_TYPE__ int_fast8_t;
typedef __INT_FAST16_TYPE__ int_fast16_t;
typedef __INT_FAST32_TYPE__ int_fast32_t;
typedef __INT_FAST64_TYPE__ int_fast64_t;

#ifdef __UINTPTR_TYPE__
typedef __UINTPTR_TYPE__ uintptr_t;
#endif
#ifdef __INTPTR_TYPE__
typedef __INTPTR_TYPE__ intptr_t;
#endif

typedef __UINTMAX_TYPE__ uintmax_t;
typedef __INTMAX_TYPE__ intmax_t;

#ifdef __clang__
#ifndef __STDINT_C_JOIN
#define __STDINT_C_EXPAND_JOIN(x, suffix) x##suffix
#define __STDINT_C_JOIN(x, suffix)        __STDINT_C_EXPAND_JOIN(x, suffix)
#endif

#undef INT8_C
#define INT8_C(x) __STDINT_C_JOIN(x, __INT8_C_SUFFIX__)
#undef INT16_C
#define INT16_C(x) __STDINT_C_JOIN(x, __INT16_C_SUFFIX__)
#undef INT32_C
#define INT32_C(x) __STDINT_C_JOIN(x, __INT32_C_SUFFIX__)
#undef INT64_C
#define INT64_C(x) __STDINT_C_JOIN(x, __INT64_C_SUFFIX__)

#undef UINT8_C
#define UINT8_C(x) __STDINT_C_JOIN(x, __UINT8_C_SUFFIX__)
#undef UINT16_C
#define UINT16_C(x) __STDINT_C_JOIN(x, __UINT16_C_SUFFIX__)
#undef UINT32_C
#define UINT32_C(x) __STDINT_C_JOIN(x, __UINT32_C_SUFFIX__)
#undef UINT64_C
#define UINT64_C(x) __STDINT_C_JOIN(x, __UINT64_C_SUFFIX__)

#undef INTMAX_C
#define INTMAX_C(x) __STDINT_C_JOIN(x, __INTMAX_C_SUFFIX__)
#undef UINTMAX_C
#define UINTMAX_C(x) __STDINT_C_JOIN(x, __UINTMAX_C_SUFFIX__)
#else
#undef INT8_C
#define INT8_C(x) __INT8_C(x)
#undef INT16_C
#define INT16_C(x) __INT16_C(x)
#undef INT32_C
#define INT32_C(x) __INT32_C(x)
#undef INT64_C
#define INT64_C(x) __INT64_C(x)

#undef UINT8_C
#define UINT8_C(x) __UINT8_C(x)
#undef UINT16_C
#define UINT16_C(x) __UINT16_C(x)
#undef UINT32_C
#define UINT32_C(x) __UINT32_C(x)
#undef UINT64_C
#define UINT64_C(x) __UINT64_C(x)

#undef INTMAX_C
#define INTMAX_C(x) __INTMAX_C(x)
#undef UINTMAX_C
#define UINTMAX_C(x) __UINTMAX_C(x)
#endif

#ifdef __UINT8_MAX__
#undef UINT8_MAX
#define UINT8_MAX __UINT8_MAX__
#endif
#ifdef __UINT16_MAX__
#undef UINT16_MAX
#define UINT16_MAX __UINT16_MAX__
#endif
#ifdef __UINT32_MAX__
#undef UINT32_MAX
#define UINT32_MAX __UINT32_MAX__
#endif
#ifdef __UINT64_MAX__
#undef UINT64_MAX
#define UINT64_MAX __UINT64_MAX__
#endif

#ifdef __INT8_MAX__
#undef INT8_MAX
#define INT8_MAX __INT8_MAX__
#endif
#ifdef __INT16_MAX__
#undef INT16_MAX
#define INT16_MAX __INT16_MAX__
#endif
#ifdef __INT32_MAX__
#undef INT32_MAX
#define INT32_MAX __INT32_MAX__
#endif
#ifdef __INT64_MAX__
#undef INT64_MAX
#define INT64_MAX __INT64_MAX__
#endif

#ifdef __INT8_MAX__
#undef INT8_MIN
#define INT8_MIN (-INT8_MAX - 1)
#endif
#ifdef __INT16_MAX__
#undef INT16_MIN
#define INT16_MIN (-INT16_MAX - 1)
#endif
#ifdef __INT32_MAX__
#undef INT32_MIN
#define INT32_MIN (-INT32_MAX - 1)
#endif
#ifdef __INT64_MAX__
#undef INT64_MIN
#define INT64_MIN (-INT64_MAX - 1)
#endif

#undef UINT_LEAST8_MAX
#define UINT_LEAST8_MAX __UINT_LEAST8_MAX__
#undef UINT_LEAST16_MAX
#define UINT_LEAST16_MAX __UINT_LEAST16_MAX__
#undef UINT_LEAST32_MAX
#define UINT_LEAST32_MAX __UINT_LEAST32_MAX__
#undef UINT_LEAST64_MAX
#define UINT_LEAST64_MAX __UINT_LEAST64_MAX__

#undef INT_LEAST8_MAX
#define INT_LEAST8_MAX __INT_LEAST8_MAX__
#undef INT_LEAST16_MAX
#define INT_LEAST16_MAX __INT_LEAST16_MAX__
#undef INT_LEAST32_MAX
#define INT_LEAST32_MAX __INT_LEAST32_MAX__
#undef INT_LEAST64_MAX
#define INT_LEAST64_MAX __INT_LEAST64_MAX__

#undef INT_LEAST8_MIN
#define INT_LEAST8_MIN (-INT_LEAST8_MAX - 1)
#undef INT_LEAST16_MIN
#define INT_LEAST16_MIN (-INT_LEAST16_MAX - 1)
#undef INT_LEAST32_MIN
#define INT_LEAST32_MIN (-INT_LEAST32_MAX - 1)
#undef INT_LEAST64_MIN
#define INT_LEAST64_MIN (-INT_LEAST64_MAX - 1)

#undef UINT_FAST8_MAX
#define UINT_FAST8_MAX __UINT_FAST8_MAX__
#undef UINT_FAST16_MAX
#define UINT_FAST16_MAX __UINT_FAST16_MAX__
#undef UINT_FAST32_MAX
#define UINT_FAST32_MAX __UINT_FAST32_MAX__
#undef UINT_FAST64_MAX
#define UINT_FAST64_MAX __UINT_FAST64_MAX__

#undef INT_FAST8_MAX
#define INT_FAST8_MAX __INT_FAST8_MAX__
#undef INT_FAST16_MAX
#define INT_FAST16_MAX __INT_FAST16_MAX__
#undef INT_FAST32_MAX
#define INT_FAST32_MAX __INT_FAST32_MAX__
#undef INT_FAST64_MAX
#define INT_FAST64_MAX __INT_FAST64_MAX__

#undef INT_FAST8_MIN
#define INT_FAST8_MIN (-INT_FAST8_MAX - 1)
#undef INT_FAST16_MIN
#define INT_FAST16_MIN (-INT_FAST16_MAX - 1)
#undef INT_FAST32_MIN
#define INT_FAST32_MIN (-INT_FAST32_MAX - 1)
#undef INT_FAST64_MIN
#define INT_FAST64_MIN (-INT_FAST64_MAX - 1)

#ifdef __UINTPTR_MAX__
#undef UINTPTR_MAX
#define UINTPTR_MAX __UINTPTR_MAX__
#endif
#ifdef __INTPTR_MAX__
#undef INTPTR_MAX
#define INTPTR_MAX __INTPTR_MAX__
#undef INTPTR_MIN
#define INTPTR_MIN (-INTPTR_MAX - 1)
#endif

#undef UINTMAX_MAX
#define UINTMAX_MAX __UINTMAX_MAX__
#undef INTMAX_MAX
#define INTMAX_MAX __INTMAX_MAX__
#undef INTMAX_MIN
#define INTMAX_MIN (-INTMAX_MAX - 1)

#undef PTRDIFF_MAX
#define PTRDIFF_MAX __PTRDIFF_MAX__
#undef PTRDIFF_MIN
#define PTRDIFF_MIN (-PTRDIFF_MAX - 1)

#undef SIG_ATOMIC_MAX
#define SIG_ATOMIC_MAX __SIG_ATOMIC_MAX__
#undef SIG_ATOMIC_MIN
#define SIG_ATOMIC_MIN (-SIG_ATOMIC_MAX - 1)

#undef SIZE_MAX
#define SIZE_MAX __SIZE_MAX__

#undef WCHAR_MAX
#define WCHAR_MAX __WCHAR_MAX__
#undef WCHAR_MIN
#define WCHAR_MIN (-WCHAR_MAX - 1)

#undef WINT_MAX
#define WINT_MAX __WINT_MAX__
#undef WINT_MIN
#define WINT_MIN (-WINT_MAX - 1)

#ifdef __INT8_TYPE__
#undef INT8_WIDTH
#define INT8_WIDTH 8
#endif
#ifdef __UINT8_TYPE__
#undef UINT8_WIDTH
#define UINT8_WIDTH 8
#endif
#ifdef __INT16_TYPE__
#undef INT16_WIDTH
#define INT16_WIDTH 16
#endif
#ifdef __UINT16_TYPE__
#undef UINT16_WIDTH
#define UINT16_WIDTH 16
#endif
#ifdef __INT32_TYPE__
#undef INT32_WIDTH
#define INT32_WIDTH 32
#endif
#ifdef __UINT32_TYPE__
#undef UINT32_WIDTH
#define UINT32_WIDTH 32
#endif
#ifdef __INT64_TYPE__
#undef INT64_WIDTH
#define INT64_WIDTH 64
#endif
#ifdef __UINT64_TYPE__
#undef UINT64_WIDTH
#define UINT64_WIDTH 64
#endif

#undef INT_LEAST8_WIDTH
#define INT_LEAST8_WIDTH __INT_LEAST8_WIDTH__
#undef UINT_LEAST8_WIDTH
#define UINT_LEAST8_WIDTH __INT_LEAST8_WIDTH__
#undef INT_LEAST16_WIDTH
#define INT_LEAST16_WIDTH __INT_LEAST16_WIDTH__
#undef UINT_LEAST16_WIDTH
#define UINT_LEAST16_WIDTH __INT_LEAST16_WIDTH__
#undef INT_LEAST32_WIDTH
#define INT_LEAST32_WIDTH __INT_LEAST32_WIDTH__
#undef UINT_LEAST32_WIDTH
#define UINT_LEAST32_WIDTH __INT_LEAST32_WIDTH__
#undef INT_LEAST64_WIDTH
#define INT_LEAST64_WIDTH __INT_LEAST64_WIDTH__
#undef UINT_LEAST64_WIDTH
#define UINT_LEAST64_WIDTH __INT_LEAST64_WIDTH__

#undef INT_FAST8_WIDTH
#define INT_FAST8_WIDTH __INT_FAST8_WIDTH__
#undef UINT_FAST8_WIDTH
#define UINT_FAST8_WIDTH __INT_FAST8_WIDTH__
#undef INT_FAST16_WIDTH
#define INT_FAST16_WIDTH __INT_FAST16_WIDTH__
#undef UINT_FAST16_WIDTH
#define UINT_FAST16_WIDTH __INT_FAST16_WIDTH__
#undef INT_FAST32_WIDTH
#define INT_FAST32_WIDTH __INT_FAST32_WIDTH__
#undef UINT_FAST32_WIDTH
#define UINT_FAST32_WIDTH __INT_FAST32_WIDTH__
#undef INT_FAST64_WIDTH
#define INT_FAST64_WIDTH __INT_FAST64_WIDTH__
#undef UINT_FAST64_WIDTH
#define UINT_FAST64_WIDTH __INT_FAST64_WIDTH__

#ifdef __INTPTR_TYPE__
#undef INTPTR_WIDTH
#define INTPTR_WIDTH __INTPTR_WIDTH__
#endif
#ifdef __UINTPTR_TYPE__
#undef UINTPTR_WIDTH
#define UINTPTR_WIDTH __INTPTR_WIDTH__
#endif

#undef INTMAX_WIDTH
#define INTMAX_WIDTH __INTMAX_WIDTH__
#undef UINTMAX_WIDTH
#define UINTMAX_WIDTH __INTMAX_WIDTH__

#undef PTRDIFF_WIDTH
#define PTRDIFF_WIDTH __PTRDIFF_WIDTH__

#undef SIG_ATOMIC_WIDTH
#define SIG_ATOMIC_WIDTH __SIG_ATOMIC_WIDTH__

#undef SIZE_WIDTH
#define SIZE_WIDTH __SIZE_WIDTH__

#undef WCHAR_WIDTH
#define WCHAR_WIDTH __WCHAR_WIDTH__

#undef WINT_WIDTH
#define WINT_WIDTH __WINT_WIDTH__
