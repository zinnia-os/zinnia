/* SPDX-License-Identifier: BSD-3-Clause */

#ifndef __ZINNIA_SIGNAL_H
#define __ZINNIA_SIGNAL_H

#include <zinnia/compiler.h>
#include <zinnia/types.h>

#if defined(__x86_64__)
#include <zinnia/x86_64/signal.h>
#elif defined(__aarch64__)
#include <zinnia/aarch64/signal.h>
#elif defined(__riscv) && __riscv_xlen == 64
#include <zinnia/riscv64/signal.h>
#else
#error "Unsupported architecture"
#endif

#define __SIGABRT   1
#define __SIGALRM   2
#define __SIGBUS    3
#define __SIGCHLD   4
#define __SIGCONT   5
#define __SIGFPE    6
#define __SIGHUP    7
#define __SIGILL    8
#define __SIGINT    9
#define __SIGKILL   10
#define __SIGPIPE   11
#define __SIGQUIT   12
#define __SIGSEGV   13
#define __SIGSTOP   14
#define __SIGTERM   15
#define __SIGTSTP   16
#define __SIGTTIN   17
#define __SIGTTOU   18
#define __SIGUSR1   19
#define __SIGUSR2   20
#define __SIGWINCH  21
#define __SIGSYS    22
#define __SIGTRAP   23
#define __SIGURG    24
#define __SIGVTALRM 25
#define __SIGXCPU   26
#define __SIGXFSZ   27
#define __SIGIO     28
#define __SIGPOLL   29
#define __SIGPROF   30
#define __SIGPWR    31
#define __SIGIOT    32
#define __SIGCANCEL 33
#define __SIGRTMIN  35
#define __SIGRTMAX  64
#define __NSIG      65

typedef void (*__sighandler_t)(int);
typedef void (*__sigrestore_t)(void);

#define __SIG_DFL ((__sighandler_t)0)
#define __SIG_IGN ((__sighandler_t)1)
#define __SIG_ERR ((__sighandler_t)(-1))

#define __SIG_BLOCK   1
#define __SIG_UNBLOCK 2
#define __SIG_SETMASK 3

#define __SA_NOCLDSTOP (1 << 0)
#define __SA_ONSTACK   (1 << 1)
#define __SA_RESETHAND (1 << 2)
#define __SA_RESTART   (1 << 3)
#define __SA_SIGINFO   (1 << 4)
#define __SA_NOCLDWAIT (1 << 5)
#define __SA_NODEFER   (1 << 6)

#define __SS_ONSTACK 1
#define __SS_DISABLE 2

#define __SIGEV_NONE   1
#define __SIGEV_SIGNAL 2
#define __SIGEV_THREAD 3

#define __SI_TKILL  (-6)
#define __SI_TIMER  (-2)
#define __SI_QUEUE  (-1)
#define __SI_USER   0
#define __SI_KERNEL 128

#define __ILL_ILLOPC  1
#define __FPE_INTDIV  1
#define __SEGV_MAPERR 1
#define __SEGV_ACCERR 2
#define __BUS_ADRALN  1
#define __TRAP_BRKPT  1

#define __CLD_EXITED    1
#define __CLD_KILLED    2
#define __CLD_DUMPED    3
#define __CLD_TRAPPED   4
#define __CLD_STOPPED   5
#define __CLD_CONTINUED 6

typedef __zn_u64 __sigset_t;

union __sigval {
    int sival_int;
    void __user* sival_ptr;
};

struct __siginfo {
    int si_signo;
    int si_code;
    int si_errno;
    __pid_t si_pid;
    __uid_t si_uid;
    void __user* si_addr;
    int si_status;
    union __sigval si_value;
};

struct __sigaction {
    union {
        __sighandler_t sa_handler;
        void (*sa_sigaction)(int, struct __siginfo*, void*);
    };
    int sa_flags;
    __sigrestore_t sa_restorer;
    __sigset_t sa_mask;
};

struct __stack {
    void __user* ss_sp;
    __zn_size ss_size;
    int ss_flags;
};

struct __sigevent {
    union __sigval sigev_value;
    int sigev_notify;
    int sigev_signo;
    void (*sigev_notify_function)(union __sigval);
    void __user* sigev_notify_attributes;
    __pid_t sigev_notify_thread_id;
};

#define __SFD_CLOEXEC  __O_CLOEXEC
#define __SFD_NONBLOCK __O_NONBLOCK

struct __signalfd_siginfo {
    __zn_u32 ssi_signo;
    __zn_i32 ssi_errno;
    __zn_i32 ssi_code;
    __zn_u32 ssi_pid;
    __zn_u32 ssi_uid;
    __zn_i32 ssi_fd;
    __zn_u32 ssi_tid;
    __zn_u32 ssi_band;
    __zn_u32 ssi_overrun;
    __zn_u32 ssi_trapno;
    __zn_i32 ssi_status;
    __zn_i32 ssi_int;
    __zn_u64 ssi_ptr;
    __zn_u64 ssi_utime;
    __zn_u64 ssi_stime;
    __zn_u64 ssi_addr;
    __zn_u16 ssi_addr_lsb;
    __zn_u16 __pad2;
    __zn_i32 ssi_syscall;
    __zn_u64 ssi_call_addr;
    __zn_u32 ssi_arch;
    __zn_u8 __pad[28];
};

#endif
