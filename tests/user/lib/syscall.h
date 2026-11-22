// SPDX-License-Identifier: BSD-3-Clause

#pragma once

#include <stddef.h>
#include <stdint.h>
#include <zinnia/syscall.h>

enum syslog_kind : uintptr_t {
    SYSLOG_ERR = 3,
    SYSLOG_INFO = 6,
};

struct sys_ret {
    uintptr_t value;
    uintptr_t error;
};

static inline struct sys_ret syscall6(
    uintptr_t num,
    uintptr_t a0,
    uintptr_t a1,
    uintptr_t a2,
    uintptr_t a3,
    uintptr_t a4,
    uintptr_t a5
) {
    register uintptr_t r10 asm("r10") = a3;
    register uintptr_t r8 asm("r8") = a4;
    register uintptr_t r9 asm("r9") = a5;
    uintptr_t value;
    uintptr_t error;
    asm volatile("syscall"
                 : "=a"(value), "=d"(error)
                 : "a"(num), "D"(a0), "S"(a1), "d"(a2), "r"(r10), "r"(r8), "r"(r9)
                 : "rcx", "r11", "memory");
    return (struct sys_ret){value, error};
}

static inline struct sys_ret syscall3(uintptr_t num, uintptr_t a0, uintptr_t a1, uintptr_t a2) {
    return syscall6(num, a0, a1, a2, 0, 0, 0);
}

static inline struct sys_ret syscall4(uintptr_t num, uintptr_t a0, uintptr_t a1, uintptr_t a2, uintptr_t a3) {
    return syscall6(num, a0, a1, a2, a3, 0, 0);
}

static inline struct sys_ret syscall5(
    uintptr_t num,
    uintptr_t a0,
    uintptr_t a1,
    uintptr_t a2,
    uintptr_t a3,
    uintptr_t a4
) {
    return syscall6(num, a0, a1, a2, a3, a4, 0);
}

[[noreturn]]
static inline void sys_exit(int status) {
    syscall3(__NR_exit, (uintptr_t)status, 0, 0);
    __builtin_unreachable();
}

static inline struct sys_ret sys_syslog(enum syslog_kind kind, const char* msg, size_t len) {
    return syscall3(__NR_syslog, kind, (uintptr_t)msg, len);
}

static inline struct sys_ret sys_mmap(void* addr, size_t len, uint32_t prot, uint32_t flags, int fd, int64_t off) {
    return syscall6(__NR_mmap, (uintptr_t)addr, len, prot, flags, (uintptr_t)fd, (uintptr_t)off);
}

static inline struct sys_ret sys_munmap(void* addr, size_t len) {
    return syscall3(__NR_munmap, (uintptr_t)addr, len, 0);
}

static inline struct sys_ret sys_openat(int dirfd, const char* path, uint32_t flags, uint32_t mode) {
    return syscall4(__NR_openat, (uintptr_t)dirfd, (uintptr_t)path, flags, mode);
}

static inline struct sys_ret sys_close(int fd) {
    return syscall3(__NR_close, (uintptr_t)fd, 0, 0);
}

static inline struct sys_ret sys_preadv(int fd, const void* iov, size_t count, uint64_t off) {
    return syscall4(__NR_preadv, (uintptr_t)fd, (uintptr_t)iov, count, off);
}

static inline struct sys_ret sys_fstat(int fd, void* buf) {
    return syscall3(__NR_fstat, (uintptr_t)fd, (uintptr_t)buf, 0);
}

static inline struct sys_ret sys_getuname(void* buf) {
    return syscall3(__NR_getuname, (uintptr_t)buf, 0, 0);
}

static inline struct sys_ret sys_setuname(const void* buf) {
    return syscall3(__NR_setuname, (uintptr_t)buf, 0, 0);
}

static inline struct sys_ret sys_mprotect(void* addr, size_t len, uint32_t prot) {
    return syscall3(__NR_mprotect, (uintptr_t)addr, len, prot);
}

static inline struct sys_ret sys_madvise(void* addr, size_t len, int advice) {
    return syscall3(__NR_madvise, (uintptr_t)addr, len, (uintptr_t)advice);
}

static inline struct sys_ret sys_msync(void* addr, size_t len, int flags) {
    return syscall3(__NR_msync, (uintptr_t)addr, len, (uintptr_t)flags);
}

static inline struct sys_ret sys_pwritev(int fd, const void* iov, size_t count, uint64_t off) {
    return syscall4(__NR_pwritev, (uintptr_t)fd, (uintptr_t)iov, count, off);
}

static inline struct sys_ret sys_readv(int fd, const void* iov, size_t count) {
    return syscall3(__NR_readv, (uintptr_t)fd, (uintptr_t)iov, count);
}

static inline struct sys_ret sys_writev(int fd, const void* iov, size_t count) {
    return syscall3(__NR_writev, (uintptr_t)fd, (uintptr_t)iov, count);
}

static inline struct sys_ret sys_seek(int fd, int64_t off, int whence) {
    return syscall3(__NR_seek, (uintptr_t)fd, (uintptr_t)off, (uintptr_t)whence);
}

static inline struct sys_ret sys_fstatat(int dirfd, const char* path, void* buf, uint32_t flags) {
    return syscall4(__NR_fstatat, (uintptr_t)dirfd, (uintptr_t)path, (uintptr_t)buf, flags);
}

static inline struct sys_ret sys_faccessat(int dirfd, const char* path, uint32_t mode, uint32_t flags) {
    return syscall4(__NR_faccessat, (uintptr_t)dirfd, (uintptr_t)path, mode, flags);
}

static inline struct sys_ret sys_fcntl(int fd, int cmd, uintptr_t arg) {
    return syscall3(__NR_fcntl, (uintptr_t)fd, (uintptr_t)cmd, arg);
}

static inline struct sys_ret sys_ftruncate(int fd, int64_t len) {
    return syscall3(__NR_ftruncate, (uintptr_t)fd, (uintptr_t)len, 0);
}

static inline struct sys_ret sys_truncate(const char* path, int64_t len) {
    return syscall3(__NR_truncate, (uintptr_t)path, (uintptr_t)len, 0);
}

static inline struct sys_ret sys_getcwd(char* buf, size_t len) {
    return syscall3(__NR_getcwd, (uintptr_t)buf, len, 0);
}

static inline struct sys_ret sys_chdir(const char* path) {
    return syscall3(__NR_chdir, (uintptr_t)path, 0, 0);
}

static inline struct sys_ret sys_fchdir(int fd) {
    return syscall3(__NR_fchdir, (uintptr_t)fd, 0, 0);
}

static inline struct sys_ret sys_mkdirat(int dirfd, const char* path, uint32_t mode) {
    return syscall3(__NR_mkdirat, (uintptr_t)dirfd, (uintptr_t)path, mode);
}

static inline struct sys_ret sys_rmdirat(int dirfd, const char* path) {
    return syscall3(__NR_rmdirat, (uintptr_t)dirfd, (uintptr_t)path, 0);
}

static inline struct sys_ret sys_unlinkat(int dirfd, const char* path, uint32_t flags) {
    return syscall3(__NR_unlinkat, (uintptr_t)dirfd, (uintptr_t)path, flags);
}

static inline struct sys_ret sys_getdents(int fd, void* buf, size_t len) {
    return syscall3(__NR_getdents, (uintptr_t)fd, (uintptr_t)buf, len);
}

static inline struct sys_ret sys_renameat(int old_dirfd, const char* old_path, int new_dirfd, const char* new_path) {
    return syscall4(
        __NR_renameat,
        (uintptr_t)old_dirfd,
        (uintptr_t)old_path,
        (uintptr_t)new_dirfd,
        (uintptr_t)new_path
    );
}

static inline struct sys_ret sys_linkat(
    int old_dirfd,
    const char* old_path,
    int new_dirfd,
    const char* new_path,
    uint32_t flags
) {
    return syscall5(
        __NR_linkat,
        (uintptr_t)old_dirfd,
        (uintptr_t)old_path,
        (uintptr_t)new_dirfd,
        (uintptr_t)new_path,
        flags
    );
}

static inline struct sys_ret sys_symlinkat(const char* target, int dirfd, const char* path) {
    return syscall3(__NR_symlinkat, (uintptr_t)target, (uintptr_t)dirfd, (uintptr_t)path);
}

static inline struct sys_ret sys_readlinkat(int dirfd, const char* path, char* buf, size_t len) {
    return syscall4(__NR_readlinkat, (uintptr_t)dirfd, (uintptr_t)path, (uintptr_t)buf, len);
}

static inline struct sys_ret sys_dup(int fd) {
    return syscall3(__NR_dup, (uintptr_t)fd, 0, 0);
}

static inline struct sys_ret sys_dup3(int old_fd, int new_fd, uint32_t flags) {
    return syscall3(__NR_dup3, (uintptr_t)old_fd, (uintptr_t)new_fd, flags);
}

static inline struct sys_ret sys_umask(uint32_t mask) {
    return syscall3(__NR_umask, mask, 0, 0);
}

static inline struct sys_ret sys_pipe(int* fds, uint32_t flags) {
    return syscall3(__NR_pipe, (uintptr_t)fds, flags, 0);
}

static inline struct sys_ret sys_ppoll(void* fds, size_t nfds, const void* timeout, const void* sigmask) {
    return syscall4(__NR_ppoll, (uintptr_t)fds, nfds, (uintptr_t)timeout, (uintptr_t)sigmask);
}

static inline struct sys_ret sys_fork() {
    return syscall3(__NR_fork, 0, 0, 0);
}

static inline struct sys_ret sys_execve(const char* path, const char* const* argv, const char* const* envp) {
    return syscall3(__NR_execve, (uintptr_t)path, (uintptr_t)argv, (uintptr_t)envp);
}

static inline struct sys_ret sys_waitpid(int pid, int* status, int options) {
    return syscall4(__NR_waitpid, (uintptr_t)pid, (uintptr_t)status, (uintptr_t)options, 0);
}

static inline struct sys_ret sys_waitid(int idtype, int id, void* info, int options) {
    return syscall4(__NR_waitid, (uintptr_t)idtype, (uintptr_t)id, (uintptr_t)info, (uintptr_t)options);
}

static inline struct sys_ret sys_kill(int pid, int sig) {
    return syscall3(__NR_kill, (uintptr_t)pid, (uintptr_t)sig, 0);
}

static inline int sys_getpid() {
    return (int)syscall3(__NR_getpid, 0, 0, 0).value;
}

static inline int sys_getppid() {
    return (int)syscall3(__NR_getppid, 0, 0, 0).value;
}

static inline int sys_gettid() {
    return (int)syscall3(__NR_gettid, 0, 0, 0).value;
}

static inline struct sys_ret sys_setsid() {
    return syscall3(__NR_setsid, 0, 0, 0);
}

static inline struct sys_ret sys_getsid(int pid) {
    return syscall3(__NR_getsid, (uintptr_t)pid, 0, 0);
}

static inline struct sys_ret sys_setpgid(int pid, int pgid) {
    return syscall3(__NR_setpgid, (uintptr_t)pid, (uintptr_t)pgid, 0);
}

static inline struct sys_ret sys_getpgid(int pid) {
    return syscall3(__NR_getpgid, (uintptr_t)pid, 0, 0);
}

static inline uint32_t sys_getuid() {
    return (uint32_t)syscall3(__NR_getuid, 0, 0, 0).value;
}

static inline uint32_t sys_geteuid() {
    return (uint32_t)syscall3(__NR_geteuid, 0, 0, 0).value;
}

static inline struct sys_ret sys_setuid(uint32_t uid) {
    return syscall3(__NR_setuid, uid, 0, 0);
}

static inline struct sys_ret sys_thread_create(void (*entry)(), void* stack) {
    return syscall3(__NR_thread_create, (uintptr_t)entry, (uintptr_t)stack, 0);
}

[[noreturn]]
static inline void sys_thread_exit() {
    syscall3(__NR_thread_exit, 0, 0, 0);
    __builtin_unreachable();
}

static inline struct sys_ret sys_futex_wait(volatile int* addr, int expected) {
    return syscall3(__NR_futex_wait, (uintptr_t)addr, (uintptr_t)expected, 0);
}

static inline struct sys_ret sys_futex_wake(volatile int* addr, uint32_t count) {
    return syscall3(__NR_futex_wake, (uintptr_t)addr, count, 0);
}

static inline struct sys_ret sys_sigsuspend(const uint64_t* mask) {
    return syscall3(__NR_sigsuspend, (uintptr_t)mask, 0, 0);
}

static inline struct sys_ret sys_getresuid(uint32_t* r, uint32_t* e, uint32_t* s) {
    return syscall3(__NR_getresuid, (uintptr_t)r, (uintptr_t)e, (uintptr_t)s);
}

static inline struct sys_ret sys_getresgid(uint32_t* r, uint32_t* e, uint32_t* s) {
    return syscall3(__NR_getresgid, (uintptr_t)r, (uintptr_t)e, (uintptr_t)s);
}
