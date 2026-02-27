#include <zinnia/channel.h>
#include <zinnia/handle.h>
#include <zinnia/mem.h>
#include <zinnia/status.h>
#include <zinnia/system.h>
#include <stdint.h>
#include <sys/auxv.h>

void server_main(zn_handle_t handle) {
    zn_log("Hello, init world!\n", 19);
    zn_log("Hello, foo world!\n", 18);
    zn_log("Hello, fo1 world!\n", 18);
    zn_log("Hello, fo2 world!\n", 18);
    zn_log("Hello, fo3 world!\n", 18);
    zn_log("Hello, fo4 world!\n", 18);
    zn_log("Hello, fo5 world!\n", 18);
    zn_log("Hello, fo6 world!\n", 18);

    uintptr_t info_ptr = 0;
    zn_status_t status =
        zn_vmo_map(handle, ZN_HANDLE_THIS_VAS, 0, &info_ptr, sizeof(int), ZN_VM_MAP_READ | ZN_VM_MAP_WRITE);
}
