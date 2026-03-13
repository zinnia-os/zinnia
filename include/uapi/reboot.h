#pragma once

#include <stdint.h>

enum reboot_cmd : uint32_t {
    RB_AUTOBOOT = 0xdeadb007,
    RB_HALT_SYSTEM = 0xdeaddead,
    RB_ENABLE_CAD = 0xdeadcad1,
    RB_DISABLE_CAD = 0xdeadcad0,
    RB_POWER_OFF = 0xdead00ff,
    RB_SW_SUSPEND = 0xdead2222,
    RB_KEXEC = 0xdeadecec,
};
