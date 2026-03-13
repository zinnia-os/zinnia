#pragma once

#include <stdint.h>
#include <uapi/types.h>

constexpr uint8_t DT_UNKNOWN = 0;
constexpr uint8_t DT_FIFO = 1;
constexpr uint8_t DT_CHR = 2;
constexpr uint8_t DT_DIR = 4;
constexpr uint8_t DT_BLK = 6;
constexpr uint8_t DT_REG = 8;
constexpr uint8_t DT_LNK = 10;
constexpr uint8_t DT_SOCK = 12;
constexpr uint8_t DT_WHT = 14;

struct dirent {
    ino_t d_ino;
    off_t d_off;
    uint16_t d_reclen;
    uint8_t d_type;
    uint8_t d_name[1024];
};

static_assert(sizeof(struct dirent) == 1048);
