#include <zinnia/channel.h>
#include <zinnia/handle.h>
#include <zinnia/mem.h>
#include <zinnia/status.h>
#include <zinnia/system.h>
#include <common/compiler.h>
#include <stdarg.h>
#include <stdint.h>
#include <stdio.h>
#include <sys/auxv.h>

[[__format(printf, 1, 2)]]
void zn_printf(const char* msg, ...) {
    char buf[1024];

    va_list args;
    va_start(args, msg);
    int32_t len = vsnprintf(buf, sizeof(buf), msg, args);
    va_end(args);

    zn_log(buf, len);
}

int main(int argc, char* argv[], char* envp[]) {
    zn_printf("initd: starting...\n");

    zn_handle_t handle = getauxval(AT_INIT_HANDLE);
    zn_printf("initd: init_handle = %i\n", handle);
}
