// SPDX-License-Identifier: BSD-3-Clause

#include <lib/assert.h>
#include <lib/console.h>
#include <lib/init.h>
#include <lib/sched.h>
#include <lib/utils.h>
#include <mem/alloc.h>
#include <mem/page.h>
#include <mem/pmap.h>
#include <mem/vma.h>
#include <stdint.h>
#include <string.h>

static const uint8_t fbcon_font[] = {
#embed "fbcon_font.bin"
};

constexpr size_t FONT_WIDTH = 8;
constexpr size_t FONT_HEIGHT = 12;
constexpr uint32_t FG_COLOR = 0xaaaaaa;
constexpr uint32_t BG_COLOR = 0x000000;

enum esc_state {
    ESC_NONE,
    ESC_START,
    ESC_CSI,
};

static struct {
    struct boot_framebuffer fb;
    uint32_t* back;
    uint32_t fg, bg;
    size_t cols, rows;
    size_t offset_x, offset_y;
    size_t cx, cy;
    enum esc_state esc;
} fbcon;

static inline uint8_t* fb_ptr() {
    return HHDM_PTR(fbcon.fb.base);
}

static inline uint32_t* back_row(size_t y) {
    return fbcon.back + y * fbcon.fb.width;
}

static void flush_rect(size_t x, size_t y, size_t w, size_t h) {
    for (size_t j = 0; j < h; j++)
        memcpy((uint32_t*)(fb_ptr() + (y + j) * fbcon.fb.pitch) + x, back_row(y + j) + x, w * sizeof(uint32_t));
}

static uint32_t pack_color(uint32_t rgb) {
    const struct boot_framebuffer* fb = &fbcon.fb;
    uint32_t r = (rgb >> 16) & 0xff;
    uint32_t g = (rgb >> 8) & 0xff;
    uint32_t b = rgb & 0xff;
    return ((r >> (8 - fb->red.size)) << fb->red.shift) | ((g >> (8 - fb->green.size)) << fb->green.shift) |
        ((b >> (8 - fb->blue.size)) << fb->blue.shift);
}

static void fill_rect(size_t x, size_t y, size_t w, size_t h, uint32_t color) {
    for (size_t j = 0; j < h; j++) {
        uint32_t* row = back_row(y + j) + x;
        for (size_t i = 0; i < w; i++)
            row[i] = color;
    }
    flush_rect(x, y, w, h);
}

static void clear_screen() {
    fill_rect(0, 0, fbcon.fb.width, fbcon.fb.height, fbcon.bg);
    fbcon.cx = fbcon.cy = 0;
}

static void draw_glyph(size_t col, size_t row, uint8_t ch) {
    const uint8_t* glyph = &fbcon_font[ch * FONT_HEIGHT];
    size_t x = fbcon.offset_x + col * FONT_WIDTH;
    size_t y = fbcon.offset_y + row * FONT_HEIGHT;

    for (size_t j = 0; j < FONT_HEIGHT; j++) {
        uint32_t* line = back_row(y + j) + x;
        for (size_t i = 0; i < FONT_WIDTH; i++)
            line[i] = (glyph[j] & (0x80 >> i)) ? fbcon.fg : fbcon.bg;
    }
    flush_rect(x, y, FONT_WIDTH, FONT_HEIGHT);
}

static void scroll_up() {
    const size_t width = fbcon.fb.width;
    const size_t text_y = fbcon.offset_y;
    const size_t text_h = fbcon.rows * FONT_HEIGHT;

    memmove(back_row(text_y), back_row(text_y + FONT_HEIGHT), (text_h - FONT_HEIGHT) * width * sizeof(uint32_t));
    for (size_t j = text_y + text_h - FONT_HEIGHT; j < text_y + text_h; j++) {
        uint32_t* row = back_row(j);
        for (size_t i = 0; i < width; i++)
            row[i] = fbcon.bg;
    }
    flush_rect(0, text_y, width, text_h);
}

static void newline() {
    fbcon.cx = 0;
    if (++fbcon.cy >= fbcon.rows) {
        fbcon.cy = fbcon.rows - 1;
        scroll_up();
    }
}

static void put_char(uint8_t ch) {
    switch (fbcon.esc) {
    case ESC_START:
        fbcon.esc = ch == '[' ? ESC_CSI : ESC_NONE;
        if (ch == 'c')
            clear_screen();
        return;
    case ESC_CSI:
        if (ch >= 0x40 && ch <= 0x7e)
            fbcon.esc = ESC_NONE;
        return;
    case ESC_NONE:
        break;
    }

    switch (ch) {
    case 0x1b:
        fbcon.esc = ESC_START;
        break;
    case '\n':
        newline();
        break;
    case '\r':
        fbcon.cx = 0;
        break;
    case '\b':
        if (fbcon.cx > 0)
            fbcon.cx--;
        break;
    case '\t':
        fbcon.cx = (fbcon.cx + 8) & ~(size_t)7;
        if (fbcon.cx >= fbcon.cols)
            newline();
        break;
    default:
        if (fbcon.cx >= fbcon.cols)
            newline();
        draw_glyph(fbcon.cx++, fbcon.cy, ch);
        break;
    }
}

static void fbcon_write(struct console* con, const char* buf, size_t count) {
    for (size_t i = 0; i < count; i++)
        put_char((uint8_t)buf[i]);
}

// TODO: Make this use a real VT.
static struct console fbcon_con = {
    .name = "fbcon",
    .priority = CONSOLE_PRIO_DISPLAY,
    .write = fbcon_write,
};

[[__init]]
void fbcon_init(struct boot_info* info) {
    const struct boot_framebuffer* fb = &info->fb;
    if (!fb->base)
        return;

    if (fb->bytes_per_pixel != 4 || fb->red.size > 8 || fb->green.size > 8 || fb->blue.size > 8)
        return;

    fbcon.fg = pack_color(FG_COLOR);
    fbcon.bg = pack_color(BG_COLOR);
    fbcon.cols = fb->width / FONT_WIDTH;
    fbcon.rows = fb->height / FONT_HEIGHT;
    fbcon.offset_x = (fb->width % FONT_WIDTH) / 2;
    fbcon.offset_y = (fb->height % FONT_HEIGHT) / 2;
    if (!fbcon.cols || !fbcon.rows)
        return;

    fbcon.fb = *fb;

    const size_t pgsz = page_size();
    const phys_t start = ROUND_DOWN(fb->base, pgsz);
    const phys_t end = ROUND_UP(fb->base + fb->pitch * fb->height, pgsz);

    for (phys_t p = start; p < end; p += pgsz) {
        errno_t status =
            pmap_map(&kernel_vma.pmap, (uintptr_t)HHDM_PTR(p), p, PTE_READ | PTE_WRITE, CACHE_WRITE_COMBINE);
        if (status)
            return;
    }

    fbcon.back = mem_alloc(fb->width * fb->height * sizeof(uint32_t), ALLOC_NOZERO);
    if (!fbcon.back)
        return;

    clear_screen();
    fbcon_con.cols = fbcon.cols;
    fbcon_con.rows = fbcon.rows;
    console_register(&fbcon_con);
}
INIT_TASK(fbcon_stage, "dev.video.fbcon", fbcon_init);
INIT_DEPENDS(fbcon_stage, sched_stage);
