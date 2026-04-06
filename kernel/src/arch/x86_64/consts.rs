#![allow(unused)]

/// Carry floating
pub const RFLAGS_CF: u64 = 1 << 0;
/// Parity Flag
pub const RFLAGS_PF: u64 = 1 << 2;
/// Auxiliary Carry Flag
pub const RFLAGS_AF: u64 = 1 << 4;
/// Zero Flag
pub const RFLAGS_ZF: u64 = 1 << 6;
/// Sign Flag
pub const RFLAGS_SF: u64 = 1 << 7;
/// Trap Flag
pub const RFLAGS_TF: u64 = 1 << 8;
/// Interrupt Enable Flag
pub const RFLAGS_IF: u64 = 1 << 9;
/// Direction Flag
pub const RFLAGS_DF: u64 = 1 << 10;
/// Overflow Flag
pub const RFLAGS_OF: u64 = 1 << 11;
/// I/O Privilege Level
pub const RFLAGS_IOPL: u64 = 3 << 12;
/// Nested Task
pub const RFLAGS_NT: u64 = 1 << 14;
/// Resume Flag
pub const RFLAGS_RF: u64 = 1 << 16;
/// Virtual-8086 Mode
pub const RFLAGS_VM: u64 = 1 << 17;
/// Alignment Check / Access Control
pub const RFLAGS_AC: u64 = 1 << 18;
/// Virtual Interrupt Flag
pub const RFLAGS_VIF: u64 = 1 << 19;
/// Virtual Interrupt Pending
pub const RFLAGS_VIP: u64 = 1 << 20;
/// ID Flag
pub const RFLAGS_ID: u64 = 1 << 21;

// Control register 0
/// Protected Mode Enable
pub const CR0_PE: usize = 1 << 0;
/// Monitor Co-Processor
pub const CR0_MP: usize = 1 << 1;
/// Emulation
pub const CR0_EM: usize = 1 << 2;
/// Task Switched
pub const CR0_TS: usize = 1 << 3;
/// Extension Type
pub const CR0_ET: usize = 1 << 4;
/// Numeric Error
pub const CR0_NE: usize = 1 << 5;
/// Write Protect
pub const CR0_WP: usize = 1 << 16;
/// Alignment Mask
pub const CR0_AM: usize = 1 << 18;
/// Not-Write Through
pub const CR0_NW: usize = 1 << 29;
/// Cache Disable
pub const CR0_CD: usize = 1 << 30;
/// Paging
pub const CR0_PG: usize = 1 << 31;

// Control register 4
/// Virtual-8086 Mode Extensions
pub const CR4_VME: usize = 1 << 0;
/// Protected Mode Virtual Interrupts
pub const CR4_PVI: usize = 1 << 1;
/// Time Stamp enabled only in ring 0
pub const CR4_TSD: usize = 1 << 2;
/// Debugging Extensions
pub const CR4_DE: usize = 1 << 3;
/// Page Size Extension
pub const CR4_PSE: usize = 1 << 4;
/// Physical Address Extension
pub const CR4_PAE: usize = 1 << 5;
/// Machine Check Exception
pub const CR4_MCE: usize = 1 << 6;
/// Page Global Enable
pub const CR4_PGE: usize = 1 << 7;
/// Performance Monitoring Counter Enable
pub const CR4_PCE: usize = 1 << 8;
/// OS support for fxsave and fxrstor instructions
pub const CR4_OSFXSR: usize = 1 << 9;
/// OS Support for unmasked simd floating point exceptions
pub const CR4_OSXMMEXCPT: usize = 1 << 10;
/// User-Mode Instruction Prevention
pub const CR4_UMIP: usize = 1 << 11;
/// Virtual Machine Extensions Enable
pub const CR4_VMXE: usize = 1 << 13;
/// Safer Mode Extensions Enable
pub const CR4_SMXE: usize = 1 << 14;
/// Enables the instructions RDFSBASE, RDGSBASE, WRFSBASE, and WRGSBASE
pub const CR4_FSGSBASE: usize = 1 << 16;
/// PCID Enable
pub const CR4_PCIDE: usize = 1 << 17;
/// XSAVE And Processor Extended States Enable
pub const CR4_OSXSAVE: usize = 1 << 18;
/// Supervisor Mode Executions Protection Enable
pub const CR4_SMEP: usize = 1 << 20;
/// Supervisor Mode Access Protection Enable
pub const CR4_SMAP: usize = 1 << 21;
/// Enable protection keys for user-mode pages
pub const CR4_PKE: usize = 1 << 22;
/// Enable Control-flow Enforcement Technology
pub const CR4_CET: usize = 1 << 23;
/// Enable protection keys for supervisor-mode pages
pub const CR4_PKS: usize = 1 << 24;

// Model specific registers
pub const MSR_EFER: u32 = 0xC0000080;
/// System Call Extensions
pub const MSR_EFER_SCE: u32 = 1 << 0;
/// Long Mode Enable
pub const MSR_EFER_LME: u32 = 1 << 8;
/// Long Mode Active
pub const MSR_EFER_LMA: u32 = 1 << 10;
/// No-Execute Enable
pub const MSR_EFER_NXE: u32 = 1 << 11;
/// Secure Virtual Machine Enable
pub const MSR_EFER_SVME: u32 = 1 << 12;
/// Long Mode Segment Limit Enable
pub const MSR_EFER_LMSLE: u32 = 1 << 13;
/// Fast FXSAVE/FXRSTOR
pub const MSR_EFER_FFXSR: u32 = 1 << 14;
/// Translation Cache Extension
pub const MSR_EFER_TCE: u32 = 1 << 15;
pub const MSR_STAR: u32 = 0xC0000081;
pub const MSR_LSTAR: u32 = 0xC0000082;
pub const MSR_CSTAR: u32 = 0xC0000083;
pub const MSR_SFMASK: u32 = 0xC0000084;
pub const MSR_FS_BASE: u32 = 0xC0000100;
pub const MSR_GS_BASE: u32 = 0xC0000101;
pub const MSR_KERNEL_GS_BASE: u32 = 0xC0000102;

pub const CPL_USER: u8 = 0b11;
pub const CPL_KERNEL: u8 = 0b00;

// CPUID Leaf 1 ECX
pub const CPUID_1C_SSE3: u32 = 1 << 0;
pub const CPUID_1C_PCLMUL: u32 = 1 << 1;
pub const CPUID_1C_DTES64: u32 = 1 << 2;
pub const CPUID_1C_MONITOR: u32 = 1 << 3;
pub const CPUID_1C_DS_CPL: u32 = 1 << 4;
pub const CPUID_1C_VMX: u32 = 1 << 5;
pub const CPUID_1C_SMX: u32 = 1 << 6;
pub const CPUID_1C_EST: u32 = 1 << 7;
pub const CPUID_1C_TM2: u32 = 1 << 8;
pub const CPUID_1C_SSSE3: u32 = 1 << 9;
pub const CPUID_1C_CID: u32 = 1 << 10;
pub const CPUID_1C_SDBG: u32 = 1 << 11;
pub const CPUID_1C_FMA: u32 = 1 << 12;
pub const CPUID_1C_CX16: u32 = 1 << 13;
pub const CPUID_1C_XTPR: u32 = 1 << 14;
pub const CPUID_1C_PDCM: u32 = 1 << 15;
pub const CPUID_1C_PCID: u32 = 1 << 17;
pub const CPUID_1C_DCA: u32 = 1 << 18;
pub const CPUID_1C_SSE4_1: u32 = 1 << 19;
pub const CPUID_1C_SSE4_2: u32 = 1 << 20;
pub const CPUID_1C_X2APIC: u32 = 1 << 21;
pub const CPUID_1C_MOVBE: u32 = 1 << 22;
pub const CPUID_1C_POPCNT: u32 = 1 << 23;
pub const CPUID_1C_TSC: u32 = 1 << 24;
pub const CPUID_1C_AES: u32 = 1 << 25;
pub const CPUID_1C_XSAVE: u32 = 1 << 26;
pub const CPUID_1C_OSXSAVE: u32 = 1 << 27;
pub const CPUID_1C_AVX: u32 = 1 << 28;
pub const CPUID_1C_F16C: u32 = 1 << 29;
pub const CPUID_1C_RDRAND: u32 = 1 << 30;
pub const CPUID_1C_HYPERVISOR: u32 = 1 << 31;

// CPUID Leaf 1 EDX
pub const CPUID_1D_FPU: u32 = 1 << 0;
pub const CPUID_1D_VME: u32 = 1 << 1;
pub const CPUID_1D_DE: u32 = 1 << 2;
pub const CPUID_1D_PSE: u32 = 1 << 3;
pub const CPUID_1D_TSC: u32 = 1 << 4;
pub const CPUID_1D_MSR: u32 = 1 << 5;
pub const CPUID_1D_PAE: u32 = 1 << 6;
pub const CPUID_1D_MCE: u32 = 1 << 7;
pub const CPUID_1D_CX8: u32 = 1 << 8;
pub const CPUID_1D_APIC: u32 = 1 << 9;
pub const CPUID_1D_SEP: u32 = 1 << 11;
pub const CPUID_1D_MTRR: u32 = 1 << 12;
pub const CPUID_1D_PGE: u32 = 1 << 13;
pub const CPUID_1D_MCA: u32 = 1 << 14;
pub const CPUID_1D_CMOV: u32 = 1 << 15;
pub const CPUID_1D_PAT: u32 = 1 << 16;
pub const CPUID_1D_PSE36: u32 = 1 << 17;
pub const CPUID_1D_PSN: u32 = 1 << 18;
pub const CPUID_1D_CLFLUSH: u32 = 1 << 19;
pub const CPUID_1D_DS: u32 = 1 << 21;
pub const CPUID_1D_ACPI: u32 = 1 << 22;
pub const CPUID_1D_MMX: u32 = 1 << 23;
pub const CPUID_1D_FXSR: u32 = 1 << 24;
pub const CPUID_1D_SSE: u32 = 1 << 25;
pub const CPUID_1D_SSE2: u32 = 1 << 26;
pub const CPUID_1D_SS: u32 = 1 << 27;
pub const CPUID_1D_HTT: u32 = 1 << 28;
pub const CPUID_1D_TM: u32 = 1 << 29;
pub const CPUID_1D_IA64: u32 = 1 << 30;
pub const CPUID_1D_PBE: u32 = 1 << 31;

// CPUID Leaf 7 EBX
pub const CPUID_7B_FSGSBASE: u32 = 1 << 0;
pub const CPUID_7B_SGX: u32 = 1 << 2;
pub const CPUID_7B_BMI: u32 = 1 << 3;
pub const CPUID_7B_HLE: u32 = 1 << 4;
pub const CPUID_7B_AVX2: u32 = 1 << 5;
pub const CPUID_7B_SMEP: u32 = 1 << 7;
pub const CPUID_7B_BMI2: u32 = 1 << 8;
pub const CPUID_7B_RTM: u32 = 1 << 11;
pub const CPUID_7B_AVX512F: u32 = 1 << 16;
pub const CPUID_7B_AVX512DQ: u32 = 1 << 17;
pub const CPUID_7B_RDSEED: u32 = 1 << 18;
pub const CPUID_7B_ADX: u32 = 1 << 19;
pub const CPUID_7B_SMAP: u32 = 1 << 20;
pub const CPUID_7B_AVX512IFMA: u32 = 1 << 21;
pub const CPUID_7B_CLFLUSHOPT: u32 = 1 << 23;
pub const CPUID_7B_CLWB: u32 = 1 << 24;
pub const CPUID_7B_AVX512CD: u32 = 1 << 28;
pub const CPUID_7B_SHA: u32 = 1 << 29;
pub const CPUID_7B_AVX512BW: u32 = 1 << 30;
pub const CPUID_7B_AVX512VL: u32 = 1 << 31;

// CPUID Leaf 7 ECX
pub const CPUID_7C_AVX512VBMI: u32 = 1 << 1;
pub const CPUID_7C_UMIP: u32 = 1 << 2;
pub const CPUID_7C_PKU: u32 = 1 << 3;
pub const CPUID_7C_OSPKE: u32 = 1 << 4;
pub const CPUID_7C_WAITPKG: u32 = 1 << 5;
pub const CPUID_7C_AVX512VBMI2: u32 = 1 << 6;
pub const CPUID_7C_SHSTK: u32 = 1 << 7;
pub const CPUID_7C_GFNI: u32 = 1 << 8;
pub const CPUID_7C_VAES: u32 = 1 << 9;
pub const CPUID_7C_VPCLMULQDQ: u32 = 1 << 10;
pub const CPUID_7C_AVX512VNNI: u32 = 1 << 11;
pub const CPUID_7C_AVX512BITALG: u32 = 1 << 12;
pub const CPUID_7C_TME_EN: u32 = 1 << 13;
pub const CPUID_7C_AVX512VPOPCNTDQ: u32 = 1 << 14;
pub const CPUID_7C_RDPID: u32 = 1 << 22;
pub const CPUID_7C_KL: u32 = 1 << 23;
pub const CPUID_7C_CLDEMOTE: u32 = 1 << 25;
pub const CPUID_7C_MOVDIRI: u32 = 1 << 27;
pub const CPUID_7C_MOVDIR64B: u32 = 1 << 28;
pub const CPUID_7C_ENQCMD: u32 = 1 << 29;

// CPUID Leaf 7 EDX
pub const CPUID_7D_UINTR: u32 = 1 << 5;
pub const CPUID_7D_AVX512VP2INTERSECT: u32 = 1 << 8;
pub const CPUID_7D_SERIALIZE: u32 = 1 << 14;
pub const CPUID_7D_TSXLDTRK: u32 = 1 << 16;
pub const CPUID_7D_PCONFIG: u32 = 1 << 18;
pub const CPUID_7D_IBT: u32 = 1 << 20;
pub const CPUID_7D_AMX_BF16: u32 = 1 << 22;
pub const CPUID_7D_AVX512FP16: u32 = 1 << 23;
pub const CPUID_7D_AMX_TILE: u32 = 1 << 24;
pub const CPUID_7D_AMX_INT8: u32 = 1 << 25;

pub const IDT_DE: u8 = 0x0;
pub const IDT_DB: u8 = 0x1;
pub const IDT_NMI: u8 = 0x2;
pub const IDT_BP: u8 = 0x3;
pub const IDT_OF: u8 = 0x4;
pub const IDT_BR: u8 = 0x5;
pub const IDT_UD: u8 = 0x6;
pub const IDT_NM: u8 = 0x7;
pub const IDT_DF: u8 = 0x8;
pub const IDT_CS: u8 = 0x9;
pub const IDT_TS: u8 = 0xA;
pub const IDT_NP: u8 = 0xB;
pub const IDT_SS: u8 = 0xC;
pub const IDT_GP: u8 = 0xD;
pub const IDT_PF: u8 = 0xE;
pub const IDT_MF: u8 = 0x10;
pub const IDT_AC: u8 = 0x11;
pub const IDT_MC: u8 = 0x12;
pub const IDT_XF: u8 = 0x13;
pub const IDT_VE: u8 = 0x14;
pub const IDT_CP: u8 = 0x15;
pub const IDT_HV: u8 = 0x1C;
pub const IDT_VC: u8 = 0x1D;
pub const IDT_SX: u8 = 0x1E;
pub const IDT_IPI_PANIC: u8 = 0xFF;
pub const IDT_IPI_RESCHED: u8 = 0xFE;
pub const IDT_IPI_SHOOTDOWN: u8 = 0xFD;
