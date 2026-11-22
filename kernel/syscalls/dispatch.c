// SPDX-License-Identifier: BSD-3-Clause

#include <lib/compiler.h>
#include <lib/errno.h>
#include <lib/irq.h>
#include <lib/panic.h>
#include <lib/percpu.h>
#include <lib/print.h>
#include <lib/process.h>
#include <lib/sched.h>
#include <lib/signal.h>
#include <lib/types.h>
#include <lib/utils.h>
#include <syscalls/syscall.h>
#include <bits/signal.h>
#include <stdcountof.h>

// Declare all syscalls.
#define SYSCALL_DECLARE(name, num_args) extern void sys_##name(...);
#include <syscalls/syscall_list.inc>
#undef SYSCALL_DECLARE
#undef SYSCALL_MISSING

// Define all syscalls.
const struct syscall syscall_table[256] = {
#define SYSCALL_DECLARE(name, num_args) [__NR_##name] = {sys_##name, num_args},
#define SYSCALL_MISSING(name, num_args)
#include <syscalls/syscall_list.inc>
#undef SYSCALL_DECLARE
#undef SYSCALL_MISSING
};

// Give all syscalls names.
const char* const syscall_names[256] = {
#define SYSCALL_DECLARE(name, num_args) [__NR_##name] = #name,
#define SYSCALL_MISSING(name, num_args) [__NR_##name] = #name,
#include <syscalls/syscall_list.inc>
#undef SYSCALL_DECLARE
#undef SYSCALL_MISSING
};

static void warn_unimplemented(size_t num) {
    const char* name = num < countof(syscall_names) && syscall_names[num] ? syscall_names[num] : "(unknown)";
    struct task* task = percpu_get()->sched.current;
    log_warn(
        "syscall: \"%s\" (%zu) is not implemented, called by \"%s\" (pid %d)\n",
        name,
        num,
        task->name,
        task->process ? task->process->pid : 0
    );
}

static void syscall_do_signals(arch_context_t* ctx, const arch_syscall_restart_t* restart, errno_t error) {
    struct task* task = percpu_get()->sched.current;
    const bool restarting = error == ERESTART;
    if (restarting || signal_pending(task) || task->signals.restore_mask)
        signal_deliver(ctx, restarting ? restart : nullptr);
}

void syscall_dispatch(arch_context_t* ctx) {
    const size_t num = ctx->ARCH_CTX_NUM;
    arch_syscall_restart_t restart;
    syscall_save(ctx, &restart);
    irq_set_state(true);

    if (__unlikely(num >= countof(syscall_table) || syscall_table[num].func == nullptr)) {
        warn_unimplemented(num);
        ctx->ARCH_CTX_RET1 = ENOSYS;
        syscall_do_signals(ctx, &restart, ENOSYS);
        irq_set_state(false);
        return;
    }

    struct syscall_ret ret;
    void* syscall_func = syscall_table[num].func;
    switch (syscall_table[num].num_args) {
    case -1: {
        struct syscall_ret (*func)(arch_context_t*) = syscall_func;
        ret = func(ctx);
        break;
    }
    case 0: {
        struct syscall_ret (*func)() = syscall_func;
        ret = func();
        break;
    }
    case 1: {
        struct syscall_ret (*func)(uintptr_t) = syscall_func;
        ret = func(ctx->ARCH_CTX_A0);
        break;
    }
    case 2: {
        struct syscall_ret (*func)(uintptr_t, uintptr_t) = syscall_func;
        ret = func(ctx->ARCH_CTX_A0, ctx->ARCH_CTX_A1);
        break;
    }
    case 3: {
        struct syscall_ret (*func)(uintptr_t, uintptr_t, uintptr_t) = syscall_func;
        ret = func(ctx->ARCH_CTX_A0, ctx->ARCH_CTX_A1, ctx->ARCH_CTX_A2);
        break;
    }
    case 4: {
        struct syscall_ret (*func)(uintptr_t, uintptr_t, uintptr_t, uintptr_t) = syscall_func;
        ret = func(ctx->ARCH_CTX_A0, ctx->ARCH_CTX_A1, ctx->ARCH_CTX_A2, ctx->ARCH_CTX_A3);
        break;
    }
    case 5: {
        struct syscall_ret (*func)(uintptr_t, uintptr_t, uintptr_t, uintptr_t, uintptr_t) = syscall_func;
        ret = func(ctx->ARCH_CTX_A0, ctx->ARCH_CTX_A1, ctx->ARCH_CTX_A2, ctx->ARCH_CTX_A3, ctx->ARCH_CTX_A4);
        break;
    }
    case 6: {
        struct syscall_ret (*func)(uintptr_t, uintptr_t, uintptr_t, uintptr_t, uintptr_t, uintptr_t) = syscall_func;
        ret = func(
            ctx->ARCH_CTX_A0,
            ctx->ARCH_CTX_A1,
            ctx->ARCH_CTX_A2,
            ctx->ARCH_CTX_A3,
            ctx->ARCH_CTX_A4,
            ctx->ARCH_CTX_A5
        );
        break;
    }
    default:
        panic("Syscall %zu has more than 6 arguments\n", num);
    }

    ctx->ARCH_CTX_RET0 = ret.value;
    ctx->ARCH_CTX_RET1 = ret.error;
    syscall_do_signals(ctx, &restart, ret.error);
    irq_set_state(false);
}
