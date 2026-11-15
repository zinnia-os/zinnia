// SPDX-License-Identifier: BSD-3-Clause

#include <stdint.h>
#include <string.h>
#include <tests/user/lib/syscall.h>
#include <tests/user/lib/utest.h>
#include <zinnia/errno.h>
#include <zinnia/fcntl.h>
#include <zinnia/mman.h>

constexpr size_t PAGE_SIZE = 0x1000;
constexpr uint32_t PROT_RW = __PROT_READ | __PROT_WRITE;
constexpr uint32_t MAP_ANON_PRIVATE = __MAP_PRIVATE | __MAP_ANON;

static uint8_t* map_anon(size_t len) {
    struct sys_ret ret = sys_mmap(nullptr, len, PROT_RW, MAP_ANON_PRIVATE, -1, 0);
    return ret.error ? nullptr : (uint8_t*)ret.value;
}

UTEST(mman, anonymous_mapping) {
    const size_t len = 4 * PAGE_SIZE;
    struct sys_ret ret = sys_mmap(nullptr, len, __PROT_READ | __PROT_WRITE, __MAP_PRIVATE | __MAP_ANON, -1, 0);
    ASSERT_EQ(ret.error, 0);

    uint8_t* mem = (uint8_t*)ret.value;
    EXPECT_EQ((uintptr_t)mem % PAGE_SIZE, 0);
    for (size_t i = 0; i < len; i += PAGE_SIZE)
        EXPECT_EQ(mem[i], 0);
    for (size_t i = 0; i < len; i++)
        mem[i] = (uint8_t)i;
    for (size_t i = 0; i < len; i++) {
        if (mem[i] != (uint8_t)i) {
            utest_fail(test, __FILE__, __LINE__, "byte %zu reads back as %u", i, mem[i]);
            break;
        }
    }

    EXPECT_EQ(sys_munmap(mem, len).error, 0);
}

UTEST(mman, rejects_zero_length) {
    struct sys_ret ret = sys_mmap(nullptr, 0, __PROT_READ, __MAP_PRIVATE | __MAP_ANON, -1, 0);
    EXPECT_EQ(ret.error, __EINVAL);
}

UTEST(mman, shared_anonymous_mapping) {
    struct sys_ret ret = sys_mmap(nullptr, PAGE_SIZE, PROT_RW, __MAP_SHARED | __MAP_ANON, -1, 0);
    ASSERT_EQ(ret.error, 0);
    uint8_t* mem = (uint8_t*)ret.value;
    EXPECT_EQ(mem[0], 0);
    mem[0] = 0x5a;
    mem[PAGE_SIZE - 1] = 0xa5;
    EXPECT_EQ(mem[0], 0x5a);
    EXPECT_EQ(mem[PAGE_SIZE - 1], 0xa5);
    EXPECT_EQ(sys_munmap(mem, PAGE_SIZE).error, 0);
}

UTEST(mman, file_mapping) {
    struct sys_ret ret = sys_openat(__AT_FDCWD, "/init", __O_RDONLY, 0);
    ASSERT_EQ(ret.error, 0);
    const int fd = (int)ret.value;

    ret = sys_mmap(nullptr, PAGE_SIZE, __PROT_READ, __MAP_PRIVATE, fd, 0);
    EXPECT_EQ(ret.error, 0);
    if (!ret.error) {
        const uint8_t elf_header[] = {0x7f, 'E', 'L', 'F'};
        EXPECT_EQ(memcmp((const void*)ret.value, elf_header, sizeof(elf_header)), 0);
        EXPECT_EQ(sys_munmap((void*)ret.value, PAGE_SIZE).error, 0);
    }
    EXPECT_EQ(sys_close(fd).error, 0);
}

UTEST(mman, fixed_mapping_replaces) {
    uint8_t* mem = map_anon(3 * PAGE_SIZE);
    ASSERT_TRUE(mem != nullptr);
    memset(mem, 0xcc, 3 * PAGE_SIZE);

    struct sys_ret ret = sys_mmap(mem + PAGE_SIZE, PAGE_SIZE, PROT_RW, MAP_ANON_PRIVATE | __MAP_FIXED, -1, 0);
    EXPECT_EQ(ret.error, 0);
    EXPECT_EQ(ret.value, (uintptr_t)(mem + PAGE_SIZE));
    EXPECT_EQ(mem[0], 0xcc);
    EXPECT_EQ(mem[PAGE_SIZE], 0);
    EXPECT_EQ(mem[2 * PAGE_SIZE], 0xcc);
    EXPECT_EQ(sys_munmap(mem, 3 * PAGE_SIZE).error, 0);
}

UTEST(mman, partial_unmap) {
    uint8_t* mem = map_anon(3 * PAGE_SIZE);
    ASSERT_TRUE(mem != nullptr);
    EXPECT_EQ(sys_munmap(mem + PAGE_SIZE, PAGE_SIZE).error, 0);
    mem[0] = 1;
    mem[2 * PAGE_SIZE] = 2;
    EXPECT_EQ(mem[0], 1);
    EXPECT_EQ(mem[2 * PAGE_SIZE], 2);
    EXPECT_EQ(sys_munmap(mem, 3 * PAGE_SIZE).error, 0);
}

UTEST(mman, rejects_invalid_arguments) {
    EXPECT_EQ(sys_mmap(nullptr, PAGE_SIZE, 0x80, MAP_ANON_PRIVATE, -1, 0).error, __EINVAL);
    EXPECT_EQ(sys_mmap(nullptr, PAGE_SIZE, PROT_RW, __MAP_ANON, -1, 0).error, __EINVAL);
    EXPECT_EQ(sys_mmap(nullptr, PAGE_SIZE, PROT_RW, MAP_ANON_PRIVATE, -1, 1).error, __EINVAL);
    EXPECT_EQ(sys_mmap((void*)0x1001, PAGE_SIZE, PROT_RW, MAP_ANON_PRIVATE | __MAP_FIXED, -1, 0).error, __EINVAL);
    EXPECT_EQ(sys_mmap(nullptr, PAGE_SIZE, __PROT_READ, __MAP_PRIVATE, 1234, 0).error, __EBADF);
}

UTEST(mman, mprotect) {
    uint8_t* mem = map_anon(2 * PAGE_SIZE);
    ASSERT_TRUE(mem != nullptr);
    mem[0] = 7;
    EXPECT_EQ(sys_mprotect(mem, PAGE_SIZE, __PROT_READ).error, 0);
    EXPECT_EQ(mem[0], 7);
    EXPECT_EQ(sys_mprotect(mem, PAGE_SIZE, PROT_RW).error, 0);
    mem[0] = 8;
    EXPECT_EQ(mem[0], 8);
    EXPECT_EQ(sys_mprotect(mem, PAGE_SIZE, 0x80).error, __EINVAL);
    EXPECT_EQ(sys_mprotect(mem, 0, __PROT_READ).error, __EINVAL);
    EXPECT_EQ(sys_munmap(mem, 2 * PAGE_SIZE).error, 0);
}

UTEST(mman, msync) {
    uint8_t* mem = map_anon(PAGE_SIZE);
    ASSERT_TRUE(mem != nullptr);
    EXPECT_EQ(sys_msync(mem, PAGE_SIZE, __MS_SYNC).error, 0);
    EXPECT_EQ(sys_msync(mem, PAGE_SIZE, __MS_ASYNC).error, 0);
    EXPECT_EQ(sys_msync(mem, PAGE_SIZE, __MS_ASYNC | __MS_SYNC).error, __EINVAL);
    EXPECT_EQ(sys_msync(mem + 1, PAGE_SIZE, __MS_SYNC).error, __EINVAL);
    EXPECT_EQ(sys_munmap(mem, PAGE_SIZE).error, 0);
    EXPECT_EQ(sys_msync(mem, PAGE_SIZE, __MS_ASYNC).error, __ENOMEM);
}

UTEST(mman, madvise) {
    uint8_t* mem = map_anon(PAGE_SIZE);
    ASSERT_TRUE(mem != nullptr);
    EXPECT_EQ(sys_madvise(mem, PAGE_SIZE, 0).error, 0);
    EXPECT_EQ(sys_madvise(mem + 1, PAGE_SIZE, 0).error, __EINVAL);
    EXPECT_EQ(sys_munmap(mem, PAGE_SIZE).error, 0);
}
