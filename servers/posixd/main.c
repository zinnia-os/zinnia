#include <menix/channel.h>
#include <menix/handle.h>
#include <menix/log.h>
#include <menix/status.h>

int main() {
    menix_log("Hello from posixd!\n");

    menix_status_t e;

    // Create the root channel so other processes can find each other.
    menix_handle_t end0, end1;
    if ((e = menix_channel_create(0, &end0, &end1)))
        return e;

    return MENIX_OK;
}
