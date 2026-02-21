#include <zinnia/channel.h>
#include <zinnia/handle.h>
#include <zinnia/status.h>
#include <zinnia/system.h>
#include <stdarg.h>
#include <stdio.h>

void zn_printf(const char* msg, ...) {
    char buf[1024];

    va_list args;
    va_start(args, msg);
    int len = vsnprintf(buf, sizeof(buf), msg, args);
    va_end(args);

    zn_log(buf, len);
}

int main() {
    zn_printf("Hello, init world!\n");

    // Create the root channel so other processes can find each other.
    zn_status_t e;
    zn_handle_t end0, end1;
    zn_printf("init: Creating root channel\n");
    if ((e = zn_channel_create(0, &end0, &end1))) {
        zn_printf("%s\n", zn_status_to_string(e));
        return e;
    }

    return 0;
}
