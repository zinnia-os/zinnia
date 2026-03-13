#include <x86_64/smp.h>
#include <x86_64/defs.h>

.intel_syntax noprefix

.global smp_trampoline_start
.global smp_trampoline_end

.section .rodata
.code16

smp_trampoline_start:
    cli
    cld
    jmp 1f

smp_trampoline_data:
    .skip SMP_INFO_SIZE

.set data_offset, (smp_trampoline_data - smp_trampoline_start)
.set gdtr_offset, (data_offset + SMP_GDTR_OFFSET)
.set farjmp_offset, (data_offset + SMP_FARJMP_OFFSET)
.set temp_stack_offset, (data_offset + SMP_TEMP_STACK_OFFSET)
.set temp_cr3_offset, (data_offset + SMP_TEMP_CR3_OFFSET)
.set entry_offset, (data_offset + SMP_ENTRY_OFFSET)
.set hhdm_offset, (data_offset + SMP_HHDM_OFFSET)

1:
    mov bx, cs
    shl ebx, 4

.set idtr_offset, (invalid_idtr - smp_trampoline_start)
    lidtd cs:idtr_offset
    lgdtd cs:gdtr_offset

.set mode32_offset, (mode32 - smp_trampoline_start)
    lea eax, [ebx + mode32_offset]
    mov dword ptr cs:farjmp_offset, eax

    mov eax, CR0_PE | CR0_ET
    mov cr0, eax
    jmp fword ptr cs:farjmp_offset

.code32
mode32:
    mov ax, SMP_KERNEL32_DS
    mov ds, ax
    mov es, ax
    mov fs, ax
    mov gs, ax
    mov ss, ax

    xor  eax, eax
    lldt ax

    mov esp, dword ptr [ebx + temp_stack_offset]

    xor eax, eax
    mov cr4, eax

    or  eax, CR4_PAE
    mov cr4, eax

    mov ecx, MSR_EFER
    mov eax, MSR_EFER_LME
    xor edx, edx
    wrmsr

    mov eax, dword ptr [ebx + temp_cr3_offset]
    mov cr3, eax

    mov eax, cr0
    or  eax, CR0_PG
    mov cr0, eax

.set mode64_offset, (mode64 - smp_trampoline_start)
    lea eax, [ebx + mode64_offset]
    push SMP_KERNEL64_CS
    push eax
    retf

.code64
mode64:
    mov ax, SMP_KERNEL64_DS
    mov ds, ax
    mov es, ax
    mov fs, ax
    mov gs, ax
    mov ss, ax

    mov ecx, MSR_EFER
    rdmsr
    or eax, MSR_EFER_NXE
    wrmsr

    xor ebp, ebp

    lea rdi, [rbx + data_offset]
    mov rax, [rbx + entry_offset]
    add rsp, [rbx + hhdm_offset]
    jmp rax

invalid_idtr:
    .word 0
    .quad 0

smp_trampoline_end:
