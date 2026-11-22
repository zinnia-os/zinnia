// SPDX-License-Identifier: BSD-3-Clause

#include <lib/atomic.h>
#include <lib/clock.h>
#include <lib/irq.h>
#include <lib/sched.h>
#include <lib/signal.h>
#include <mem/usercopy.h>
#include <syscalls/private.h>
#include <stdckdint.h>
#include <zinnia/time.h>

static struct task* current_task() {
    return percpu_get()->sched.current;
}

struct syscall_ret sys_sigaction(signal_t sig, const __user struct __sigaction* act, __user struct __sigaction* oact) {
    struct syscall_ret ret = {};

    if (!sig_valid(sig)) {
        ret.error = EINVAL;
        return ret;
    }

    struct __sigaction action;
    if (act) {
        if (sig == SIGKILL || sig == SIGSTOP) {
            ret.error = EINVAL;
            return ret;
        }
        if (!usercopy_read(&action, act, sizeof(action))) {
            ret.error = EFAULT;
            return ret;
        }
        action.sa_mask &= ~SIG_UNBLOCKABLE;
    }

    struct process* proc = self();
    spin_lock(&proc->signals.lock);
    const struct __sigaction old = proc->signals.actions[sig - 1];
    spin_unlock(&proc->signals.lock);

    if (oact && !usercopy_write(oact, &old, sizeof(old))) {
        ret.error = EFAULT;
        return ret;
    }

    if (act) {
        spin_lock(&proc->signals.lock);
        proc->signals.actions[sig - 1] = action;
        spin_unlock(&proc->signals.lock);
        signal_flush_ignored(proc, sig);
    }
    return ret;
}

struct syscall_ret sys_sigprocmask(int how, const __user sigset_t* set, __user sigset_t* old) {
    struct syscall_ret ret = {};

    sigset_t new_set = 0;
    if (set) {
        if (how < __SIG_BLOCK || how > __SIG_SETMASK) {
            ret.error = EINVAL;
            return ret;
        }
        if (!usercopy_read(&new_set, set, sizeof(new_set))) {
            ret.error = EFAULT;
            return ret;
        }
    }

    struct task* task = current_task();
    struct process_signals* s = &task->process->signals;
    spin_lock(&s->lock);
    const sigset_t current = atomic_load(&task->signals.mask, ATOMIC_RELAXED);
    spin_unlock(&s->lock);

    if (old && !usercopy_write(old, &current, sizeof(current))) {
        ret.error = EFAULT;
        return ret;
    }

    if (set) {
        spin_lock(&s->lock);
        sigset_t mask = atomic_load(&task->signals.mask, ATOMIC_RELAXED);
        if (how == __SIG_BLOCK)
            mask |= new_set;
        else if (how == __SIG_UNBLOCK)
            mask &= ~new_set;
        else
            mask = new_set;
        atomic_store(&task->signals.mask, mask & ~SIG_UNBLOCKABLE, ATOMIC_RELAXED);
        spin_unlock(&s->lock);
    }
    return ret;
}

static bool altstack_contains(const struct task_signals* t, uintptr_t sp) {
    return t->alt_size && sp > t->alt_sp && sp - t->alt_sp <= t->alt_size;
}

static errno_t altstack_set(struct task_signals* t, const struct __stack* next, uintptr_t sp) {
    if (altstack_contains(t, sp))
        return EPERM;
    if (next->ss_flags != 0 && next->ss_flags != __SS_ONSTACK && next->ss_flags != __SS_DISABLE)
        return EINVAL;

    if (next->ss_flags == __SS_DISABLE) {
        t->alt_sp = 0;
        t->alt_size = 0;
        return 0;
    }
    if (next->ss_size < MAX(__MINSIGSTKSZ, signal_frame_size()))
        return ENOMEM;
    uintptr_t end;
    if (ckd_add(&end, (uintptr_t)next->ss_sp, next->ss_size))
        return EINVAL;
    t->alt_sp = (uintptr_t)next->ss_sp;
    t->alt_size = next->ss_size;
    return 0;
}

[[noreturn]]
struct syscall_ret sys_sigreturn(arch_context_t* ctx) {
    struct task* task = current_task();
    const uintptr_t uc_addr = ctx->ARCH_CTX_A0;
    __ucontext_t uc;
    if (!usercopy_read(&uc, (const __user void*)uc_addr, sizeof(uc)) || signal_restore_context(ctx, &uc, uc_addr))
        process_exit_signal(SIGSEGV);

    if (!(uc.uc_flags & __UC_CPU))
        syscall_set_return(ctx, 0, 0);
    if (uc.uc_flags & __UC_STACK)
        altstack_set(&task->signals, &uc.uc_stack, ctx->ARCH_CTX_SP);

    struct process_signals* s = &task->process->signals;
    spin_lock(&s->lock);
    if (uc.uc_flags & __UC_SIGMASK)
        atomic_store(&task->signals.mask, uc.uc_sigmask & ~SIG_UNBLOCKABLE, ATOMIC_RELAXED);
    task->signals.restore_mask = false;
    spin_unlock(&s->lock);

    signal_deliver(ctx, nullptr);
    irq_set_state(false);
    arch_sched_jump_to_context(ctx);
}

struct syscall_ret sys_sigpending(__user sigset_t* set) {
    struct syscall_ret ret = {};

    struct task* task = current_task();
    struct process_signals* s = &task->process->signals;
    spin_lock(&s->lock);
    const sigset_t pending =
        (atomic_load(&task->signals.queue.pending, ATOMIC_RELAXED) | atomic_load(&s->shared.pending, ATOMIC_RELAXED)) &
        atomic_load(&task->signals.mask, ATOMIC_RELAXED);
    spin_unlock(&s->lock);
    if (!usercopy_write(set, &pending, sizeof(pending)))
        ret.error = EFAULT;
    return ret;
}

struct syscall_ret sys_sigtimedwait(
    const __user sigset_t* set,
    __user struct __siginfo* user_info,
    const __user struct __timespec* timeout
) {
    struct syscall_ret ret = {};

    sigset_t wait_set;
    if (!usercopy_read(&wait_set, set, sizeof(wait_set))) {
        ret.error = EFAULT;
        return ret;
    }
    wait_set &= ~SIG_UNBLOCKABLE;

    uint64_t deadline = 0;
    if (timeout) {
        // Convert the relative timeout to an absolute deadline and saturate on overflow.
        struct __timespec ts;
        if (!usercopy_read(&ts, timeout, sizeof(ts))) {
            ret.error = EFAULT;
            return ret;
        }
        if (ts.tv_sec < 0 || ts.tv_nsec < 0 || ts.tv_nsec >= 1'000'000'000) {
            ret.error = EINVAL;
            return ret;
        }
        uint64_t ns;
        if (ckd_mul(&ns, (uint64_t)ts.tv_sec, 1'000'000'000) || ckd_add(&ns, ns, (uint64_t)ts.tv_nsec) ||
            ckd_add(&deadline, clock_get_elapsed_ns(), ns))
            deadline = UINT64_MAX;
    }

    struct task* task = current_task();
    struct poll_table table;
    poll_table_init(&table);
    poll_wait(&table, &task->process->signals.event);

    signal_t sig = 0;
    while (true) {
        struct sig_info info;
        sig = signal_dequeue(task, wait_set, &info);
        if (sig) {
            struct __siginfo out;
            signal_info_to_user(sig, &info, &out);
            if (user_info && !usercopy_write(user_info, &out, sizeof(out))) {
                signal_send_task(task, sig, &info);
                ret.error = EFAULT;
            }
            break;
        }
        if (signal_pending(task)) {
            ret.error = EINTR;
            break;
        }
        if (timeout && clock_get_elapsed_ns() >= deadline) {
            ret.error = EAGAIN;
            break;
        }
        poll_table_wait(&table, timeout ? &deadline : nullptr);
        atomic_store(&table.triggered, false, ATOMIC_RELAXED);
    }
    poll_table_fini(&table);
    if (!ret.error)
        ret.value = sig;
    return ret;
}

struct syscall_ret sys_sigsuspend(const __user sigset_t* set) {
    struct syscall_ret ret = {};

    sigset_t mask;
    if (!usercopy_read(&mask, set, sizeof(mask))) {
        ret.error = EFAULT;
        return ret;
    }

    struct task* task = current_task();
    signal_set_restore_mask(task, mask);

    struct poll_table table;
    poll_table_init(&table);
    while (poll_table_wait(&table, nullptr) != EINTR)
        atomic_store(&table.triggered, false, ATOMIC_RELAXED);
    poll_table_fini(&table);
    ret.error = EINTR;
    return ret;
}

struct syscall_ret sys_sigaltstack(arch_context_t* ctx) {
    struct syscall_ret ret = {};

    const __user struct __stack* ss = (const __user struct __stack*)ctx->ARCH_CTX_A0;
    __user struct __stack* oss = (__user struct __stack*)ctx->ARCH_CTX_A1;
    const uintptr_t sp = ctx->ARCH_CTX_SP;

    struct task_signals* t = &current_task()->signals;
    if (oss) {
        const struct __stack old = {
            .ss_sp = (__user void*)t->alt_sp,
            .ss_size = t->alt_size,
            .ss_flags = altstack_contains(t, sp) ? __SS_ONSTACK
                : t->alt_size                    ? 0
                                                 : __SS_DISABLE,
        };
        if (!usercopy_write(oss, &old, sizeof(old))) {
            ret.error = EFAULT;
            return ret;
        }
    }

    if (ss) {
        struct __stack next;
        if (!usercopy_read(&next, ss, sizeof(next))) {
            ret.error = EFAULT;
            return ret;
        }
        ret.error = altstack_set(t, &next, sp);
    }
    return ret;
}
