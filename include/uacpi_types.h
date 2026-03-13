#pragma once

#include <stdarg.h>
#include <stddef.h>
#include <stdint.h>

typedef uint8_t uacpi_u8;
typedef uint16_t uacpi_u16;
typedef uint32_t uacpi_u32;
typedef uint64_t uacpi_u64;

typedef int8_t uacpi_i8;
typedef int16_t uacpi_i16;
typedef int32_t uacpi_i32;
typedef int64_t uacpi_i64;

#define UACPI_TRUE  true
#define UACPI_FALSE false
typedef bool uacpi_bool;

#define UACPI_NULL nullptr
#define NULL       nullptr

typedef uintptr_t uacpi_uintptr;
typedef uacpi_uintptr uacpi_virt_addr;
typedef size_t uacpi_size;

typedef va_list uacpi_va_list;
#define uacpi_va_start va_start
#define uacpi_va_end   va_end
#define uacpi_va_arg   va_arg

typedef char uacpi_char;

#define uacpi_offsetof offsetof

#define UACPI_PRIu64     "llu"
#define UACPI_PRIx64     "llx"
#define UACPI_PRIX64     "llX"
#define UACPI_FMT64(val) ((unsigned long long)(val))
