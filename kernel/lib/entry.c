// SPDX-License-Identifier: BSD-3-Clause

#include <boot/boot.h>
#include <dev/dt/fdt.h>
#include <lib/assert.h>
#include <lib/compiler.h>
#include <lib/init.h>

[[__section(".boot.info")]]
struct boot_info boot_info;

[[__init]]
void kernel_entry() {
    kernel_early_init();

    assert(boot_info.num_mem_maps, "Loader did not provide a memory map");

    // TODO: Maybe move this to the loader so the kernel doesn't have to care.
    if (boot_info.dtb)
        boot_info.dtb_size = fdt_read_u32((const uint8_t*)(boot_info.hhdm_base + boot_info.dtb) + 4);

    kernel_main(&boot_info);
}
