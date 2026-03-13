#pragma once

#include <stdint.h>
#include <uapi/types.h>

#define POLL_IN  1
#define POLL_OUT 2
#define POLL_MSG 3
#define POLL_ERR 4
#define POLL_PRI 5
#define POLL_HUP 6

#define SIGHUP    1
#define SIGINT    2
#define SIGQUIT   3
#define SIGCONT   4
#define SIGBUS    5
#define SIGABRT   6
#define SIGCHLD   7
#define SIGFPE    8
#define SIGKILL   9
#define SIGILL    10
#define SIGPIPE   11
#define SIGSEGV   12
#define SIGSTOP   13
#define SIGALRM   14
#define SIGTERM   15
#define SIGTSTP   16
#define SIGTTIN   17
#define SIGTTOU   18
#define SIGUSR1   19
#define SIGUSR2   20
#define SIGIO     21
#define SIGPOLL   SIGIO
#define SIGPROF   22
#define SIGSYS    23
#define SIGCANCEL SIGSYS
#define SIGTRAP   24
#define SIGURG    25
#define SIGVTALRM 26
#define SIGXCPU   27
#define SIGXFSZ   28
#define SIGWINCH  29
#define SIGPWR    30

#define BUS_ADRALN 1
#define BUS_ADRERR 2
#define BUS_OBJERR 3

#define ILL_ILLOPC   1
#define ILL_ILLOPN   2
#define ILL_ILLADR   3
#define ILL_ILLTRP   4
#define ILL_PRVOPC   5
#define ILL_PRVREG   6
#define ILL_COPROC   7
#define ILL_BADSTK   8
#define ILL_BADIADDR 9

#define SEGV_MAPERR 1
#define SEGV_ACCERR 2

#define SIG_BLOCK   1
#define SIG_UNBLOCK 2
#define SIG_SETMASK 3

#define SA_NOCLDSTOP (1 << 0)
#define SA_ONSTACK   (1 << 1)
#define SA_RESETHAND (1 << 2)
#define SA_RESTART   (1 << 3)
#define SA_SIGINFO   (1 << 4)
#define SA_NOCLDWAIT (1 << 5)
#define SA_NODEFER   (1 << 6)

#define MINSIGSTKSZ 2048
#define SIGSTKSZ    8192
#define SS_ONSTACK  1
#define SS_DISABLE  2

#define SIGEV_NONE   1
#define SIGEV_SIGNAL 2
#define SIGEV_THREAD 3

#define SI_ASYNCNL (-60)
#define SI_TKILL   (-6)
#define SI_SIGIO   (-5)
#define SI_ASYNCIO (-4)
#define SI_MESGQ   (-3)
#define SI_TIMER   (-2)
#define SI_QUEUE   (-1)
#define SI_USER    0
#define SI_KERNEL  128

#define NSIG 65

#define CLD_EXITED    1
#define CLD_KILLED    2
#define CLD_DUMPED    3
#define CLD_TRAPPED   4
#define CLD_STOPPED   5
#define CLD_CONTINUED 6

typedef uint64_t sigset_t;

union sigval {
    int sival_int;
    void* sival_ptr;
};

struct sigevent {
    int sigev_notify;
    int sigev_signo;
    union sigval sigev_value;
    void (*sigev_notify_function)(union sigval);
    pthread_attr_t* sigev_notify_attributes;
};

typedef struct {
    int si_signo;
    int si_code;
    int si_errno;
    pid_t si_pid;
    uid_t si_uid;
    void* si_addr;
    int si_status;
    union sigval si_value;
} siginfo_t;

typedef struct __stack {
    void* ss_sp;
    size_t ss_size;
    int32_t ss_flags;
} stack_t;

struct sigaction {
    union {
        void (*sa_handler)(int);
        void (*sa_sigaction)(int, siginfo_t*, void*);
    };
    void (*sa_restorer)(void);
    sigset_t sa_mask;
    int sa_flags;
};
