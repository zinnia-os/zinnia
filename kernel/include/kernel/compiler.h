#pragma once

// These hints may improve code generation, but are optional and can be stubbed.
#if defined(__GNUC__) || defined(__clang__)
#define __cold              gnu::cold
#define __hot               gnu::hot
#define __used              gnu::used
#define __unused            gnu::unused
#define __format(like, ...) gnu::format(like, __VA_ARGS__)
#define __likely(x)         __builtin_expect(!!(x), 1)
#define __unlikely(x)       __builtin_expect(!!(x), 0)
#define __always_inline     gnu::always_inline
#else
#define __cold
#define __hot
#define __used
#define __unused
#define __format(like, ...)
#define __likely(x)   x
#define __unlikely(x) x
#define __always_inline
#endif

// Only clang has these hints, GCC will throw a warning here.
// They also don't support the C23 attribute format.
#ifdef __clang__
#define __user __attribute__((noderef, address_space(1)))
#define __phys __attribute__((noderef, address_space(2)))
#else
#define __user
#define __phys
#endif

// Attributes that affect data layout. Not optional.
#if defined(__GNUC__) || defined(__clang__)
#define __weak       gnu::weak
#define __section(x) gnu::section(x)
#define __aligned(x) gnu::aligned(x)
#define __packed     gnu::packed
#define __naked      gnu::naked
#else
#error "Unsupported compiler!"
#endif

// Intrinsics
#define __unreachable() __builtin_unreachable()
