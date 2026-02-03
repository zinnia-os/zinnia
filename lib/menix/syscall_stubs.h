#pragma once

#include <menix/status.h>
#include <stdint.h>

#ifdef __x86_64__
#define ASM_REG_NUM "rax"
#define ASM_REG_RET "rax"
#define ASM_REG_A0  "rdi"
#define ASM_REG_A1  "rsi"
#define ASM_REG_A2  "rdx"
#define ASM_REG_A3  "r9"
#define ASM_REG_A4  "r8"
#define ASM_REG_A5  "r10"
#define ASM_SYSCALL "syscall"
#define ASM_CLOBBER "rcx", "r11"
typedef uint64_t arg_t;
#elif defined(__aarch64__)
#define ASM_REG_NUM "x8"
#define ASM_REG_RET "x0"
#define ASM_REG_A0  "x0"
#define ASM_REG_A1  "x1"
#define ASM_REG_A2  "x2"
#define ASM_REG_A3  "x3"
#define ASM_REG_A4  "x4"
#define ASM_REG_A5  "x5"
#define ASM_SYSCALL "svc 0"
#define ASM_CLOBBER
typedef uint64_t arg_t;
#elif defined(__riscv) && (__riscv_xlen == 64)
#define ASM_REG_NUM "a7"
#define ASM_REG_RET "a0"
#define ASM_REG_A0  "a0"
#define ASM_REG_A1  "a1"
#define ASM_REG_A2  "a2"
#define ASM_REG_A3  "a3"
#define ASM_REG_A4  "a4"
#define ASM_REG_A5  "a5"
#define ASM_SYSCALL "ecall"
#define ASM_CLOBBER
typedef uint64_t arg_t;
#elif defined(__loongarch64)
#define ASM_REG_NUM "a7"
#define ASM_REG_RET "a0"
#define ASM_REG_A0  "a0"
#define ASM_REG_A1  "a1"
#define ASM_REG_A2  "a2"
#define ASM_REG_A3  "a3"
#define ASM_REG_A4  "a4"
#define ASM_REG_A5  "a5"
#define ASM_SYSCALL "syscall 0"
#define ASM_CLOBBER
typedef uint64_t arg_t;
#else
#error "Unsupported architecture!"
#endif

static inline menix_status_t syscall0(arg_t num) {
    register arg_t rnum asm(ASM_REG_NUM) = num;
    register arg_t value asm(ASM_REG_RET);
    asm volatile(ASM_SYSCALL : "=r"(value) : "r"(rnum) : "memory", ASM_CLOBBER);

    return value;
}

static inline menix_status_t syscall1(arg_t num, arg_t a0) {
    register arg_t rnum asm(ASM_REG_NUM) = num;
    register arg_t value asm(ASM_REG_RET);
    register arg_t r0 asm(ASM_REG_A0) = a0;
    asm volatile(ASM_SYSCALL : "=r"(value) : "r"(rnum), "r"(r0) : "memory", ASM_CLOBBER);

    return value;
}

static inline menix_status_t syscall2(arg_t num, arg_t a0, arg_t a1) {
    register arg_t rnum asm(ASM_REG_NUM) = num;
    register arg_t value asm(ASM_REG_RET);
    register arg_t r0 asm(ASM_REG_A0) = a0;
    register arg_t r1 asm(ASM_REG_A1) = a1;
    asm volatile(ASM_SYSCALL : "=r"(value) : "r"(rnum), "r"(r0), "r"(r1) : "memory", ASM_CLOBBER);

    return value;
}

static inline menix_status_t syscall3(arg_t num, arg_t a0, arg_t a1, arg_t a2) {
    register arg_t rnum asm(ASM_REG_NUM) = num;
    register arg_t value asm(ASM_REG_RET);
    register arg_t r0 asm(ASM_REG_A0) = a0;
    register arg_t r1 asm(ASM_REG_A1) = a1;
    register arg_t r2 asm(ASM_REG_A2) = a2;
    asm volatile(ASM_SYSCALL : "=r"(value) : "r"(rnum), "r"(r0), "r"(r1), "r"(r2) : "memory", ASM_CLOBBER);

    return value;
}

static inline menix_status_t syscall4(arg_t num, arg_t a0, arg_t a1, arg_t a2, arg_t a3) {
    register arg_t rnum asm(ASM_REG_NUM) = num;
    register arg_t value asm(ASM_REG_RET);
    register arg_t r0 asm(ASM_REG_A0) = a0;
    register arg_t r1 asm(ASM_REG_A1) = a1;
    register arg_t r2 asm(ASM_REG_A2) = a2;
    register arg_t r3 asm(ASM_REG_A3) = a3;
    asm volatile(ASM_SYSCALL : "=r"(value) : "r"(rnum), "r"(r0), "r"(r1), "r"(r2), "r"(r3) : "memory", ASM_CLOBBER);

    return value;
}

static inline menix_status_t syscall5(arg_t num, arg_t a0, arg_t a1, arg_t a2, arg_t a3, arg_t a4) {
    register arg_t rnum asm(ASM_REG_NUM) = num;
    register arg_t value asm(ASM_REG_RET);
    register arg_t r0 asm(ASM_REG_A0) = a0;
    register arg_t r1 asm(ASM_REG_A1) = a1;
    register arg_t r2 asm(ASM_REG_A2) = a2;
    register arg_t r3 asm(ASM_REG_A3) = a3;
    register arg_t r4 asm(ASM_REG_A4) = a4;
    asm volatile(ASM_SYSCALL
                 : "=r"(value)
                 : "r"(rnum), "r"(r0), "r"(r1), "r"(r2), "r"(r3), "r"(r4)
                 : "memory", ASM_CLOBBER);

    return value;
}

static inline menix_status_t syscall6(arg_t num, arg_t a0, arg_t a1, arg_t a2, arg_t a3, arg_t a4, arg_t a5) {
    register arg_t rnum asm(ASM_REG_NUM) = num;
    register arg_t value asm(ASM_REG_RET);
    register arg_t r0 asm(ASM_REG_A0) = a0;
    register arg_t r1 asm(ASM_REG_A1) = a1;
    register arg_t r2 asm(ASM_REG_A2) = a2;
    register arg_t r3 asm(ASM_REG_A3) = a3;
    register arg_t r4 asm(ASM_REG_A4) = a4;
    register arg_t r5 asm(ASM_REG_A5) = a5;
    asm volatile(ASM_SYSCALL
                 : "=r"(value)
                 : "r"(rnum), "r"(r0), "r"(r1), "r"(r2), "r"(r3), "r"(r4), "r"(r5)
                 : "memory", ASM_CLOBBER);

    return value;
}
