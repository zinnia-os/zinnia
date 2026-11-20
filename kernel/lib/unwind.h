// SPDX-License-Identifier: BSD-3-Clause

#pragma once

#include <lib/compiler.h>
#include <bits/unwind.h>
#include <stddef.h>
#include <stdint.h>

// We use the bits of a uint64_t to track the validity of all registers.
static_assert(UNWIND_NUM_REGS <= 64);

// Register state of a single stack frame.
struct unwind_frame {
    uintptr_t regs[UNWIND_NUM_REGS];
    uint64_t valid; // Bitmap of register indices holding a known value.
    uintptr_t pc;
    uintptr_t stack_base;
    uintptr_t stack_top;

    // If true, `pc` points at the instruction that was executing.
    // Otherwise it is a return address.
    bool exact_pc;
};

// Returns the address that should be used for symbol and FDE lookups of this frame.
static inline uintptr_t unwind_lookup_pc(const struct unwind_frame* frame) {
    return frame->exact_pc ? frame->pc : frame->pc - 1;
}

void unwind_find_stack(struct unwind_frame* frame);

// Initializes `frame` with the register state of the calling function.
// This has to be inlined so the captured registers belong to the caller.
[[__force_inline]]
static inline void unwind_init(struct unwind_frame* frame) {
    *frame = (struct unwind_frame){.exact_pc = true};
    arch_unwind_capture(frame->regs, &frame->pc, &frame->valid);
    unwind_find_stack(frame);
}

// Unwinds `frame` to the frame of its caller.
// Returns false if the end of the stack was reached or the frame could not be unwound.
bool unwind_step(struct unwind_frame* frame);
