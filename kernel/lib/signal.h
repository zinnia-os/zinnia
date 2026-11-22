// SPDX-License-Identifier: BSD-3-Clause

#pragma once

#include <fs/poll.h>
#include <lib/atomic.h>
#include <lib/errno.h>
#include <lib/spin.h>
#include <lib/types.h>
#include <bits/sched.h>
#include <stddef.h>
#include <stdint.h>
#include <zinnia/signal.h>
#include <zinnia/ucontext.h>

constexpr int SIG_MAX = 64;

typedef enum : int {
    SIGABRT = __SIGABRT,
    SIGALRM = __SIGALRM,
    SIGBUS = __SIGBUS,
    SIGCHLD = __SIGCHLD,
    SIGCONT = __SIGCONT,
    SIGFPE = __SIGFPE,
    SIGHUP = __SIGHUP,
    SIGILL = __SIGILL,
    SIGINT = __SIGINT,
    SIGKILL = __SIGKILL,
    SIGPIPE = __SIGPIPE,
    SIGQUIT = __SIGQUIT,
    SIGSEGV = __SIGSEGV,
    SIGSTOP = __SIGSTOP,
    SIGTERM = __SIGTERM,
    SIGTSTP = __SIGTSTP,
    SIGTTIN = __SIGTTIN,
    SIGTTOU = __SIGTTOU,
    SIGUSR1 = __SIGUSR1,
    SIGUSR2 = __SIGUSR2,
    SIGWINCH = __SIGWINCH,
    SIGSYS = __SIGSYS,
    SIGTRAP = __SIGTRAP,
    SIGURG = __SIGURG,
    SIGVTALRM = __SIGVTALRM,
    SIGXCPU = __SIGXCPU,
    SIGXFSZ = __SIGXFSZ,
    SIGIO = __SIGIO,
    SIGPOLL = __SIGPOLL,
    SIGPROF = __SIGPROF,
    SIGPWR = __SIGPWR,
    SIGIOT = __SIGIOT,
    SIGCANCEL = __SIGCANCEL,
    SIGRTMIN = __SIGRTMIN,
    SIGRTMAX = __SIGRTMAX,
} signal_t;

typedef __sigset_t sigset_t;

#define sig_bit(sig) ((sigset_t)1 << ((sig) - 1))

static inline bool sig_valid(signal_t sig) {
    return sig >= 1 && sig <= SIG_MAX;
}

constexpr sigset_t SIG_UNBLOCKABLE = (sig_bit(SIGKILL) | sig_bit(SIGSTOP));
constexpr sigset_t SIG_STOPS = (sig_bit(SIGSTOP) | sig_bit(SIGTSTP) | sig_bit(SIGTTIN) | sig_bit(SIGTTOU));

struct task;
struct process;

struct sig_info {
    int code;
    int error;
    pid_t pid;
    uid_t uid;
    uintptr_t addr;
    int status;
    uintptr_t value;
};

struct sig_queue {
    _Atomic(sigset_t) pending;
    struct sig_info info[SIG_MAX];
};

struct task_signals {
    struct sig_queue queue;
    _Atomic(sigset_t) mask;
    sigset_t saved_mask;
    bool restore_mask;
    uintptr_t alt_sp;
    size_t alt_size;
};

struct process_signals {
    struct spinlock lock;
    struct __sigaction actions[SIG_MAX];
    struct sig_queue shared;
    struct poll_event event;

    _Atomic(bool) stopped;
    signal_t stop_sig;
    bool stop_unwaited;
    bool cont_unwaited;
    struct poll_event cont_event;
};

// Everything the arch code needs to build a signal frame.
struct sig_delivery {
    signal_t sig;
    uintptr_t handler;
    uint64_t flags;
    uintptr_t restorer;
    sigset_t old_mask;
    struct __siginfo info;
    uintptr_t alt_sp;
    size_t alt_size;
};

void signal_process_init(struct process* proc, struct process* parent);
void signal_task_fork(struct task* task, struct task* parent);
void signal_exec(struct process* proc, struct task* task);

struct sig_info signal_info_kernel();
struct sig_info signal_info_user(struct process* sender, int code);

bool signal_pending(struct task* task);

// Queue `sig` on `proc` and wake a thread that doesn't block it.
bool signal_send(struct process* proc, signal_t sig, const struct sig_info* info);
void signal_send_task(struct task* task, signal_t sig, const struct sig_info* info);
bool signal_send_thread(struct process* proc, size_t tid, signal_t sig, const struct sig_info* info);

// Queue a synchronous fault signal on `task`.
void signal_force(struct task* task, signal_t sig, const struct sig_info* info);

void signal_flush_ignored(struct process* proc, signal_t sig);
signal_t signal_dequeue(struct task* task, sigset_t set, struct sig_info* out_info);

void signal_deliver(arch_context_t* ctx, const arch_syscall_restart_t* restart);

// Switch to `mask` until the next signal delivery.
void signal_set_restore_mask(struct task* task, sigset_t mask);

void signal_restore_mask(struct task* task);

void signal_info_to_user(signal_t sig, const struct sig_info* info, struct __siginfo* out);

#include <bits/signal.h>

// Return the stack space a signal frame needs at most (`AT_MINSIGSTKSZ`).
static inline size_t signal_frame_size() {
    return arch_signal_frame_size();
}

static inline errno_t signal_setup_frame(arch_context_t* ctx, const struct sig_delivery* delivery) {
    return arch_signal_setup_frame(ctx, delivery);
}

static inline errno_t signal_restore_context(arch_context_t* ctx, const __ucontext_t* uc, uintptr_t uc_addr) {
    return arch_signal_restore_context(ctx, uc, uc_addr);
}

// Save the registers that the syscall return clobbers, so we can restart the syscall later.
static inline void syscall_save(const arch_context_t* ctx, arch_syscall_restart_t* out) {
    arch_syscall_save(ctx, out);
}

// Rewind `ctx` to the syscall instruction and undo the clobbers saved by `syscall_save`.
static inline void syscall_restart(arch_context_t* ctx, const arch_syscall_restart_t* restart) {
    arch_syscall_restart(ctx, restart);
}

static inline void syscall_set_return(arch_context_t* ctx, uintptr_t value, errno_t error) {
    arch_syscall_set_return(ctx, value, error);
}
