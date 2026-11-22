// SPDX-License-Identifier: BSD-3-Clause

#include <lib/atomic.h>
#include <lib/percpu.h>
#include <lib/process.h>
#include <lib/sched.h>
#include <lib/signal.h>
#include <string.h>

enum default_action {
    ACTION_TERMINATE,
    ACTION_CORE,
    ACTION_STOP,
    ACTION_CONTINUE,
    ACTION_IGNORE,
};

static enum default_action default_action(signal_t sig) {
    switch (sig) {
    case SIGCHLD:
    case SIGURG:
    case SIGWINCH:
        return ACTION_IGNORE;
    case SIGCONT:
        return ACTION_CONTINUE;
    case SIGSTOP:
    case SIGTSTP:
    case SIGTTIN:
    case SIGTTOU:
        return ACTION_STOP;
    case SIGQUIT:
    case SIGABRT:
    case SIGBUS:
    case SIGFPE:
    case SIGILL:
    case SIGSEGV:
    case SIGSYS:
    case SIGTRAP:
    case SIGXCPU:
    case SIGXFSZ:
    case SIGIOT:
        return ACTION_CORE;
    default:
        return ACTION_TERMINATE;
    }
}

static bool action_ignores(const struct __sigaction* action, signal_t sig) {
    return action->sa_handler == __SIG_IGN || (action->sa_handler == __SIG_DFL && default_action(sig) == ACTION_IGNORE);
}

static void queue_add(struct sig_queue* queue, signal_t sig, const struct sig_info* info) {
    if (atomic_load(&queue->pending, ATOMIC_RELAXED) & sig_bit(sig))
        return;
    queue->info[sig - 1] = *info;
    atomic_fetch_or(&queue->pending, sig_bit(sig), ATOMIC_RELEASE);
}

static void queue_discard(struct sig_queue* queue, sigset_t set) {
    atomic_fetch_and(&queue->pending, ~set, ATOMIC_RELEASE);
}

static signal_t queue_take(struct sig_queue* queue, sigset_t allowed, struct sig_info* out) {
    const sigset_t ready = atomic_load(&queue->pending, ATOMIC_RELAXED) & allowed;
    if (!ready)
        return 0;
    const signal_t sig = __builtin_ctzll(ready) + 1;
    queue_discard(queue, sig_bit(sig));
    if (out)
        *out = queue->info[sig - 1];
    return sig;
}

static void discard_everywhere_locked(struct process* proc, sigset_t set) {
    queue_discard(&proc->signals.shared, set);
    spin_lock(&proc->tasks_lock);
    for (struct task* t = tailq_first(&proc->tasks); t; t = tailq_next(t, process_link))
        queue_discard(&t->signals.queue, set);
    spin_unlock(&proc->tasks_lock);
}

void signal_process_init(struct process* proc, struct process* parent) {
    struct process_signals* s = &proc->signals;
    spin_init(&s->lock);
    poll_event_init(&s->event);
    poll_event_init(&s->cont_event);

    if (parent) {
        spin_lock(&parent->signals.lock);
        memcpy(s->actions, parent->signals.actions, sizeof(s->actions));
        spin_unlock(&parent->signals.lock);
    }
}

void signal_task_fork(struct task* task, struct task* parent) {
    atomic_store(&task->signals.mask, atomic_load(&parent->signals.mask, ATOMIC_RELAXED), ATOMIC_RELAXED);
    task->signals.alt_sp = parent->signals.alt_sp;
    task->signals.alt_size = parent->signals.alt_size;
}

void signal_exec(struct process* proc, struct task* task) {
    spin_lock(&proc->signals.lock);
    for (size_t i = 0; i < SIG_MAX; i++) {
        if (proc->signals.actions[i].sa_handler != __SIG_IGN)
            proc->signals.actions[i] = (struct __sigaction){};
    }
    task->signals.alt_sp = 0;
    task->signals.alt_size = 0;
    spin_unlock(&proc->signals.lock);
}

struct sig_info signal_info_kernel() {
    return (struct sig_info){.code = __SI_KERNEL};
}

struct sig_info signal_info_user(struct process* sender, int code) {
    return (struct sig_info){
        .code = code,
        .pid = sender->pid,
        .uid = sender->identity.uid,
    };
}

bool signal_pending(struct task* task) {
    if (process_task_must_exit(task))
        return true;
    const sigset_t pending = atomic_load(&task->signals.queue.pending, ATOMIC_ACQUIRE) |
        atomic_load(&task->process->signals.shared.pending, ATOMIC_ACQUIRE);
    return pending & ~atomic_load(&task->signals.mask, ATOMIC_RELAXED);
}

static bool do_send(struct process* proc, ssize_t tid, signal_t sig, const struct sig_info* info) {
    if (sig == 0) {
        // Signal 0 only checks if the target exists, so just look for a matching task.
        spin_lock(&proc->tasks_lock);
        bool found = false;
        for (struct task* t = tailq_first(&proc->tasks); t && !found; t = tailq_next(t, process_link))
            found = tid < 0 || t->id == (size_t)tid;
        spin_unlock(&proc->tasks_lock);
        return found;
    }

    struct process_signals* s = &proc->signals;
    const sigset_t bit = sig_bit(sig);
    bool continued = false;
    bool queued = false;

    spin_lock(&s->lock);
    if ((sig == SIGCONT || sig == SIGKILL) && atomic_load(&s->stopped, ATOMIC_RELAXED)) {
        atomic_store(&s->stopped, false, ATOMIC_RELEASE);
        s->stop_unwaited = false;
        s->cont_unwaited = sig == SIGCONT;
        continued = true;
    }
    if (sig == SIGCONT)
        discard_everywhere_locked(proc, SIG_STOPS);
    else if (bit & SIG_STOPS)
        discard_everywhere_locked(proc, sig_bit(SIGCONT));

    // Wake a thread that can take the signal right away, or just the first one.
    spin_lock(&proc->tasks_lock);
    struct task* first = tailq_first(&proc->tasks);
    struct task* target = nullptr;
    for (struct task* t = first; t; t = tailq_next(t, process_link)) {
        if (tid >= 0 ? t->id == (size_t)tid : !(atomic_load(&t->signals.mask, ATOMIC_RELAXED) & bit)) {
            target = t;
            break;
        }
    }
    const bool blocked = target ? (atomic_load(&target->signals.mask, ATOMIC_RELAXED) & bit) != 0 : true;
    if (!target && tid < 0)
        target = first;

    // Keep blocked signals even if ignored.
    const struct __sigaction* action = &s->actions[sig - 1];
    bool drop = !target || (!(bit & SIG_UNBLOCKABLE) && !blocked && action_ignores(action, sig));
    if (proc->pid == 1 && action->sa_handler == __SIG_DFL && !blocked)
        drop = true;

    if (!drop) {
        queue_add(tid >= 0 ? &target->signals.queue : &s->shared, sig, info);
        queued = true;
        if (sig == SIGKILL) {
            for (struct task* t = first; t; t = tailq_next(t, process_link))
                sched_notify(t);
        } else {
            sched_notify(target);
        }
    }
    spin_unlock(&proc->tasks_lock);
    spin_unlock(&s->lock);

    if (queued)
        poll_event_notify(&s->event);
    if (continued) {
        poll_event_notify(&s->cont_event);
        if (sig == SIGCONT)
            process_notify_parent(proc, __CLD_CONTINUED, SIGCONT);
    }
    return target != nullptr;
}

bool signal_send(struct process* proc, signal_t sig, const struct sig_info* info) {
    return do_send(proc, -1, sig, info);
}

bool signal_send_thread(struct process* proc, size_t tid, signal_t sig, const struct sig_info* info) {
    return do_send(proc, tid, sig, info);
}

void signal_send_task(struct task* task, signal_t sig, const struct sig_info* info) {
    do_send(task->process, task->id, sig, info);
}

void signal_force(struct task* task, signal_t sig, const struct sig_info* info) {
    struct process_signals* s = &task->process->signals;
    spin_lock(&s->lock);
    if (s->actions[sig - 1].sa_handler == __SIG_IGN ||
        (atomic_load(&task->signals.mask, ATOMIC_RELAXED) & sig_bit(sig)))
        s->actions[sig - 1] = (struct __sigaction){};

    atomic_fetch_and(&task->signals.mask, ~sig_bit(sig), ATOMIC_RELAXED);
    queue_add(&task->signals.queue, sig, info);
    spin_unlock(&s->lock);
}

void signal_flush_ignored(struct process* proc, signal_t sig) {
    struct process_signals* s = &proc->signals;
    spin_lock(&s->lock);
    if (action_ignores(&s->actions[sig - 1], sig))
        discard_everywhere_locked(proc, sig_bit(sig));
    spin_unlock(&s->lock);
}

signal_t signal_dequeue(struct task* task, sigset_t set, struct sig_info* out_info) {
    struct process_signals* s = &task->process->signals;
    struct sig_info info;
    spin_lock(&s->lock);
    signal_t sig = queue_take(&task->signals.queue, set, &info);
    if (!sig)
        sig = queue_take(&s->shared, set, &info);
    spin_unlock(&s->lock);
    if (sig && out_info)
        *out_info = info;
    return sig;
}

void signal_set_restore_mask(struct task* task, sigset_t mask) {
    struct process_signals* s = &task->process->signals;
    spin_lock(&s->lock);
    if (!task->signals.restore_mask) {
        task->signals.saved_mask = atomic_load(&task->signals.mask, ATOMIC_RELAXED);
        task->signals.restore_mask = true;
    }
    atomic_store(&task->signals.mask, mask & ~SIG_UNBLOCKABLE, ATOMIC_RELAXED);
    spin_unlock(&s->lock);
}

void signal_restore_mask(struct task* task) {
    struct process_signals* s = &task->process->signals;
    spin_lock(&s->lock);
    if (task->signals.restore_mask) {
        atomic_store(&task->signals.mask, task->signals.saved_mask, ATOMIC_RELAXED);
        task->signals.restore_mask = false;
    }
    spin_unlock(&s->lock);
}

void signal_info_to_user(signal_t sig, const struct sig_info* info, struct __siginfo* out) {
    *out = (struct __siginfo){
        .si_signo = sig,
        .si_code = info->code,
        .si_errno = info->error,
        .si_pid = info->pid,
        .si_uid = info->uid,
        .si_addr = (__user void*)info->addr,
        .si_status = info->status,
        .si_value.sival_ptr = (__user void*)info->value,
    };
}

void signal_deliver(arch_context_t* ctx, const arch_syscall_restart_t* restart) {
    struct task* task = percpu_get()->sched.current;
    struct process* proc = task->process;
    struct process_signals* s = &proc->signals;
    if (proc == process_kernel())
        return;

    while (true) {
        if (process_task_must_exit(task))
            sched_exit();

        spin_lock(&s->lock);
        if (atomic_load(&s->stopped, ATOMIC_RELAXED)) {
            spin_unlock(&s->lock);

            // Sleep uninterruptibly until SIGCONT or SIGKILL continues the process, or we have to exit.
            struct poll_table table;
            poll_table_init(&table);
            table.interruptible = false;
            poll_wait(&table, &s->cont_event);
            while (atomic_load(&s->stopped, ATOMIC_ACQUIRE) && !process_task_must_exit(task)) {
                poll_table_wait(&table, nullptr);
                atomic_store(&table.triggered, false, ATOMIC_RELAXED);
            }
            poll_table_fini(&table);
            continue;
        }

        const sigset_t allowed = ~atomic_load(&task->signals.mask, ATOMIC_RELAXED);
        struct sig_info info;
        signal_t sig = queue_take(&task->signals.queue, allowed, &info);
        if (!sig)
            sig = queue_take(&s->shared, allowed, &info);

        if (!sig) {
            if (task->signals.restore_mask) {
                atomic_store(&task->signals.mask, task->signals.saved_mask, ATOMIC_RELAXED);
                task->signals.restore_mask = false;
                spin_unlock(&s->lock);
                continue;
            }
            spin_unlock(&s->lock);
            if (restart)
                syscall_restart(ctx, restart);
            return;
        }

        const struct __sigaction action = s->actions[sig - 1];
        if (action.sa_handler != __SIG_DFL && action.sa_handler != __SIG_IGN && (action.sa_flags & __SA_RESETHAND))
            s->actions[sig - 1] = (struct __sigaction){};

        if (action.sa_handler == __SIG_IGN) {
            spin_unlock(&s->lock);
            continue;
        }

        if (action.sa_handler == __SIG_DFL) {
            switch (default_action(sig)) {
            case ACTION_IGNORE:
            case ACTION_CONTINUE:
                spin_unlock(&s->lock);
                continue;
            case ACTION_STOP:
                atomic_store(&s->stopped, true, ATOMIC_RELEASE);
                s->stop_sig = sig;
                s->stop_unwaited = true;
                s->cont_unwaited = false;
                spin_unlock(&s->lock);
                process_notify_parent(proc, __CLD_STOPPED, sig);
                continue;
            case ACTION_TERMINATE:
            case ACTION_CORE:
                spin_unlock(&s->lock);
                process_exit_signal(sig);
            }
        }

        struct sig_delivery delivery = {
            .sig = sig,
            .handler = (uintptr_t)action.sa_handler,
            .flags = action.sa_flags,
            .restorer = (uintptr_t)action.sa_restorer,
            .old_mask = task->signals.restore_mask ? task->signals.saved_mask
                                                   : atomic_load(&task->signals.mask, ATOMIC_RELAXED),
            .alt_sp = task->signals.alt_sp,
            .alt_size = task->signals.alt_size,
        };
        task->signals.restore_mask = false;
        if (!(action.sa_flags & __SA_NODEFER))
            atomic_fetch_or(&task->signals.mask, sig_bit(sig), ATOMIC_RELAXED);
        atomic_store(
            &task->signals.mask,
            (atomic_load(&task->signals.mask, ATOMIC_RELAXED) | action.sa_mask) & ~SIG_UNBLOCKABLE,
            ATOMIC_RELAXED
        );
        spin_unlock(&s->lock);

        signal_info_to_user(sig, &info, &delivery.info);
        if (restart) {
            if (action.sa_flags & __SA_RESTART)
                syscall_restart(ctx, restart);
            else
                syscall_set_return(ctx, 0, EINTR);
        }

        if (signal_setup_frame(ctx, &delivery))
            process_exit_signal(SIGSEGV);
        return;
    }
}
