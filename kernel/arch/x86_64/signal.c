// SPDX-License-Identifier: BSD-3-Clause

#include <arch/x86_64/asm.h>
#include <arch/x86_64/defs.h>
#include <arch/x86_64/percpu.h>
#include <lib/percpu.h>
#include <lib/utils.h>
#include <mem/alloc.h>
#include <mem/layout.h>
#include <mem/usercopy.h>
#include <bits/signal.h>
#include <stdckdint.h>
#include <stddef.h>
#include <string.h>

static_assert(sizeof(struct __mcontext_tag) == 16);
static_assert(sizeof(struct __mcontext_x86_64_fxsave_state) == 512);
static_assert(offsetof(__mcontext_t, mc_rip) == 120);
static_assert(offsetof(__mcontext_t, mc_fpregs) == 192);
static_assert(sizeof(__mcontext_t) == 704);
static_assert(sizeof(__ucontext_t) == 752);
static_assert(sizeof(__ucontext_t) % alignof(struct __mcontext_tag) == 0);
static_assert(offsetof(__mcontext_t, mc_rax) == offsetof(arch_context_t, rax));

constexpr size_t RED_ZONE = 128;
constexpr size_t FXSAVE_SIZE = sizeof(struct __mcontext_x86_64_fxsave_state);
constexpr size_t XSAVE_ALIGN = __MC_TAG_X86_64_XSAVE_ALIGN;
constexpr size_t GREGS_SIZE = offsetof(__mcontext_t, mc_rip);
constexpr size_t MAX_TAGS = 16;
constexpr uint32_t MXCSR_MASK_DEFAULT = 0xFFBF;
constexpr uint64_t RFLAGS_USER = 0xDD5;

struct fpu_buffer {
    void* raw;
    uint8_t* state;
    size_t size;
};

struct frame_layout {
    size_t ext_offset;
    size_t ext_size;
    size_t size;
};

static errno_t fpu_buffer_alloc(struct fpu_buffer* buf) {
    buf->size = percpu_ref(arch_data)->fpu_size;
    buf->raw = mem_alloc(buf->size + XSAVE_ALIGN, 0);
    if (!buf->raw)
        return ENOMEM;
    buf->state = (uint8_t*)ROUND_UP((uintptr_t)buf->raw, XSAVE_ALIGN);
    memset(buf->state, 0, buf->size);
    return 0;
}

static void fpu_buffer_free(struct fpu_buffer* buf) {
    mem_free(buf->raw, buf->size + XSAVE_ALIGN);
}

static struct frame_layout frame_layout() {
    const size_t fpu_size = percpu_ref(arch_data)->fpu_size;
    if (fpu_size <= FXSAVE_SIZE)
        return (struct frame_layout){.size = sizeof(__ucontext_t)};

    const size_t ext_offset = ROUND_UP(sizeof(__ucontext_t) + 2 * sizeof(struct __mcontext_tag), XSAVE_ALIGN);
    const size_t ext_size = fpu_size - FXSAVE_SIZE;
    return (struct frame_layout){.ext_offset = ext_offset, .ext_size = ext_size, .size = ext_offset + ext_size};
}

static bool on_alt_stack(const struct sig_delivery* delivery, uint64_t sp) {
    return delivery->alt_size && sp > delivery->alt_sp && sp - delivery->alt_sp <= delivery->alt_size;
}

static errno_t find_tag(uintptr_t uc_addr, uint32_t magic, struct __mcontext_tag* out) {
    for (size_t i = 0; i < MAX_TAGS; i++) {
        const uintptr_t addr = uc_addr + sizeof(__ucontext_t) + i * sizeof(*out);
        if (!usercopy_read(out, (const __user void*)addr, sizeof(*out)))
            return EFAULT;
        if (out->magic == __MC_TAG_END)
            return ENOENT;
        if (out->magic == magic)
            return 0;
    }
    return EFAULT;
}

static errno_t load_xsave(const __ucontext_t* uc, uintptr_t uc_addr, uint8_t* ext, size_t ext_size) {
    uint64_t* header = (uint64_t*)ext;
    if (uc->uc_flags & __UC_TAGS) {
        struct __mcontext_tag tag;
        const errno_t status = find_tag(uc_addr, __MC_TAG_X86_64_XSAVE, &tag);
        if (status && status != ENOENT)
            return status;

        uintptr_t addr;
        if (!status) {
            if (tag.size != ext_size || tag.offset < sizeof(__ucontext_t) || tag.offset % XSAVE_ALIGN ||
                ckd_add(&addr, uc_addr, tag.offset) || !usercopy_read(ext, (const __user void*)addr, ext_size))
                return EFAULT;
            if ((header[0] & ~asm_rdxcr(0)) || header[1] || header[2])
                return EFAULT;
        }
    }
    header[0] |= XCR0_X87 | XCR0_SSE;
    return 0;
}

size_t arch_signal_frame_size() {
    return ROUND_UP(sizeof(struct __siginfo), 16) + frame_layout().size + XSAVE_ALIGN + sizeof(uint64_t);
}

errno_t arch_signal_setup_frame(arch_context_t* ctx, const struct sig_delivery* delivery) {
    if (!arch_mem_addr_is_user(delivery->handler) || !arch_mem_addr_is_user(delivery->restorer))
        return EFAULT;

    const bool on_alt = on_alt_stack(delivery, ctx->rsp);
    const bool use_alt = (delivery->flags & __SA_ONSTACK) && delivery->alt_size && !on_alt;
    const uint64_t base = use_alt ? delivery->alt_sp + delivery->alt_size : ctx->rsp - RED_ZONE;
    if (base < arch_signal_frame_size() || !arch_mem_addr_is_user(base - 1))
        return EFAULT;

    const struct frame_layout layout = frame_layout();
    const uint64_t info_addr = ROUND_DOWN(base - sizeof(struct __siginfo), 16);
    const uint64_t uc_addr = ROUND_DOWN(info_addr - layout.size, XSAVE_ALIGN);
    const uint64_t ret_addr = uc_addr - sizeof(uint64_t);

    struct fpu_buffer fpu;
    errno_t status = fpu_buffer_alloc(&fpu);
    if (status)
        return status;
    percpu_ref(arch_data)->fpu_save(fpu.state);

    __ucontext_t uc = {
        .uc_flags = __UC_SIGMASK | __UC_STACK | __UC_CPU | __UC_TLSBASE | (layout.ext_size ? __UC_TAGS : 0),
        .uc_sigmask = delivery->old_mask,
        .uc_stack =
            {
                .ss_sp = (__user void*)delivery->alt_sp,
                .ss_size = delivery->alt_size,
                .ss_flags = on_alt       ? __SS_ONSTACK
                    : delivery->alt_size ? 0
                                         : __SS_DISABLE,
            },
        .uc_mcontext = {
            .mc_rip = ctx->rip,
            .mc_rflags = ctx->rflags,
            .mc_rsp = ctx->rsp,
            .mc_fsbase = asm_rdmsr(MSR_FS_BASE),
            .mc_gsbase = asm_rdmsr(MSR_KERNEL_GS_BASE),
            .mc_trapno = ctx->isr,
            .mc_err = ctx->error,
            .mc_cr2 = ctx->isr == IDT_PF ? (uint64_t)delivery->info.si_addr : 0,
        },
    };

    memcpy(&uc.uc_mcontext, ctx, GREGS_SIZE);
    memcpy(&uc.uc_mcontext.mc_fpregs, fpu.state, FXSAVE_SIZE);

    const struct __mcontext_tag tags[] = {
        {.magic = __MC_TAG_X86_64_XSAVE, .size = layout.ext_size, .offset = layout.ext_offset},
        {.magic = __MC_TAG_END},
    };
    const uint64_t restorer = delivery->restorer;

    bool ok = usercopy_write((__user void*)info_addr, &delivery->info, sizeof(delivery->info)) &&
        usercopy_write((__user void*)uc_addr, &uc, sizeof(uc)) &&
        usercopy_write((__user void*)ret_addr, &restorer, sizeof(restorer));

    if (ok && layout.ext_size)
        ok = usercopy_write((__user void*)(uc_addr + sizeof(uc)), tags, sizeof(tags)) &&
            usercopy_write((__user void*)(uc_addr + layout.ext_offset), fpu.state + FXSAVE_SIZE, layout.ext_size);
    fpu_buffer_free(&fpu);

    if (!ok)
        return EFAULT;

    ctx->rip = delivery->handler;
    ctx->rdi = delivery->sig;
    ctx->rsi = info_addr;
    ctx->rdx = uc_addr;
    ctx->rsp = ret_addr;
    ctx->rflags &= ~(uint64_t)RFLAGS_DF;
    return 0;
}

errno_t arch_signal_restore_context(arch_context_t* ctx, const __ucontext_t* uc, uintptr_t uc_addr) {
    const __mcontext_t* mc = &uc->uc_mcontext;
    const bool restore_cpu = uc->uc_flags & __UC_CPU;
    const bool restore_tls = uc->uc_flags & __UC_TLSBASE;

    if (restore_cpu && !arch_mem_addr_is_user(mc->mc_rip))
        return EFAULT;
    if (restore_tls && (!arch_mem_addr_is_user(mc->mc_fsbase) || !arch_mem_addr_is_user(mc->mc_gsbase)))
        return EFAULT;

    struct fpu_buffer fpu;
    errno_t status = fpu_buffer_alloc(&fpu);
    if (status)
        return status;

    struct arch_percpu* cpu = percpu_ref(arch_data);
    cpu->fpu_save(fpu.state);
    const uint32_t live_mask = ((const struct __mcontext_x86_64_fxsave_state*)fpu.state)->mxcsr_mask;
    const uint32_t mxcsr_mask = live_mask ? live_mask : MXCSR_MASK_DEFAULT;

    memcpy(fpu.state, &mc->mc_fpregs, FXSAVE_SIZE);
    status = mc->mc_fpregs.mxcsr & ~mxcsr_mask ? EFAULT : 0;

    if (!status && fpu.size > FXSAVE_SIZE) {
        memset(fpu.state + FXSAVE_SIZE, 0, fpu.size - FXSAVE_SIZE);
        status = load_xsave(uc, uc_addr, fpu.state + FXSAVE_SIZE, fpu.size - FXSAVE_SIZE);
    }

    if (status) {
        fpu_buffer_free(&fpu);
        return status;
    }

    if (restore_cpu) {
        memcpy(ctx, mc, GREGS_SIZE);
        ctx->rip = mc->mc_rip;
        ctx->rsp = mc->mc_rsp;
        ctx->rflags = (mc->mc_rflags & RFLAGS_USER) | 0x202;
    }

    if (restore_tls) {
        asm_wrmsr(MSR_FS_BASE, mc->mc_fsbase);
        asm_wrmsr(MSR_KERNEL_GS_BASE, mc->mc_gsbase);
    }

    cpu->fpu_restore(fpu.state);
    fpu_buffer_free(&fpu);
    return 0;
}

void arch_syscall_save(const arch_context_t* ctx, arch_syscall_restart_t* out) {
    out->rax = ctx->rax;
    out->rdx = ctx->rdx;
}

void arch_syscall_restart(arch_context_t* ctx, const arch_syscall_restart_t* restart) {
    ctx->rip -= 2; // 0f 05
    ctx->rax = restart->rax;
    ctx->rdx = restart->rdx;
}

void arch_syscall_set_return(arch_context_t* ctx, uintptr_t value, errno_t error) {
    ctx->rax = value;
    ctx->rdx = error;
}

bool arch_context_is_user(const arch_context_t* ctx) {
    return (ctx->cs & CPL_USER) == CPL_USER;
}
