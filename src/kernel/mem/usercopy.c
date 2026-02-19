#include <kernel/percpu.h>
#include <kernel/usercopy.h>

bool usercopy_read(void* dst, const __user void* src, size_t len) {
    return arch_usercopy_read(dst, src, len, &percpu_get()->usercopy_region);
}

bool usercopy_write(__user void* dst, const void* src, size_t len) {
    return arch_usercopy_write(dst, src, len, &percpu_get()->usercopy_region);
}

bool usercopy_strlen(const __user char* str, size_t max, size_t* len) {
    return arch_usercopy_strlen(str, max, len, &percpu_get()->usercopy_region);
}
