// SPDX-License-Identifier: BSD-3-Clause

#pragma once

#include <lib/types.h>
#include <stddef.h>
#include <stdint.h>
#include <zinnia/signal.h>

constexpr int SIG_MAX = 64;

typedef __sigset_t sigset_t;

#define sig_bit(sig) ((sigset_t)1 << ((sig) - 1))

static inline bool sig_valid(int sig) {
    return sig >= 1 && sig <= SIG_MAX;
}

constexpr sigset_t SIG_UNBLOCKABLE = (sig_bit(__SIGKILL) | sig_bit(__SIGSTOP));
constexpr sigset_t SIG_STOPS = (sig_bit(__SIGSTOP) | sig_bit(__SIGTSTP) | sig_bit(__SIGTTIN) | sig_bit(__SIGTTOU));

struct task;

struct sig_delivery {
    int sig;
    uintptr_t handler;
    uint64_t flags;
    uintptr_t restorer;
    sigset_t old_mask;
    struct __siginfo info;
    uintptr_t alt_sp;
    size_t alt_size;
};

// TODO
static inline bool signal_pending(struct task*) {
    return false;
}
