// SPDX-License-Identifier: BSD-3-Clause

#include <lib/percpu.h>
#include <lib/unwind.h>
#include <lib/utils.h>
#include <mem/layout.h>
#include <stdckdint.h>
#include <stdcountof.h>
#include <string.h>

[[__visibility("hidden")]]
extern const uint8_t __ld_eh_frame_hdr_start[];
[[__visibility("hidden")]]
extern const uint8_t __ld_eh_frame_hdr_end[];
[[__visibility("hidden")]]
extern const uint8_t __ld_eh_frame_start[];
[[__visibility("hidden")]]
extern const uint8_t __ld_eh_frame_end[];
[[__visibility("hidden")]]
extern const uint8_t __ld_kernel_start[];
[[__visibility("hidden")]]
extern const uint8_t __ld_kernel_end[];
[[__visibility("hidden")]]
extern uint8_t __ld_stack_bottom[];
[[__visibility("hidden")]]
extern uint8_t __ld_stack_top[];

#define REG_BIT(r) (1ull << (r))

// DW_EH_PE_*
enum : uint8_t {
    PE_ABSPTR = 0x00,
    PE_ULEB128 = 0x01,
    PE_UDATA2 = 0x02,
    PE_UDATA4 = 0x03,
    PE_UDATA8 = 0x04,
    PE_SLEB128 = 0x09,
    PE_SDATA2 = 0x0a,
    PE_SDATA4 = 0x0b,
    PE_SDATA8 = 0x0c,
    PE_FORMAT_MASK = 0x0f,

    PE_PCREL = 0x10,
    PE_DATAREL = 0x30,
    PE_APP_MASK = 0x70,

    PE_INDIRECT = 0x80,
    PE_OMIT = 0xff,
};

// DW_CFA_*
enum : uint8_t {
    CFA_ADVANCE_LOC = 0x40,
    CFA_OFFSET = 0x80,
    CFA_RESTORE = 0xc0,

    CFA_NOP = 0x00,
    CFA_SET_LOC = 0x01,
    CFA_ADVANCE_LOC1 = 0x02,
    CFA_ADVANCE_LOC2 = 0x03,
    CFA_ADVANCE_LOC4 = 0x04,
    CFA_OFFSET_EXTENDED = 0x05,
    CFA_RESTORE_EXTENDED = 0x06,
    CFA_UNDEFINED = 0x07,
    CFA_SAME_VALUE = 0x08,
    CFA_REGISTER = 0x09,
    CFA_REMEMBER_STATE = 0x0a,
    CFA_RESTORE_STATE = 0x0b,
    CFA_DEF_CFA = 0x0c,
    CFA_DEF_CFA_REGISTER = 0x0d,
    CFA_DEF_CFA_OFFSET = 0x0e,
    CFA_DEF_CFA_EXPRESSION = 0x0f,
    CFA_EXPRESSION = 0x10,
    CFA_OFFSET_EXTENDED_SF = 0x11,
    CFA_DEF_CFA_SF = 0x12,
    CFA_DEF_CFA_OFFSET_SF = 0x13,
    CFA_VAL_OFFSET = 0x14,
    CFA_VAL_OFFSET_SF = 0x15,
    CFA_VAL_EXPRESSION = 0x16,
    // DW_CFA_AARCH64_negate_ra_state on aarch64.
    // TODO: We don't sign return addresses, this can be ignored for now.
    CFA_GNU_WINDOW_SAVE = 0x2d,
    CFA_GNU_ARGS_SIZE = 0x2e,
    CFA_GNU_NEGATIVE_OFFSET_EXTENDED = 0x2f,
};

// DW_OP_*
enum : uint8_t {
    OP_ADDR = 0x03,
    OP_DEREF = 0x06,
    OP_CONST1U = 0x08,
    OP_CONST1S = 0x09,
    OP_CONST2U = 0x0a,
    OP_CONST2S = 0x0b,
    OP_CONST4U = 0x0c,
    OP_CONST4S = 0x0d,
    OP_CONST8U = 0x0e,
    OP_CONST8S = 0x0f,
    OP_CONSTU = 0x10,
    OP_CONSTS = 0x11,
    OP_DUP = 0x12,
    OP_DROP = 0x13,
    OP_OVER = 0x14,
    OP_PICK = 0x15,
    OP_SWAP = 0x16,
    OP_ROT = 0x17,
    OP_ABS = 0x19,
    OP_AND = 0x1a,
    OP_DIV = 0x1b,
    OP_MINUS = 0x1c,
    OP_MOD = 0x1d,
    OP_MUL = 0x1e,
    OP_NEG = 0x1f,
    OP_NOT = 0x20,
    OP_OR = 0x21,
    OP_PLUS = 0x22,
    OP_PLUS_UCONST = 0x23,
    OP_SHL = 0x24,
    OP_SHR = 0x25,
    OP_SHRA = 0x26,
    OP_XOR = 0x27,
    OP_BRA = 0x28,
    OP_EQ = 0x29,
    OP_GE = 0x2a,
    OP_GT = 0x2b,
    OP_LE = 0x2c,
    OP_LT = 0x2d,
    OP_NE = 0x2e,
    OP_SKIP = 0x2f,
    OP_LIT0 = 0x30,
    OP_LIT31 = 0x4f,
    OP_BREG0 = 0x70,
    OP_BREG31 = 0x8f,
    OP_BREGX = 0x92,
    OP_DEREF_SIZE = 0x94,
    OP_NOP = 0x96,
};

struct cursor {
    const uint8_t* pos;
    const uint8_t* end;
    bool error;
};

static const uint8_t* cursor_take(struct cursor* c, size_t n) {
    if (c->error || (size_t)(c->end - c->pos) < n) {
        c->error = true;
        return nullptr;
    }
    const uint8_t* p = c->pos;
    c->pos += n;
    return p;
}

// TODO: Support big endian
static uint64_t read_fixed(struct cursor* c, size_t n) {
    uint64_t value = 0;
    const uint8_t* p = cursor_take(c, n);
    if (p != nullptr)
        memcpy(&value, p, n);
    return value;
}

static uint8_t read_byte(struct cursor* c) {
    return read_fixed(c, 1);
}

static uint64_t read_uleb(struct cursor* c) {
    uint64_t result = 0;
    unsigned shift = 0;
    const uint8_t* p;
    do {
        p = cursor_take(c, 1);
        if (p == nullptr)
            return 0;
        if (shift < 64)
            result |= (uint64_t)(*p & 0x7f) << shift;
        shift += 7;
    } while (*p & 0x80);
    return result;
}

static int64_t read_sleb(struct cursor* c) {
    uint64_t result = 0;
    unsigned shift = 0;
    const uint8_t* p;
    do {
        p = cursor_take(c, 1);
        if (p == nullptr)
            return 0;
        if (shift < 64)
            result |= (uint64_t)(*p & 0x7f) << shift;
        shift += 7;
    } while (*p & 0x80);
    if (shift < 64 && (*p & 0x40))
        result |= ~0ull << shift;
    return (int64_t)result;
}

static bool in_range(uintptr_t addr, size_t size, uintptr_t base, uintptr_t top) {
    uintptr_t end;
    return !ckd_add(&end, addr, size) && addr >= base && end <= top;
}

static bool read_mem(const struct unwind_frame* frame, uintptr_t addr, void* out, size_t size) {
    if (!in_range(addr, size, frame->stack_base, frame->stack_top))
        return false;
    memcpy(out, (const void*)addr, size);
    return true;
}

static bool read_word(const struct unwind_frame* frame, uintptr_t addr, uintptr_t* out) {
    if (addr % alignof(uintptr_t))
        return false;
    return read_mem(frame, addr, out, sizeof(*out));
}

static bool use_stack(struct unwind_frame* frame, uintptr_t sp, uintptr_t base, size_t size) {
    if (base == 0 || sp < base || sp - base > size)
        return false;
    frame->stack_base = base;
    frame->stack_top = base + size;
    return true;
}

void unwind_find_stack(struct unwind_frame* frame) {
    frame->stack_base = 0;
    frame->stack_top = 0;
    if (!(frame->valid & REG_BIT(UNWIND_REG_SP)))
        return;

    const uintptr_t sp = frame->regs[UNWIND_REG_SP];
    if (use_stack(frame, sp, (uintptr_t)__ld_stack_bottom, __ld_stack_top - __ld_stack_bottom))
        return;

    struct percpu* cpu = percpu_get();
    struct task* current = cpu->sched.current;
    if (current && use_stack(frame, sp, current->kernel_stack.base, current->kernel_stack.size))
        return;
    use_stack(frame, sp, cpu->boot_stack.base, cpu->boot_stack.size);
}

// Reads a pointer with the given DW_EH_PE encoding.
// `datarel` is the base for DW_EH_PE_datarel.
static uintptr_t read_encoded(struct cursor* c, uint8_t enc, uintptr_t datarel) {
    const uintptr_t field = (uintptr_t)c->pos;
    uint64_t value;

    switch (enc & PE_FORMAT_MASK) {
    case PE_ABSPTR:
        value = read_fixed(c, sizeof(uintptr_t));
        break;
    case PE_ULEB128:
        value = read_uleb(c);
        break;
    case PE_UDATA2:
        value = read_fixed(c, 2);
        break;
    case PE_UDATA4:
        value = read_fixed(c, 4);
        break;
    case PE_UDATA8:
        value = read_fixed(c, 8);
        break;
    case PE_SLEB128:
        value = read_sleb(c);
        break;
    case PE_SDATA2:
        value = (int16_t)read_fixed(c, 2);
        break;
    case PE_SDATA4:
        value = (int32_t)read_fixed(c, 4);
        break;
    case PE_SDATA8:
        value = read_fixed(c, 8);
        break;
    default:
        c->error = true;
        return 0;
    }

    switch (enc & PE_APP_MASK) {
    case 0:
        break;
    case PE_PCREL:
        value += field;
        break;
    case PE_DATAREL:
        if (datarel == 0)
            c->error = true;
        value += datarel;
        break;
    default:
        c->error = true;
        return 0;
    }

    if (enc & PE_INDIRECT) {
        if (c->error || value % alignof(uintptr_t) ||
            !in_range(value, sizeof(uintptr_t), (uintptr_t)__ld_kernel_start, (uintptr_t)__ld_kernel_end)) {
            c->error = true;
            return 0;
        }
        memcpy(&value, (const void*)value, sizeof(uintptr_t));
    }
    return value;
}

struct cie {
    uint64_t code_align;
    int64_t data_align;
    uint64_t ra_reg;
    uint8_t fde_enc;
    bool has_aug_data;
    bool signal_frame;
    struct cursor insns;
};

struct fde {
    uintptr_t pc_begin;
    uintptr_t pc_end;
    struct cursor insns;
};

// Reads the heaedr of the .eh_frame entry at `pos` and returns a cursor over its body, starting at the ID.
static bool read_entry(const uint8_t* pos, struct cursor* body) {
    struct cursor c = {.pos = pos, .end = __ld_eh_frame_end};
    const uint32_t length = read_fixed(&c, 4);

    // TODO: We don't support the 64-bit DWARF format.
    if (c.error || length < 4 || length == 0xffff'ffff || length > (size_t)(c.end - c.pos))
        return false;

    *body = (struct cursor){.pos = c.pos, .end = c.pos + length};
    return true;
}

static bool parse_fde(const uint8_t* pos, struct cie* cie, struct fde* fde) {
    struct cursor c;
    if (!read_entry(pos, &c))
        return false;

    const uint8_t* id_pos = c.pos;
    const uint32_t cie_offset = read_fixed(&c, 4);
    if (c.error || cie_offset == 0 || cie_offset > (size_t)(id_pos - __ld_eh_frame_start))
        return false;

    // Parse the CIE that this FDE belongs to.
    struct cursor cie_body;
    if (!read_entry(id_pos - cie_offset, &cie_body) || read_fixed(&cie_body, 4) != 0)
        return false;

    const uint8_t version = read_byte(&cie_body);
    if (version != 1 && version != 3)
        return false;

    const char* aug = (const char*)cie_body.pos;
    while (!cie_body.error && read_byte(&cie_body) != 0)
        ;

    *cie = (struct cie){.fde_enc = PE_ABSPTR};
    cie->code_align = read_uleb(&cie_body);
    cie->data_align = read_sleb(&cie_body);
    cie->ra_reg = version == 1 ? read_byte(&cie_body) : read_uleb(&cie_body);
    if (cie_body.error)
        return false;

    if (aug[0] == 'z') {
        cie->has_aug_data = true;
        const uint64_t aug_len = read_uleb(&cie_body);
        struct cursor data = {.pos = cie_body.pos};
        if (cursor_take(&cie_body, aug_len) == nullptr)
            return false;
        data.end = cie_body.pos;

        for (const char* a = aug + 1; *a; a++) {
            switch (*a) {
            case 'R':
                cie->fde_enc = read_byte(&data);
                break;
            case 'L':
                read_byte(&data);
                break;
            case 'P':
                // Skip the personality routine.
                // Ignore DW_EH_PE_indirect so we don'tr follow the pointer.
                read_encoded(&data, read_byte(&data) & ~PE_INDIRECT, 0);
                break;
            case 'S':
                cie->signal_frame = true;
                break;
            case 'B': // TODO: aarch64 pointer authentication with the B key.
            case 'G': // TODO: aarch64 memory tagging.
                break;
            default:
                // The remaining augmentation data can be skipped thanks to the length prefix.
                goto done;
            }
        }
done:
        if (data.error)
            return false;
    } else if (aug[0] != 0) {
        // Without the 'z' augmentation we can't skip unknown augmentation data.
        return false;
    }

    cie->insns = cie_body;

    fde->pc_begin = read_encoded(&c, cie->fde_enc, 0);
    const uintptr_t range = read_encoded(&c, cie->fde_enc & PE_FORMAT_MASK, 0);
    if (cie->has_aug_data)
        cursor_take(&c, read_uleb(&c));
    if (c.error || ckd_add(&fde->pc_end, fde->pc_begin, range))
        return false;

    fde->insns = c;
    return true;
}

enum rule_kind : uint8_t {
    RULE_SAME = 0,   // The register wasn't modified.
    RULE_UNDEF,      // The register is undefined.
    RULE_OFFSET,     // Saved at CFA + value.
    RULE_VAL_OFFSET, // Value is CFA + value.
    RULE_REG,        // Saved in register `rule::value`.
    RULE_EXPR,       // Saved at the address computed by the expression at offset `rule::value` in .eh_frame.
    RULE_VAL_EXPR,   // Value is computed by the expression at offset `rule::value` in .eh_frame.
};

// Note: Keep this small, since it's mostly used during panics and the kernel
// may be starved of memory at that point.
struct rule {
    int32_t value;
    enum rule_kind kind;
};

struct cfi_row {
    struct rule regs[UNWIND_NUM_REGS];
    struct rule cfa;    // Either RULE_REG or RULE_EXPR.
    int64_t cfa_offset; // Used if cfa == RULE_REG.
};

// Amount of remembered states per CFA program.
constexpr size_t STATE_STACK_DEPTH = 3;

// Skips an expression block and returns its offset in .eh_frame.
static int32_t take_expr(struct cursor* c) {
    const uint8_t* start = c->pos;
    cursor_take(c, read_uleb(c));

    if (start < __ld_eh_frame_start || start >= __ld_eh_frame_end)
        c->error = true;

    return start - __ld_eh_frame_start;
}

static void set_rule(struct cfi_row* row, uint64_t reg, enum rule_kind kind, int64_t value, struct cursor* c) {
    // Make sure the value fits inside an int32_t.
    if (!(value >= INT32_MIN && value <= INT32_MAX)) {
        c->error = true;
        return;
    }

    // We don't care about registers we don't track to begin with.
    if (reg < UNWIND_NUM_REGS)
        row->regs[reg] = (struct rule){.kind = kind, .value = value};
}

// Executes call frame instructions until the location moves past `target`.
// `initial` is the row after the CIE instructions.
static bool run_cfa_program(
    const struct cie* cie,
    struct cursor c,
    uintptr_t loc,
    uintptr_t target,
    struct cfi_row* row,
    const struct cfi_row* initial
) {
    struct cfi_row stack[STATE_STACK_DEPTH];
    size_t depth = 0;

    while (!c.error && c.pos < c.end) {
        const uint8_t insn = read_byte(&c);
        const uint8_t low = insn & 0x3f;
        uint64_t delta = 0;
        uint64_t reg;

        switch (insn & 0xc0) {
        case CFA_ADVANCE_LOC:
            delta = low * cie->code_align;
            goto advance;
        case CFA_OFFSET:
            set_rule(row, low, RULE_OFFSET, (int64_t)read_uleb(&c) * cie->data_align, &c);
            continue;
        case CFA_RESTORE:
            if (initial == nullptr)
                return false;
            if (low < UNWIND_NUM_REGS)
                row->regs[low] = initial->regs[low];
            continue;
        }

        switch (insn) {
        case CFA_NOP:
            break;
        case CFA_SET_LOC:
            loc = read_encoded(&c, cie->fde_enc, 0);
            if (loc > target)
                return !c.error;
            break;
        case CFA_ADVANCE_LOC1:
            delta = read_fixed(&c, 1) * cie->code_align;
            goto advance;
        case CFA_ADVANCE_LOC2:
            delta = read_fixed(&c, 2) * cie->code_align;
            goto advance;
        case CFA_ADVANCE_LOC4:
            delta = read_fixed(&c, 4) * cie->code_align;
            goto advance;
        case CFA_OFFSET_EXTENDED:
            reg = read_uleb(&c);
            set_rule(row, reg, RULE_OFFSET, (int64_t)read_uleb(&c) * cie->data_align, &c);
            break;
        case CFA_OFFSET_EXTENDED_SF:
            reg = read_uleb(&c);
            set_rule(row, reg, RULE_OFFSET, read_sleb(&c) * cie->data_align, &c);
            break;
        case CFA_GNU_NEGATIVE_OFFSET_EXTENDED:
            reg = read_uleb(&c);
            set_rule(row, reg, RULE_OFFSET, -(int64_t)read_uleb(&c) * cie->data_align, &c);
            break;
        case CFA_VAL_OFFSET:
            reg = read_uleb(&c);
            set_rule(row, reg, RULE_VAL_OFFSET, (int64_t)read_uleb(&c) * cie->data_align, &c);
            break;
        case CFA_VAL_OFFSET_SF:
            reg = read_uleb(&c);
            set_rule(row, reg, RULE_VAL_OFFSET, read_sleb(&c) * cie->data_align, &c);
            break;
        case CFA_RESTORE_EXTENDED:
            reg = read_uleb(&c);
            if (initial == nullptr)
                return false;
            if (reg < UNWIND_NUM_REGS)
                row->regs[reg] = initial->regs[reg];
            break;
        case CFA_UNDEFINED:
            set_rule(row, read_uleb(&c), RULE_UNDEF, 0, &c);
            break;
        case CFA_SAME_VALUE:
            set_rule(row, read_uleb(&c), RULE_SAME, 0, &c);
            break;
        case CFA_REGISTER:
            reg = read_uleb(&c);
            set_rule(row, reg, RULE_REG, read_uleb(&c), &c);
            break;
        case CFA_EXPRESSION:
            reg = read_uleb(&c);
            set_rule(row, reg, RULE_EXPR, take_expr(&c), &c);
            break;
        case CFA_VAL_EXPRESSION:
            reg = read_uleb(&c);
            set_rule(row, reg, RULE_VAL_EXPR, take_expr(&c), &c);
            break;
        case CFA_REMEMBER_STATE:
            if (depth == STATE_STACK_DEPTH)
                return false;
            stack[depth++] = *row;
            break;
        case CFA_RESTORE_STATE:
            if (depth == 0)
                return false;
            *row = stack[--depth];
            break;
        case CFA_DEF_CFA:
            row->cfa = (struct rule){.kind = RULE_REG, .value = read_uleb(&c)};
            row->cfa_offset = read_uleb(&c);
            break;
        case CFA_DEF_CFA_SF:
            row->cfa = (struct rule){.kind = RULE_REG, .value = read_uleb(&c)};
            row->cfa_offset = read_sleb(&c) * cie->data_align;
            break;
        case CFA_DEF_CFA_REGISTER:
            if (row->cfa.kind != RULE_REG)
                return false;
            row->cfa.value = read_uleb(&c);
            break;
        case CFA_DEF_CFA_OFFSET:
            if (row->cfa.kind != RULE_REG)
                return false;
            row->cfa_offset = read_uleb(&c);
            break;
        case CFA_DEF_CFA_OFFSET_SF:
            if (row->cfa.kind != RULE_REG)
                return false;
            row->cfa_offset = read_sleb(&c) * cie->data_align;
            break;
        case CFA_DEF_CFA_EXPRESSION:
            row->cfa = (struct rule){.kind = RULE_EXPR, .value = take_expr(&c)};
            break;
        case CFA_GNU_WINDOW_SAVE:
            break;
        case CFA_GNU_ARGS_SIZE:
            read_uleb(&c);
            break;
        default:
            return false;
        }
        continue;

advance:
        if (ckd_add(&loc, loc, delta) || loc > target)
            return !c.error;
    }
    return !c.error;
}

static bool get_reg(const struct unwind_frame* frame, uint64_t reg, uintptr_t* out) {
    if (reg >= UNWIND_NUM_REGS || !(frame->valid & REG_BIT(reg)))
        return false;
    *out = frame->regs[reg];
    return true;
}

// Evaluates the DWARF expression at offset `expr` in .eh_frame.
static bool eval_expr(int32_t expr, const struct unwind_frame* frame, const uintptr_t* initial, uintptr_t* out) {
    if (expr < 0 || expr >= __ld_eh_frame_end - __ld_eh_frame_start)
        return false;

    struct cursor c = {.pos = __ld_eh_frame_start + expr, .end = __ld_eh_frame_end};
    const uint64_t len = read_uleb(&c);
    const uint8_t* block = cursor_take(&c, len);
    if (block == nullptr)
        return false;
    c = (struct cursor){.pos = block, .end = block + len};

    uintptr_t stack[16];
    size_t sp = 0;

#define PUSH(v) \
    do { \
        const uintptr_t pushed = (v); \
        if (sp == countof(stack)) \
            return false; \
        stack[sp++] = pushed; \
    } while (0)

#define NEED(n) \
    do { \
        if (sp < (n)) \
            return false; \
    } while (0)

#define BINOP(expr) \
    do { \
        NEED(2); \
        const uintptr_t b = stack[--sp]; \
        const uintptr_t a = stack[sp - 1]; \
        stack[sp - 1] = (expr); \
    } while (0)

    if (initial != nullptr)
        PUSH(*initial);

    while (!c.error && c.pos < c.end) {
        const uint8_t op = read_byte(&c);
        uintptr_t value;
        int64_t offset;

        if (op >= OP_LIT0 && op <= OP_LIT31) {
            PUSH(op - OP_LIT0);
            continue;
        }
        if (op >= OP_BREG0 && op <= OP_BREG31) {
            offset = read_sleb(&c);
            if (!get_reg(frame, op - OP_BREG0, &value))
                return false;
            PUSH(value + offset);
            continue;
        }

        switch (op) {
        case OP_ADDR:
            PUSH(read_fixed(&c, sizeof(uintptr_t)));
            break;
        case OP_CONST1U:
            PUSH(read_fixed(&c, 1));
            break;
        case OP_CONST1S:
            PUSH((int8_t)read_fixed(&c, 1));
            break;
        case OP_CONST2U:
            PUSH(read_fixed(&c, 2));
            break;
        case OP_CONST2S:
            PUSH((int16_t)read_fixed(&c, 2));
            break;
        case OP_CONST4U:
            PUSH(read_fixed(&c, 4));
            break;
        case OP_CONST4S:
            PUSH((int32_t)read_fixed(&c, 4));
            break;
        case OP_CONST8U:
        case OP_CONST8S:
            PUSH(read_fixed(&c, 8));
            break;
        case OP_CONSTU:
            PUSH(read_uleb(&c));
            break;
        case OP_CONSTS:
            PUSH(read_sleb(&c));
            break;
        case OP_DUP:
            NEED(1);
            PUSH(stack[sp - 1]);
            break;
        case OP_DROP:
            NEED(1);
            sp--;
            break;
        case OP_OVER:
            NEED(2);
            PUSH(stack[sp - 2]);
            break;
        case OP_PICK:
            value = read_byte(&c);
            NEED(value + 1);
            PUSH(stack[sp - 1 - value]);
            break;
        case OP_SWAP:
            NEED(2);
            value = stack[sp - 1];
            stack[sp - 1] = stack[sp - 2];
            stack[sp - 2] = value;
            break;
        case OP_ROT:
            NEED(3);
            value = stack[sp - 1];
            stack[sp - 1] = stack[sp - 2];
            stack[sp - 2] = stack[sp - 3];
            stack[sp - 3] = value;
            break;
        case OP_DEREF:
            NEED(1);
            if (!read_word(frame, stack[sp - 1], &stack[sp - 1]))
                return false;
            break;
        case OP_DEREF_SIZE:
            value = read_byte(&c);
            NEED(1);
            if (value == 0 || value > sizeof(uintptr_t))
                return false;
            uintptr_t word = 0;
            if (!read_mem(frame, stack[sp - 1], &word, value))
                return false;
            stack[sp - 1] = word;
            break;
        case OP_ABS:
            NEED(1);
            if ((intptr_t)stack[sp - 1] < 0)
                stack[sp - 1] = -stack[sp - 1];
            break;
        case OP_NEG:
            NEED(1);
            stack[sp - 1] = -stack[sp - 1];
            break;
        case OP_NOT:
            NEED(1);
            stack[sp - 1] = ~stack[sp - 1];
            break;
        case OP_PLUS_UCONST:
            value = read_uleb(&c);
            NEED(1);
            stack[sp - 1] += value;
            break;
        case OP_AND:
            BINOP(a & b);
            break;
        case OP_OR:
            BINOP(a | b);
            break;
        case OP_XOR:
            BINOP(a ^ b);
            break;
        case OP_PLUS:
            BINOP(a + b);
            break;
        case OP_MINUS:
            BINOP(a - b);
            break;
        case OP_MUL:
            BINOP(a * b);
            break;
        case OP_SHL:
            BINOP(b < 64 ? a << b : 0);
            break;
        case OP_SHR:
            BINOP(b < 64 ? a >> b : 0);
            break;
        case OP_SHRA:
            BINOP((intptr_t)a >> (b < 64 ? b : 63));
            break;
        case OP_EQ:
            BINOP(a == b);
            break;
        case OP_NE:
            BINOP(a != b);
            break;
        case OP_GE:
            BINOP((intptr_t)a >= (intptr_t)b);
            break;
        case OP_GT:
            BINOP((intptr_t)a > (intptr_t)b);
            break;
        case OP_LE:
            BINOP((intptr_t)a <= (intptr_t)b);
            break;
        case OP_LT:
            BINOP((intptr_t)a < (intptr_t)b);
            break;
        case OP_DIV:
        case OP_MOD:
            NEED(2);
            if (stack[sp - 1] == 0)
                return false;
            if (op == OP_DIV)
                BINOP((intptr_t)a / (intptr_t)b);
            else
                BINOP(a % b);
            break;
        case OP_BREGX:
            value = read_uleb(&c);
            offset = read_sleb(&c);
            if (!get_reg(frame, value, &value))
                return false;
            PUSH(value + offset);
            break;
        case OP_SKIP:
        case OP_BRA:
            offset = (int16_t)read_fixed(&c, 2);
            if (op == OP_BRA) {
                NEED(1);
                if (stack[--sp] == 0)
                    break;
            }
            if (offset < block - c.pos || offset > c.end - c.pos)
                return false;
            c.pos += offset;
            break;
        case OP_NOP:
            break;
        default:
            return false;
        }
    }

#undef PUSH
#undef NEED
#undef BINOP

    if (c.error || sp == 0)
        return false;
    *out = stack[sp - 1];
    return true;
}

bool unwind_step(struct unwind_frame* frame) {
    const uintptr_t pc = unwind_lookup_pc(frame);
    struct unwind_frame next;
    struct cie cie;
    struct fde fde;

    // Look up the FDE in the binary search table of .eh_frame_hdr.
    const uintptr_t hdr = (uintptr_t)__ld_eh_frame_hdr_start;
    struct cursor c = {.pos = __ld_eh_frame_hdr_start, .end = __ld_eh_frame_hdr_end};

    const uint8_t version = read_byte(&c);
    const uint8_t eh_frame_ptr_enc = read_byte(&c);
    const uint8_t fde_count_enc = read_byte(&c);
    const uint8_t table_enc = read_byte(&c);
    if (c.error || version != 1 || eh_frame_ptr_enc == PE_OMIT || fde_count_enc == PE_OMIT)
        goto search_linear;

    read_encoded(&c, eh_frame_ptr_enc, hdr);
    const size_t count = read_encoded(&c, fde_count_enc, hdr);

    // The linker always emits this encoding, we can index the table directly.
    if (c.error || table_enc != (PE_DATAREL | PE_SDATA4) || count > (size_t)(c.end - c.pos) / 8)
        goto search_linear;

    const uint8_t* table = c.pos;

    // Find the last entry with initial_loc <= pc.
    size_t lo = 0, hi = count;
    while (lo < hi) {
        const size_t mid = lo + (hi - lo) / 2;
        int32_t loc;
        memcpy(&loc, table + mid * 8, sizeof(loc));
        if (hdr + (intptr_t)loc <= pc)
            lo = mid + 1;
        else
            hi = mid;
    }
    if (lo == 0)
        goto frame_pointer;

    int32_t fde_offset;
    memcpy(&fde_offset, table + (lo - 1) * 8 + 4, sizeof(fde_offset));
    if (!parse_fde((const uint8_t*)(hdr + (intptr_t)fde_offset), &cie, &fde) || pc < fde.pc_begin || pc >= fde.pc_end)
        goto frame_pointer;
    goto found;

search_linear:
    // No usable .eh_frame_hdr, walk all entries.
    struct cursor body;
    for (const uint8_t* pos = __ld_eh_frame_start; read_entry(pos, &body); pos = body.end) {
        uint32_t id;
        memcpy(&id, body.pos, sizeof(id));
        if (id != 0 && parse_fde(pos, &cie, &fde) && pc >= fde.pc_begin && pc < fde.pc_end)
            goto found;
    }
    goto frame_pointer;

found:
    if (cie.ra_reg >= UNWIND_NUM_REGS)
        goto frame_pointer;

    struct cfi_row initial = {.cfa.kind = RULE_UNDEF};
    if (!run_cfa_program(&cie, cie.insns, 0, UINTPTR_MAX, &initial, nullptr))
        goto frame_pointer;
    struct cfi_row row = initial;
    if (!run_cfa_program(&cie, fde.insns, fde.pc_begin, pc, &row, &initial))
        goto frame_pointer;

    uintptr_t cfa;
    if (row.cfa.kind == RULE_EXPR) {
        if (!eval_expr(row.cfa.value, frame, nullptr, &cfa))
            goto frame_pointer;
    } else if (row.cfa.kind == RULE_REG && get_reg(frame, row.cfa.value, &cfa)) {
        cfa += row.cfa_offset;
    } else {
        goto frame_pointer;
    }

    // By definition, the CFA is the value of the stack pointer in the caller.
    // Exception frames override this with an explicit rule for the stack pointer.
    next = *frame;
    next.regs[UNWIND_REG_SP] = cfa;
    next.valid |= REG_BIT(UNWIND_REG_SP);

    for (size_t r = 0; r < UNWIND_NUM_REGS; r++) {
        const struct rule* rule = &row.regs[r];
        uintptr_t value;
        bool ok;

        switch (rule->kind) {
        case RULE_SAME:
            continue;
        case RULE_UNDEF:
            ok = false;
            break;
        case RULE_OFFSET:
            ok = read_word(frame, cfa + rule->value, &value);
            break;
        case RULE_VAL_OFFSET:
            value = cfa + rule->value, ok = true;
            break;
        case RULE_REG:
            ok = get_reg(frame, rule->value, &value);
            break;
        case RULE_EXPR:
            ok = eval_expr(rule->value, frame, &cfa, &value) && read_word(frame, value, &value);
            break;
        case RULE_VAL_EXPR:
            ok = eval_expr(rule->value, frame, &cfa, &value);
            break;
        default:
            ok = false;
            break;
        }

        if (ok) {
            next.regs[r] = value;
            next.valid |= REG_BIT(r);
        } else {
            next.valid &= ~REG_BIT(r);
        }
    }

    // The CFI marks this as the outermost frame.
    if (row.regs[cie.ra_reg].kind == RULE_UNDEF)
        return false;
    if (!get_reg(&next, cie.ra_reg, &next.pc))
        goto frame_pointer;
    if (next.pc == 0)
        return false;

    // If this was an exception frame, the caller didn't execute a call instruction.
    next.exact_pc = cie.signal_frame;
    goto check;

frame_pointer:
    // Fall back to the frame record for code without CFI.
    uintptr_t fp, record[2];
    if (!get_reg(frame, UNWIND_REG_FP, &fp) || fp % alignof(uintptr_t) || !read_mem(frame, fp, record, sizeof(record)))
        return false;

    // The frame record has to be on the stack of this frame.
    if ((frame->valid & REG_BIT(UNWIND_REG_SP)) && fp < frame->regs[UNWIND_REG_SP])
        return false;

    if (record[1] == 0)
        return false;

    next = (struct unwind_frame){
        .pc = record[1],
        .stack_base = frame->stack_base,
        .stack_top = frame->stack_top,
        .exact_pc = false,
    };

    next.regs[UNWIND_REG_FP] = record[0];
    next.valid = REG_BIT(UNWIND_REG_FP);

    if (UNWIND_FP_KNOWS_SP) {
        next.regs[UNWIND_REG_SP] = fp + sizeof(record);
        next.valid |= REG_BIT(UNWIND_REG_SP);
    }

check:
    // Stop once we leave the kernel.
    if (mem_addr_is_user(next.pc))
        return false;

    // The stack grows down, so the caller's frame must be at a higher address. Don't loop forever.
    const uint64_t sp_bit = REG_BIT(UNWIND_REG_SP);
    if ((frame->valid & sp_bit) && (next.valid & sp_bit)) {
        const uintptr_t sp = frame->regs[UNWIND_REG_SP];
        const uintptr_t next_sp = next.regs[UNWIND_REG_SP];
        if (next_sp < sp || (next_sp == sp && next.pc == frame->pc))
            return false;
    }

    *frame = next;
    return true;
}
