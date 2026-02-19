#pragma once

#include <zinnia/status.h>
#include <common/syscall_numbers.h>
#include <stdint.h>

#ifdef __x86_64__
typedef uint64_t zn_arg_t;
#elif defined(__aarch64__)
typedef uint64_t zn_arg_t;
#elif defined(__riscv) && (__riscv_xlen == 64)
typedef uint64_t zn_arg_t;
#elif defined(__loongarch64)
typedef uint64_t zn_arg_t;
#else
#error "Unsupported architecture!"
#endif

#ifdef __x86_64__
#define ASM_REG_NUM "rax"
#define ASM_REG_RET "rax"
#define ASM_REG_A0  "rdi"
#define ASM_REG_A1  "rsi"
#define ASM_REG_A2  "rdx"
#define ASM_REG_A3  "r9"
#define ASM_REG_A4  "r8"
#define ASM_REG_A5  "r10"
#define ASM_REG_A6  "r12"
#define ASM_REG_A7  "r13"
#define ASM_SYSCALL "syscall"
#define ASM_CLOBBER "rcx", "r11"
#elif defined(__aarch64__)
#define ASM_REG_NUM "x8"
#define ASM_REG_RET "x0"
#define ASM_REG_A0  "x0"
#define ASM_REG_A1  "x1"
#define ASM_REG_A2  "x2"
#define ASM_REG_A3  "x3"
#define ASM_REG_A4  "x4"
#define ASM_REG_A5  "x5"
#define ASM_REG_A6  "x6"
#define ASM_REG_A7  "x7"
#define ASM_SYSCALL "svc 0"
#define ASM_CLOBBER
#elif defined(__riscv) && (__riscv_xlen == 64)
#define ASM_REG_NUM "t3"
#define ASM_REG_RET "a0"
#define ASM_REG_A0  "a0"
#define ASM_REG_A1  "a1"
#define ASM_REG_A2  "a2"
#define ASM_REG_A3  "a3"
#define ASM_REG_A4  "a4"
#define ASM_REG_A5  "a5"
#define ASM_REG_A5  "a6"
#define ASM_REG_A5  "a7"
#define ASM_SYSCALL "ecall"
#define ASM_CLOBBER
#elif defined(__loongarch64)
#define ASM_REG_NUM "a7"
#define ASM_REG_RET "a0"
#define ASM_REG_A0  "a0"
#define ASM_REG_A1  "a1"
#define ASM_REG_A2  "a2"
#define ASM_REG_A3  "a3"
#define ASM_REG_A4  "a4"
#define ASM_REG_A5  "a5"
// TODO: Define ABI
#define ASM_REG_A6  TODO
#define ASM_REG_A7  TODO
#define ASM_SYSCALL "syscall 0"
#define ASM_CLOBBER
#else
#error "Unsupported architecture!"
#endif

static inline zn_status_t zn_syscall0(zn_syscall_t num) {
    register zn_syscall_t rnum asm(ASM_REG_NUM) = num;
    register zn_status_t value asm(ASM_REG_RET);
    asm volatile(ASM_SYSCALL : "=r"(value) : "r"(rnum) : "memory", ASM_CLOBBER);
    return value;
}

static inline zn_status_t zn_syscall1(zn_arg_t a0, zn_syscall_t num) {
    register zn_syscall_t rnum asm(ASM_REG_NUM) = num;
    register zn_status_t value asm(ASM_REG_RET);
    register zn_arg_t r0 asm(ASM_REG_A0) = a0;
    asm volatile(ASM_SYSCALL : "=r"(value) : "r"(rnum), "r"(r0) : "memory", ASM_CLOBBER);
    return value;
}
#define zn_syscall1(a0, num) zn_syscall1((zn_arg_t)a0, num)

static inline zn_status_t zn_syscall2(zn_arg_t a0, zn_arg_t a1, zn_syscall_t num) {
    register zn_syscall_t rnum asm(ASM_REG_NUM) = num;
    register zn_status_t value asm(ASM_REG_RET);
    register zn_arg_t r0 asm(ASM_REG_A0) = a0;
    register zn_arg_t r1 asm(ASM_REG_A1) = a1;
    asm volatile(ASM_SYSCALL : "=r"(value) : "r"(rnum), "r"(r0), "r"(r1) : "memory", ASM_CLOBBER);
    return value;
}
#define zn_syscall2(a0, a1, num) zn_syscall2((zn_arg_t)a0, (zn_arg_t)a1, num)

static inline zn_status_t zn_syscall3(zn_arg_t a0, zn_arg_t a1, zn_arg_t a2, zn_syscall_t num) {
    register zn_syscall_t rnum asm(ASM_REG_NUM) = num;
    register zn_status_t value asm(ASM_REG_RET);
    register zn_arg_t r0 asm(ASM_REG_A0) = a0;
    register zn_arg_t r1 asm(ASM_REG_A1) = a1;
    register zn_arg_t r2 asm(ASM_REG_A2) = a2;
    asm volatile(ASM_SYSCALL : "=r"(value) : "r"(rnum), "r"(r0), "r"(r1), "r"(r2) : "memory", ASM_CLOBBER);
    return value;
}
#define zn_syscall3(a0, a1, a2, num) zn_syscall3((zn_arg_t)a0, (zn_arg_t)a1, (zn_arg_t)a2, num)

static inline zn_status_t zn_syscall4(zn_arg_t a0, zn_arg_t a1, zn_arg_t a2, zn_arg_t a3, zn_syscall_t num) {
    register zn_syscall_t rnum asm(ASM_REG_NUM) = num;
    register zn_status_t value asm(ASM_REG_RET);
    register zn_arg_t r0 asm(ASM_REG_A0) = a0;
    register zn_arg_t r1 asm(ASM_REG_A1) = a1;
    register zn_arg_t r2 asm(ASM_REG_A2) = a2;
    register zn_arg_t r3 asm(ASM_REG_A3) = a3;
    asm volatile(ASM_SYSCALL : "=r"(value) : "r"(rnum), "r"(r0), "r"(r1), "r"(r2), "r"(r3) : "memory", ASM_CLOBBER);
    return value;
}
#define zn_syscall4(a0, a1, a2, a3, num) zn_syscall4((zn_arg_t)a0, (zn_arg_t)a1, (zn_arg_t)a2, (zn_arg_t)a3, num)

static inline zn_status_t zn_syscall5(
    zn_arg_t a0,
    zn_arg_t a1,
    zn_arg_t a2,
    zn_arg_t a3,
    zn_arg_t a4,
    zn_syscall_t num
) {
    register zn_syscall_t rnum asm(ASM_REG_NUM) = num;
    register zn_status_t value asm(ASM_REG_RET);
    register zn_arg_t r0 asm(ASM_REG_A0) = a0;
    register zn_arg_t r1 asm(ASM_REG_A1) = a1;
    register zn_arg_t r2 asm(ASM_REG_A2) = a2;
    register zn_arg_t r3 asm(ASM_REG_A3) = a3;
    register zn_arg_t r4 asm(ASM_REG_A4) = a4;
    asm volatile(ASM_SYSCALL
                 : "=r"(value)
                 : "r"(rnum), "r"(r0), "r"(r1), "r"(r2), "r"(r3), "r"(r4)
                 : "memory", ASM_CLOBBER);
    return value;
}
#define zn_syscall5(a0, a1, a2, a3, a4, num) \
    zn_syscall5((zn_arg_t)a0, (zn_arg_t)a1, (zn_arg_t)a2, (zn_arg_t)a3, (zn_arg_t)a4, num)

static inline zn_status_t zn_syscall6(
    zn_arg_t a0,
    zn_arg_t a1,
    zn_arg_t a2,
    zn_arg_t a3,
    zn_arg_t a4,
    zn_arg_t a5,
    zn_syscall_t num
) {
    register zn_syscall_t rnum asm(ASM_REG_NUM) = num;
    register zn_status_t value asm(ASM_REG_RET);
    register zn_arg_t r0 asm(ASM_REG_A0) = a0;
    register zn_arg_t r1 asm(ASM_REG_A1) = a1;
    register zn_arg_t r2 asm(ASM_REG_A2) = a2;
    register zn_arg_t r3 asm(ASM_REG_A3) = a3;
    register zn_arg_t r4 asm(ASM_REG_A4) = a4;
    register zn_arg_t r5 asm(ASM_REG_A5) = a5;
    asm volatile(ASM_SYSCALL
                 : "=r"(value)
                 : "r"(rnum), "r"(r0), "r"(r1), "r"(r2), "r"(r3), "r"(r4), "r"(r5)
                 : "memory", ASM_CLOBBER);
    return value;
}
#define zn_syscall6(a0, a1, a2, a3, a4, a5, num) \
    zn_syscall6((zn_arg_t)a0, (zn_arg_t)a1, (zn_arg_t)a2, (zn_arg_t)a3, (zn_arg_t)a4, (zn_arg_t)a5, num)

static inline zn_status_t zn_syscall7(
    zn_arg_t a0,
    zn_arg_t a1,
    zn_arg_t a2,
    zn_arg_t a3,
    zn_arg_t a4,
    zn_arg_t a5,
    zn_arg_t a6,
    zn_syscall_t num
) {
    register zn_syscall_t rnum asm(ASM_REG_NUM) = num;
    register zn_status_t value asm(ASM_REG_RET);
    register zn_arg_t r0 asm(ASM_REG_A0) = a0;
    register zn_arg_t r1 asm(ASM_REG_A1) = a1;
    register zn_arg_t r2 asm(ASM_REG_A2) = a2;
    register zn_arg_t r3 asm(ASM_REG_A3) = a3;
    register zn_arg_t r4 asm(ASM_REG_A4) = a4;
    register zn_arg_t r5 asm(ASM_REG_A5) = a5;
    register zn_arg_t r6 asm(ASM_REG_A6) = a6;
    asm volatile(ASM_SYSCALL
                 : "=r"(value)
                 : "r"(rnum), "r"(r0), "r"(r1), "r"(r2), "r"(r3), "r"(r4), "r"(r5), "r"(r6)
                 : "memory", ASM_CLOBBER);
    return value;
}
#define zn_syscall7(a0, a1, a2, a3, a4, a5, a6, num) \
    zn_syscall7((zn_arg_t)a0, (zn_arg_t)a1, (zn_arg_t)a2, (zn_arg_t)a3, (zn_arg_t)a4, (zn_arg_t)a5, (zn_arg_t)a6, num)

static inline zn_status_t zn_syscall8(
    zn_arg_t a0,
    zn_arg_t a1,
    zn_arg_t a2,
    zn_arg_t a3,
    zn_arg_t a4,
    zn_arg_t a5,
    zn_arg_t a6,
    zn_arg_t a7,
    zn_syscall_t num
) {
    register zn_syscall_t rnum asm(ASM_REG_NUM) = num;
    register zn_status_t value asm(ASM_REG_RET);
    register zn_arg_t r0 asm(ASM_REG_A0) = a0;
    register zn_arg_t r1 asm(ASM_REG_A1) = a1;
    register zn_arg_t r2 asm(ASM_REG_A2) = a2;
    register zn_arg_t r3 asm(ASM_REG_A3) = a3;
    register zn_arg_t r4 asm(ASM_REG_A4) = a4;
    register zn_arg_t r5 asm(ASM_REG_A5) = a5;
    register zn_arg_t r6 asm(ASM_REG_A6) = a6;
    register zn_arg_t r7 asm(ASM_REG_A7) = a7;
    asm volatile(ASM_SYSCALL
                 : "=r"(value)
                 : "r"(rnum), "r"(r0), "r"(r1), "r"(r2), "r"(r3), "r"(r4), "r"(r5), "r"(r6), "r"(r7)
                 : "memory", ASM_CLOBBER);
    return value;
}
#define zn_syscall8(a0, a1, a2, a3, a4, a5, a6, a7, num) \
    zn_syscall8( \
        (zn_arg_t)a0, \
        (zn_arg_t)a1, \
        (zn_arg_t)a2, \
        (zn_arg_t)a3, \
        (zn_arg_t)a4, \
        (zn_arg_t)a5, \
        (zn_arg_t)a6, \
        (zn_arg_t)a7, \
        num \
    )
