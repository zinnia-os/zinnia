#pragma once

#include <common/compiler.h>
#include <kernel/percpu.h>
#include <stddef.h>

struct usercopy_region {
    void (*start_ip)();
    void (*end_ip)();
    void (*fault_ip)();
};

// Copies a block of data from user to kernel memory.
bool usercopy_read(void* dst, const __user void* src, size_t len);

// Copies a block of data from kernel to user memory.
bool usercopy_write(__user void* dst, const void* src, size_t len);

// Performs a strlen() on a user string.
bool usercopy_strlen(const __user char* str, size_t max, size_t* len);

bool arch_usercopy_read(void* dst, const __user void* src, size_t len, struct usercopy_region** region);
bool arch_usercopy_write(__user void* dst, const void* src, size_t len, struct usercopy_region** region);
bool arch_usercopy_strlen(const __user char* str, size_t max, size_t* len, struct usercopy_region** region);
