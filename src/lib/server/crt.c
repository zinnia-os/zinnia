#include <zinnia/handle.h>
#include <zinnia/thread.h>
#include <common/compiler.h>
#include <common/elf.h>
#include <stdint.h>

extern void server_main(zn_handle_t);
extern void __zinnia_vdso_load(uintptr_t);

static void __zinnia_entry(uintptr_t* entry_stack) {
    uintptr_t* aux = entry_stack;
    aux += *aux + 1; // First, we skip argc and all args.
    aux++;

    while (*aux) { // Loop through the environment.
        aux++;
    }
    aux++;

    uintptr_t vdso_base = 0;
    zn_handle_t init_handle = ZN_HANDLE_INVALID;

    while (true) {
        uintptr_t* value = aux + 1;
        if (*aux == AT_NULL)
            break;
        if (*aux == AT_SYSINFO_EHDR)
            vdso_base = (uintptr_t)(*value);
        if (*aux == AT_INIT_HANDLE)
            init_handle = (zn_handle_t)(*value);
        aux += 2;
    }

    __zinnia_vdso_load(vdso_base);
    server_main(init_handle);
    zn_thread_exit();
    __unreachable();
}

// Entry point
[[__naked]]
void _start() {
#ifdef __x86_64__
    asm volatile(
        "mov rdi, rsp\n"
        "jmp %c0" ::"i"(__zinnia_entry)
    );
#else
#error "Unsupported architecture!"
#endif
}
