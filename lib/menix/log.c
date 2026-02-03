#include <menix/log.h>
#include <menix/status.h>
#include <menix/syscall_numbers.h>
#include <string.h>
#include "syscall_stubs.h"

void menix_log(const char* message) {
    syscall2(MENIX_SYSCALL_LOG, (arg_t)message, (arg_t)strlen(message));
}
