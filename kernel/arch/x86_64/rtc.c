// SPDX-License-Identifier: BSD-3-Clause

#define LOG_FMT(fmt) "x86_64/rtc: " fmt

#include <arch/x86_64/asm.h>
#include <fs/vfs.h>
#include <lib/clock.h>
#include <lib/compiler.h>
#include <lib/cpu.h>
#include <lib/initgraph.h>
#include <lib/percpu.h>
#include <lib/print.h>
#include <lib/time.h>
#include <lib/utils.h>
#include <stdint.h>

constexpr uint16_t CMOS_ADDR = 0x70;
constexpr uint16_t CMOS_DATA = 0x71;

constexpr uint8_t REG_SECONDS = 0x00;
constexpr uint8_t REG_MINUTES = 0x02;
constexpr uint8_t REG_HOURS = 0x04;
constexpr uint8_t REG_DAY = 0x07;
constexpr uint8_t REG_MONTH = 0x08;
constexpr uint8_t REG_YEAR = 0x09;
constexpr uint8_t REG_STATUS_A = 0x0A;
constexpr uint8_t REG_STATUS_B = 0x0B;

constexpr uint8_t STATUS_A_UPDATE_IN_PROGRESS = 1 << 7;
constexpr uint8_t STATUS_B_24H = 1 << 1;
constexpr uint8_t STATUS_B_BINARY = 1 << 2;

struct rtc_time {
    uint8_t seconds;
    uint8_t minutes;
    uint8_t hours;
    uint8_t day;
    uint8_t month;
    uint8_t year;
};

static uint8_t rtc_read(uint8_t reg) {
    asm_outb(CMOS_ADDR, reg & 0x7F);
    return asm_inb(CMOS_DATA);
}

static void rtc_read_raw(struct rtc_time* out) {
    while (rtc_read(REG_STATUS_A) & STATUS_A_UPDATE_IN_PROGRESS) {
        arch_cpu_pause();
    }

    out->seconds = rtc_read(REG_SECONDS);
    out->minutes = rtc_read(REG_MINUTES);
    out->hours = rtc_read(REG_HOURS);
    out->day = rtc_read(REG_DAY);
    out->month = rtc_read(REG_MONTH);
    out->year = rtc_read(REG_YEAR);
}

static bool rtc_time_equal(const struct rtc_time* a, const struct rtc_time* b) {
    return a->seconds == b->seconds && a->minutes == b->minutes && a->hours == b->hours && a->day == b->day &&
        a->month == b->month && a->year == b->year;
}

static uint8_t bcd_to_bin(uint8_t value) {
    return (value & 0x0F) + ((value >> 4) * 10);
}

static bool is_leap_year(uint32_t year) {
    return (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
}

static uint8_t days_in_month(uint32_t year, uint8_t month) {
    static constexpr uint8_t days_in_month[12] = {31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31};

    if (month > 12)
        return -1;

    uint8_t days = days_in_month[month - 1];
    // February has 29 days in a leap year.
    if ((month == 2) && is_leap_year(year))
        return days + 1;

    return days;
}

static bool days_since_epoch(uint32_t year, uint32_t month, uint32_t day, uint64_t* out) {
    if (year < 1970 || month < 1 || month > 12 || day < 1 || day > days_in_month(year, month))
        return false;

    uint64_t days = 0;
    for (uint32_t y = 1970; y < year; y++)
        days += is_leap_year(y) ? 366 : 365;
    for (uint32_t m = 1; m < month; m++)
        days += days_in_month(year, m);
    days += day - 1;

    *out = days;
    return true;
}

static bool rtc_read_unix_ns(uint64_t* out) {
    struct rtc_time last, current;
    rtc_read_raw(&last);
    while (true) {
        rtc_read_raw(&current);
        if (rtc_time_equal(&current, &last))
            break;
        last = current;
    }

    const uint8_t status_b = rtc_read(REG_STATUS_B);
    const bool is_24h = status_b & STATUS_B_24H;
    const bool pm = !is_24h && (current.hours & 0x80);

    uint8_t second = current.seconds;
    uint8_t minute = current.minutes;
    uint8_t hour = current.hours & 0x7F;
    uint8_t day = current.day;
    uint8_t month = current.month;
    uint8_t year = current.year;

    if (!(status_b & STATUS_B_BINARY)) {
        second = bcd_to_bin(second);
        minute = bcd_to_bin(minute);
        hour = bcd_to_bin(hour);
        day = bcd_to_bin(day);
        month = bcd_to_bin(month);
        year = bcd_to_bin(year);
    }

    if (!is_24h) {
        hour %= 12;
        if (pm)
            hour += 12;
    }

    uint64_t days;
    if (!days_since_epoch(2000 + year, month, day, &days))
        return false;

    const uint64_t secs = days * 86'400 + hour * 3'600 + minute * 60 + second;
    *out = secs * 1'000'000'000;
    return true;
}

[[__init]]
static void rtc_init(struct boot_info*) {
    uint64_t ns;
    if (!rtc_read_unix_ns(&ns)) {
        log_warn("Failed to read a sensible time from RTC!\n");
        return;
    }

    log("%lu seconds since the Unix epoch\n", ns / SECS_TO_NS(1));
    clock_set_realtime_ns(ns);
}
INIT_TASK(rtc_stage, "arch.x86_64.rtc", rtc_init);
INIT_DEPENDS(rtc_stage, percpu_stage);
INIT_ENTAILS(rtc_stage, vfs_stage);
