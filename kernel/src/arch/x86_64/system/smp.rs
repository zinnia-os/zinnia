use crate::{
    arch::{
        virt::{PageTableEntry, get_level_bits, get_max_leaf_level, get_page_bits},
        x86_64::{
            consts::{CR0_ET, CR0_PE, CR0_PG, CR4_PAE, MSR_EFER, MSR_EFER_LME, MSR_EFER_NXE},
            cpu::IS_INIT,
            system::{
                apic::{
                    DeliveryMode, DeliveryStatus, DestinationMode, IpiTarget, LAPIC, Level,
                    TriggerMode,
                },
                gdt::{BASE_GDT, Gdt},
            },
        },
    },
    boot::BootInfo,
    clock,
    memory::{
        PhysAddr,
        pmm::{AllocFlags, KernelAlloc, PageAllocator},
        virt::{KERNEL_STACK_SIZE, PteFlags, mmu::PageTable},
    },
    percpu::{self, CpuData},
    process::{Process, task::Task},
    sched,
    util::mutex::spin::SpinMutex,
};
use alloc::{sync::Arc, vec::Vec};
use bytemuck::{Pod, Zeroable};
use core::{arch::global_asm, mem::offset_of, sync::atomic::Ordering};
use uacpi_sys::{UACPI_STATUS_OK, acpi_entry_hdr, acpi_madt_lapic, acpi_madt_x2apic, uacpi_table};

unsafe extern "C" {
    pub unsafe static SMP_TRAMPOLINE_START: u8;
    pub unsafe static SMP_TRAMPOLINE_DATA: u8;
    pub unsafe static SMP_TRAMPOLINE_END: u8;
}

// The AP trampoline.
global_asm!("
.global SMP_TRAMPOLINE_START
.global SMP_TRAMPOLINE_ENTRY
.global SMP_TRAMPOLINE_END

.section .rodata
.code16

SMP_TRAMPOLINE_START:
    cli
    cld
    jmp 1f

SMP_TRAMPOLINE_DATA:
    .skip {info_size}

.set data_offset, (SMP_TRAMPOLINE_DATA - SMP_TRAMPOLINE_START)
.set gdtr_offset, (data_offset + {gdtr_offset})
.set farjmp_offset, (data_offset + {farjmp_offset})
.set temp_stack_offset, (data_offset + {temp_stack_offset})
.set temp_cr3_offset, (data_offset + {temp_cr3_offset})
.set entry_offset, (data_offset + {entry_offset})
.set hhdm_offset, (data_offset + {hhdm_offset})

1:
    mov bx, cs
    shl ebx, 4

.set idtr_offset, (invalid_idtr - SMP_TRAMPOLINE_START)
    lidtd cs:idtr_offset
    lgdtd cs:gdtr_offset

.set mode32_offset, (mode32 - SMP_TRAMPOLINE_START)
    lea eax, [ebx + mode32_offset]
    mov dword ptr cs:farjmp_offset, eax

    mov eax, {cr0_pe} | {cr0_et}
    mov cr0, eax
    jmp fword ptr cs:farjmp_offset

.code32
mode32:
    mov ax, {kernel32_ds}
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

    or  eax, {cr4_pae}
    mov cr4, eax

    mov ecx, {msr_efer}
    mov eax, {efer_lme}
    xor edx, edx
    wrmsr

    mov eax, dword ptr [ebx + temp_cr3_offset]
    mov cr3, eax

    mov eax, cr0
    or  eax, {cr0_pg}
    mov cr0, eax

.set mode64_offset, (mode64 - SMP_TRAMPOLINE_START)
    lea eax, [ebx + mode64_offset]
    push {kernel64_cs}
    push eax
    retf

.code64
mode64:
    mov ax, {kernel64_ds}
    mov ds, ax
    mov es, ax
    mov fs, ax
    mov gs, ax
    mov ss, ax

    mov ecx, {msr_efer}
    rdmsr
    or eax, {efer_nxe}
    wrmsr

    xor ebp, ebp

    lea rdi, [rbx + data_offset]
    mov rax, [rbx + entry_offset]
    add rsp, [rbx + hhdm_offset]
    jmp rax

invalid_idtr:
    .word 0
    .quad 0

SMP_TRAMPOLINE_END:",
    info_size = const INFO_SIZE,

    gdtr_offset = const GDTR_OFFSET,
    farjmp_offset = const FARJMP_OFFSET,
    temp_stack_offset = const TEMP_STACK_OFFSET,
    temp_cr3_offset = const TEMP_CR3_OFFSET,
    entry_offset = const ENTRY_OFFSET,
    hhdm_offset = const HHDM_OFFSET,

    kernel32_ds = const KERNEL32_DS,
    kernel64_cs = const KERNEL64_CS,
    kernel64_ds = const KERNEL64_DS,

    cr0_pe = const CR0_PE,
    cr0_et = const CR0_ET,
    cr0_pg = const CR0_PG,
    cr4_pae = const CR4_PAE,

    msr_efer = const MSR_EFER,
    efer_lme = const MSR_EFER_LME,
    efer_nxe = const MSR_EFER_NXE,
);

const INFO_SIZE: usize = size_of::<InfoData>();
const GDTR_OFFSET: usize = offset_of!(InfoData, gdtr_limit);
const FARJMP_OFFSET: usize = offset_of!(InfoData, farjmp_offset);
const TEMP_STACK_OFFSET: usize = offset_of!(InfoData, temp_stack);
const TEMP_CR3_OFFSET: usize = offset_of!(InfoData, temp_cr3);
const ENTRY_OFFSET: usize = offset_of!(InfoData, entry);
const HHDM_OFFSET: usize = offset_of!(InfoData, hhdm_offset);

const KERNEL32_DS: usize = offset_of!(Gdt, kernel32_data);
const KERNEL64_CS: usize = offset_of!(Gdt, kernel64_code);
const KERNEL64_DS: usize = offset_of!(Gdt, kernel64_data);

/// Information which is passed to a booted up AP via the trampoline.
#[repr(C, packed)]
#[derive(Debug, Pod, Zeroable, Clone, Copy)]
struct InfoData {
    gdt: Gdt,
    gdtr_limit: u16,
    gdtr_base: u64,
    farjmp_offset: u32,
    farjmp_segment: u32,
    temp_stack: u32,
    hhdm_offset: u64,
    temp_cr3: u32,
    entry: u64,
    lapic_id: u32,
    booted: u8,
}

extern "C" fn ap_entry(info: PhysAddr) -> ! {
    unsafe { PageTable::get_kernel().set_active() };

    let cpu_ctx = percpu::allocate_cpu().expect("Unable to allocate per-CPU context");
    super::super::cpu::setup_core(cpu_ctx);

    // Create a new idle task for this CPU.
    let idle_task =
        Arc::new(Task::new(sched::idle_fn, 0, 0, Process::get_kernel(), false).unwrap());
    cpu_ctx
        .scheduler
        .idle_task
        .store(Arc::into_raw(idle_task) as *mut _, Ordering::Release);

    // Create a dummy task to drop right after the first reschedule.
    let dummy = Arc::new(Task::new(sched::dummy_fn, 0, 0, Process::get_kernel(), false).unwrap());
    cpu_ctx
        .scheduler
        .current
        .store(Arc::into_raw(dummy) as *mut _, Ordering::Release);

    assert!(
        cpu_ctx.present.load(Ordering::Acquire),
        "CPU is not present?"
    );
    assert!(cpu_ctx.online.load(Ordering::Acquire), "CPU is not online?");
    log!("CPU {}: online", CpuData::get().id);

    unsafe {
        // Let the BSP know that we're alive.
        let booted = (info.as_hhdm() as *mut u8).byte_add(offset_of!(InfoData, booted));
        booted.write_volatile(1);
    }

    cpu_ctx.scheduler.do_yield();
    loop {}
}

fn start_ap(temp_cr3: u32, id: u32) {
    log!("CPU {id}: starting");
    let start = &raw const SMP_TRAMPOLINE_START; // Start of the trampoline.
    let data = &raw const SMP_TRAMPOLINE_DATA; // Start of the data passed to the trampoline.
    let end = &raw const SMP_TRAMPOLINE_END; // End of the trampoline.

    let tp_code =
        unsafe { core::slice::from_raw_parts(start, end.byte_offset_from_unsigned(start)) };

    assert!(tp_code.len() <= 0x1000);

    let Ok(mem) = KernelAlloc::alloc_bytes(tp_code.len(), AllocFlags::Kernel20) else {
        error!("Failed to allocate 20-bit memory for the AP trampoline!");
        return;
    };

    let Ok(stack_mem) = KernelAlloc::alloc_bytes(KERNEL_STACK_SIZE, AllocFlags::Kernel32) else {
        error!("Failed to allocate a stack for the AP trampoline!");
        return;
    };

    let temp_stack = stack_mem.value() + KERNEL_STACK_SIZE;

    // Prepare the AP trampoline.
    let buffer: &mut [u8] =
        unsafe { core::slice::from_raw_parts_mut(mem.as_hhdm(), tp_code.len()) };

    buffer.copy_from_slice(tp_code);

    let data_offset = unsafe { data.offset_from_unsigned(start) };

    // Save our metadata to the trampoline.
    let info = InfoData {
        gdt: BASE_GDT,
        gdtr_limit: (size_of::<Gdt>() - 1) as u16,
        gdtr_base: (mem.value() + data_offset + offset_of!(InfoData, gdt)) as _,
        farjmp_offset: 0,
        farjmp_segment: offset_of!(Gdt, kernel32_code) as _,
        temp_stack: temp_stack as u32,
        hhdm_offset: PhysAddr::null().as_hhdm() as *mut u8 as _,
        temp_cr3,
        entry: ap_entry as *const fn() as _,
        lapic_id: id,
        booted: 0,
    };

    buffer[data_offset..data_offset + size_of::<InfoData>()]
        .copy_from_slice(bytemuck::bytes_of(&info));

    let lapic = LAPIC.get();

    lapic.send_ipi(
        IpiTarget::Specific(id),
        0,
        DeliveryMode::INIT,
        DestinationMode::Physical,
        DeliveryStatus::Idle,
        Level::Assert,
        TriggerMode::Edge,
    );
    clock::block_ns(10_000_000).unwrap();

    lapic.send_ipi(
        IpiTarget::Specific(id),
        (mem.value() >> 12) as u8,
        DeliveryMode::StartUp,
        DestinationMode::Physical,
        DeliveryStatus::Idle,
        Level::Assert,
        TriggerMode::Edge,
    );
    clock::block_ns(10_000_000).unwrap();

    let booted = unsafe {
        mem.as_hhdm::<u8>()
            .byte_add(data_offset)
            .byte_add(offset_of!(InfoData, booted))
    };

    while unsafe { booted.read_volatile() } == 0 {
        clock::block_ns(1_000_000).unwrap();
    }
    unsafe { KernelAlloc::dealloc(mem, 1) };
}

static FOUND_APS: SpinMutex<Vec<u32>> = SpinMutex::new(Vec::new());

#[initgraph::task(
    name = "arch.x86_64.discover-aps",
    depends = [
        crate::memory::MEMORY_STAGE,
        crate::system::acpi::TABLES_STAGE,
        crate::clock::CLOCK_STAGE,
    ],
    entails = [crate::arch::INIT_STAGE],
)]
fn DISCOVER_APS_STAGE() {
    // Setup BSP.
    super::super::cpu::setup_core(CpuData::get());

    // Parse the MADT to discover LAPICs.
    unsafe {
        let mut table = uacpi_table::default();
        let status = uacpi_sys::uacpi_table_find_by_signature(c"APIC".as_ptr(), &raw mut table);
        if status != UACPI_STATUS_OK {
            return;
        }

        let madt_ptr = table.__bindgen_anon_1.ptr as *const uacpi_sys::acpi_madt;
        let madt = madt_ptr.read_unaligned();

        let mut offset = 0;
        while offset < madt.hdr.length - size_of::<uacpi_sys::acpi_sdt_hdr>() as u32 {
            let ptr = madt_ptr.offset(1).byte_offset(offset as isize) as *const acpi_entry_hdr;
            let entry = ptr.read_unaligned();

            match entry.type_ as u32 {
                uacpi_sys::ACPI_MADT_ENTRY_TYPE_LAPIC => {
                    let lapic = (ptr as *const acpi_madt_lapic).read_unaligned();

                    // If this LAPIC is enabled and not the BSP, start it.
                    if lapic.flags & 1 != 0 && lapic.id as u32 != LAPIC.get().id() {
                        FOUND_APS.lock().push(lapic.id as u32)
                    }
                }
                uacpi_sys::ACPI_MADT_ENTRY_TYPE_LOCAL_X2APIC => {
                    let lapic = (ptr as *const acpi_madt_x2apic).read_unaligned();

                    // If this LAPIC is enabled and not the BSP, start it.
                    if lapic.flags & 1 != 0 && lapic.id != LAPIC.get().id() {
                        FOUND_APS.lock().push(lapic.id)
                    }
                }
                _ => {}
            }
            offset += entry.length as u32;
        }

        uacpi_sys::uacpi_table_unref(&mut table);
    };
}

#[initgraph::task(
    name = "arch.x86_64.init-aps",
    depends = [DISCOVER_APS_STAGE, crate::clock::CLOCK_STAGE, crate::sched::SCHEDULER_STAGE],
    entails = [crate::arch::INIT_STAGE],
)]
fn INIT_APS_STAGE() {
    // Prepare an identity mapped page table, with the root highter half tables mapped as well.
    let temp_table = KernelAlloc::alloc(1, AllocFlags::Kernel32 | AllocFlags::Zeroed)
        .expect("Unable to allocate a page table in 32-bit physical memory");
    let temp_l3 = KernelAlloc::alloc(1, AllocFlags::Kernel32 | AllocFlags::Zeroed)
        .expect("Unable to allocate a page level in 32-bit physical memory");

    unsafe {
        let temp_buffer = temp_table.as_hhdm() as *mut u64;
        let temp_l3_buffer = temp_l3.as_hhdm() as *mut u64;

        // Identity map the lower half.
        for i in 0..4 {
            temp_l3_buffer.add(i).write(
                PageTableEntry::new(
                    PhysAddr::new(
                        i * (1 << (get_page_bits() + get_max_leaf_level() * get_level_bits())),
                    ),
                    PteFlags::Read | PteFlags::Write | PteFlags::Exec | PteFlags::Large,
                    3,
                )
                .inner() as u64,
            );
        }

        temp_buffer.write(
            PageTableEntry::new(
                temp_l3,
                PteFlags::Read | PteFlags::Write | PteFlags::Exec,
                3,
            )
            .inner() as u64,
        );

        // Copy over the higher half maps from the root table.
        let kernel_page = PageTable::get_kernel().get_head_addr().as_hhdm() as *const u64;
        for i in 256..512 {
            temp_buffer.offset(i).write(kernel_page.offset(i).read());
        }
    }

    assert!(
        temp_table.value() < u32::MAX as usize,
        "Temporary page table *must* fit inside a 32-bit value"
    );

    match BootInfo::get().command_line.get_usize("smp") {
        Some(x) => {
            for ap in FOUND_APS.lock().iter().take(x.saturating_sub(1)) {
                start_ap(temp_table.value() as u32, *ap);
            }
        }
        None => {
            for ap in FOUND_APS.lock().iter() {
                start_ap(temp_table.value() as u32, *ap);
            }
        }
    }

    unsafe {
        KernelAlloc::dealloc(temp_table, 1);
        KernelAlloc::dealloc(temp_l3, 1);
    }

    IS_INIT.store(true, Ordering::Relaxed)
}
