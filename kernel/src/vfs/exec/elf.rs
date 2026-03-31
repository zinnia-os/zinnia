use super::ExecInfo;
use crate::{
    arch,
    memory::{MemoryObject, PagedMemoryObject, virt::VmFlags},
    posix::errno::{EResult, Errno},
    process::{Process, task::Task, to_user},
    util::align_down,
    vfs::{
        exec::ExecFormat,
        file::{File, MmapFlags, OpenFlags},
        inode::Mode,
    },
};
use alloc::{sync::Arc, vec::Vec};
use bytemuck::{Pod, Zeroable};
use core::num::NonZeroUsize;

// ELF Header Identification
pub const ELF_MAG: [u8; 4] = [0x7F, b'E', b'L', b'F'];
pub const EI_CLASS: usize = 4; // File class
pub const EI_DATA: usize = 5; // Data encoding
pub const EI_VERSION: usize = 6; // File version
pub const EI_OSABI: usize = 7; // OS/ABI identification
pub const EI_ABIVERSION: usize = 8; // ABI version
pub const EI_PAD: usize = 9; // Start of padding bytes
pub const EI_NIDENT: usize = 16; // Size of e_ident[]

// ELF Identification Type
pub const ELFCLASS32: u8 = 1;
pub const ELFCLASS64: u8 = 2;
pub const ELFCLASSNUM: u8 = 3;
pub const ELFDATA2LSB: u8 = 1;
pub const ELFDATA2MSB: u8 = 2;
pub const ELFDATANUM: u8 = 3;

#[cfg(target_arch = "x86_64")]
pub const EM_CURRENT: u16 = 0x3E;
#[cfg(target_arch = "aarch64")]
pub const EM_CURRENT: u16 = 0xB7;
#[cfg(target_arch = "riscv64")]
pub const EM_CURRENT: u16 = 0xF3;
#[cfg(target_arch = "loongarch64")]
pub const EM_CURRENT: u16 = 0x102;

pub const EV_NONE: u8 = 0;
pub const EV_CURRENT: u8 = 1;
pub const EV_NUM: u8 = 2;

pub const ELFOSABI_SYSV: u8 = 0; // System V ABI
pub const ELFOSABI_HPUX: u8 = 1; // HP-UX operating system
pub const ELFOSABI_STANDALONE: u8 = 255; // Standalone (embedded) application

// ELF Header Type
pub const ET_NONE: u16 = 0;
pub const ET_REL: u16 = 1;
pub const ET_EXEC: u16 = 2;
pub const ET_DYN: u16 = 3;
pub const ET_CORE: u16 = 4;

// Program Header Types
pub const PT_NULL: u32 = 0x00000000;
pub const PT_LOAD: u32 = 0x00000001;
pub const PT_DYNAMIC: u32 = 0x00000002;
pub const PT_INTERP: u32 = 0x00000003;
pub const PT_NOTE: u32 = 0x00000004;
pub const PT_SHLIB: u32 = 0x00000005;
pub const PT_PHDR: u32 = 0x00000006;
pub const PT_TLS: u32 = 0x00000007;
pub const PT_MODVERSION: u32 = 0x60000001;
pub const PT_MODAUTHOR: u32 = 0x60000002;
pub const PT_MODDESC: u32 = 0x60000003;

// Program Header Flags
pub const PF_EXECUTE: u32 = 0x01;
pub const PF_WRITE: u32 = 0x02;
pub const PF_READ: u32 = 0x04;

// Dynamic table
pub const DT_NULL: u32 = 0;
pub const DT_NEEDED: u32 = 1;
pub const DT_PLTRELSZ: u32 = 2;
pub const DT_PLTGOT: u32 = 3;
pub const DT_HASH: u32 = 4;
pub const DT_STRTAB: u32 = 5;
pub const DT_SYMTAB: u32 = 6;
pub const DT_RELA: u32 = 7;
pub const DT_RELASZ: u32 = 8;
pub const DT_RELAENT: u32 = 9;
pub const DT_STRSZ: u32 = 10;
pub const DT_SYMENT: u32 = 11;
pub const DT_INIT: u32 = 12;
pub const DT_FINI: u32 = 13;
pub const DT_SONAME: u32 = 14;
pub const DT_RPATH: u32 = 15;
pub const DT_SYMBOLIC: u32 = 16;
pub const DT_REL: u32 = 17;
pub const DT_RELSZ: u32 = 18;
pub const DT_RELENT: u32 = 19;
pub const DT_PLTREL: u32 = 20;
pub const DT_DEBUG: u32 = 21;
pub const DT_TEXTREL: u32 = 22;
pub const DT_JMPREL: u32 = 23;
pub const DT_BIND_NOW: u32 = 24;
pub const DT_INIT_ARRAY: u32 = 25;
pub const DT_FINI_ARRAY: u32 = 26;
pub const DT_INIT_ARRAYSZ: u32 = 27;
pub const DT_FINI_ARRAYSZ: u32 = 28;
pub const DT_LOOS: u32 = 0x60000000;
pub const DT_HIOS: u32 = 0x6FFFFFFF;
pub const DT_LOPROC: u32 = 0x70000000;
pub const DT_HIPROC: u32 = 0x7FFFFFFF;

// Symbol bindings
pub const STB_LOCAL: u32 = 0; // Not visible outside the object file
pub const STB_GLOBAL: u32 = 1; // Global symbol, visible to all object files
pub const STB_WEAK: u32 = 2; // Global scope, but with lower precedence than global symbols
pub const STB_LOOS: u32 = 10; // Environment-specific use
pub const STB_HIOS: u32 = 12; //
pub const STB_LOPROC: u32 = 13; // Processor-specific use
pub const STB_HIPROC: u32 = 15; //

// Symbol types
pub const STT_NOTYPE: u32 = 0; // No type specified (e.g., an absolute symbol)
pub const STT_OBJECT: u32 = 1; // Data object
pub const STT_FUNC: u32 = 2; // Function entry point
pub const STT_SECTION: u32 = 3; // Symbol is associated with a section
pub const STT_FILE: u32 = 4; // Source file associated with the object file
pub const STT_LOOS: u32 = 10; // Environment-specific use
pub const STT_HIOS: u32 = 12; //
pub const STT_LOPROC: u32 = 13; // Processor-specific use
pub const STT_HIPROC: u32 = 15; //

// Auxvals
pub const AT_NULL: u32 = 0;
pub const AT_IGNORE: u32 = 1;
pub const AT_EXECFD: u32 = 2;
pub const AT_PHDR: u32 = 3;
pub const AT_PHENT: u32 = 4;
pub const AT_PHNUM: u32 = 5;
pub const AT_PAGESZ: u32 = 6;
pub const AT_BASE: u32 = 7;
pub const AT_FLAGS: u32 = 8;
pub const AT_ENTRY: u32 = 9;
pub const AT_NOTELF: u32 = 10;
pub const AT_UID: u32 = 11;
pub const AT_EUID: u32 = 12;
pub const AT_GID: u32 = 13;
pub const AT_EGID: u32 = 14;
pub const AT_SECURE: u32 = 23;
pub const AT_L4_AUX: u32 = 0xf0;
pub const AT_L4_ENV: u32 = 0xf1;

pub const R_X86_64_NONE: u32 = 0;
pub const R_X86_64_64: u32 = 1;
pub const R_X86_64_COPY: u32 = 5;
pub const R_X86_64_GLOB_DAT: u32 = 6;
pub const R_X86_64_JUMP_SLOT: u32 = 7;
pub const R_X86_64_RELATIVE: u32 = 8;

pub const R_RISCV_NONE: u32 = 0;
pub const R_RISCV_64: u32 = 2;
pub const R_RISCV_RELATIVE: u32 = 3;
pub const R_RISCV_COPY: u32 = 4;
pub const R_RISCV_JUMP_SLOT: u32 = 5;

#[cfg(target_arch = "x86_64")]
pub const R_COMMON_NONE: u32 = R_X86_64_NONE;
#[cfg(target_arch = "x86_64")]
pub const R_COMMON_64: u32 = R_X86_64_64;
#[cfg(target_arch = "x86_64")]
pub const R_COMMON_GLOB_DAT: u32 = R_X86_64_GLOB_DAT;
#[cfg(target_arch = "x86_64")]
pub const R_COMMON_JUMP_SLOT: u32 = R_X86_64_JUMP_SLOT;
#[cfg(target_arch = "x86_64")]
pub const R_COMMON_RELATIVE: u32 = R_X86_64_RELATIVE;

#[cfg(target_arch = "riscv64")]
pub const R_COMMON_NONE: u32 = R_RISCV_NONE;
#[cfg(target_arch = "riscv64")]
pub const R_COMMON_64: u32 = R_RISCV_64;
#[cfg(target_arch = "riscv64")]
pub const R_COMMON_GLOB_DAT: u32 = R_RISCV_64;
#[cfg(target_arch = "riscv64")]
pub const R_COMMON_JUMP_SLOT: u32 = R_RISCV_JUMP_SLOT;
#[cfg(target_arch = "riscv64")]
pub const R_COMMON_RELATIVE: u32 = R_RISCV_RELATIVE;

#[cfg(target_pointer_width = "64")]
pub type ElfAddr = u64;
#[cfg(target_pointer_width = "64")]
pub type ElfOff = u64;
#[cfg(target_pointer_width = "32")]
pub type ElfAddr = u32;
#[cfg(target_pointer_width = "32")]
pub type ElfOff = u32;

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
#[cfg(target_pointer_width = "64")]
pub struct ElfPhdr {
    pub p_type: u32,
    pub p_flags: u32,
    pub p_offset: ElfOff,
    pub p_vaddr: ElfAddr,
    pub p_paddr: ElfAddr,
    pub p_filesz: u64,
    pub p_memsz: u64,
    pub p_align: u64,
}
#[cfg(target_pointer_width = "64")]
static_assert!(size_of::<ElfPhdr>() == 56);

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
#[cfg(target_pointer_width = "32")]
pub struct ElfPhdr {
    pub p_type: u32,
    pub p_offset: ElfOff,
    pub p_vaddr: ElfAddr,
    pub p_paddr: ElfAddr,
    pub p_filesz: u32,
    pub p_memsz: u32,
    pub p_flags: u32,
    pub p_align: u32,
}
#[cfg(target_pointer_width = "32")]
static_assert!(size_of::<ElfPhdr>() == 32);

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
#[cfg(target_pointer_width = "64")]
pub struct ElfSym {
    pub st_name: u32,
    pub st_info: u8,
    pub st_other: u8,
    pub st_shndx: u16,
    pub st_value: ElfAddr,
    pub st_size: u64,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
#[cfg(target_pointer_width = "32")]
pub struct ElfSym {
    pub st_name: u32,
    pub st_value: ElfAddr,
    pub st_size: u32,
    pub st_info: u8,
    pub st_other: u8,
    pub st_shndx: u16,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
#[cfg(target_pointer_width = "64")]
pub struct ElfDyn {
    pub d_tag: i64,
    pub d_val: u64,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
#[cfg(target_pointer_width = "32")]
pub struct ElfDyn {
    pub d_tag: i32,
    pub d_val: u32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
#[cfg(target_pointer_width = "64")]
pub struct ElfRela {
    pub r_offset: ElfAddr,
    pub r_info: u64,
    pub r_addend: i64,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
#[cfg(target_pointer_width = "32")]
pub struct ElfRela {
    pub r_offset: ElfAddr,
    pub r_info: u32,
    pub r_addend: i32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct ElfHashTable {
    pub nbucket: u32,
    pub nchain: u32,
}

/// ELF header
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct ElfHdr {
    /// ELF identification
    pub e_ident: [u8; EI_NIDENT],
    /// Object file type
    pub e_type: u16,
    /// Machine type
    pub e_machine: u16,
    /// Object file version
    pub e_version: u32,
    /// Entry point address
    pub e_entry: ElfAddr,
    /// Program header offset
    pub e_phoff: ElfOff,
    /// Section header offset
    pub e_shoff: ElfOff,
    /// Processor-specific flags
    pub e_flags: u32,
    /// ELF header size
    pub e_ehsize: u16,
    /// Size of program header entry
    pub e_phentsize: u16,
    /// Number of program header entries
    pub e_phnum: u16,
    /// Size of section header entry
    pub e_shentsize: u16,
    /// Number of section header entries
    pub e_shnum: u16,
    /// Section name string table index
    pub e_shstrndx: u16,
}
static_assert!(size_of::<ElfHdr>() == 64);

// Yes I know ELF already has "Format" in the name.
#[derive(Debug)]
pub struct ElfFormat;

struct ElfInfo {
    at_phdr: usize,
    at_phnum: usize,
    at_phent: usize,
    at_entry: usize,
}

impl ElfFormat {
    // Loads an ELF file into an address space. Returns the entry point address.
    fn load_file(
        file: &Arc<File>,
        proc: &Arc<Process>,
        info: &mut ExecInfo,
        base: usize,
    ) -> EResult<ElfInfo> {
        // Read the header.
        let mut hdr_data = [0u8; size_of::<ElfHdr>()];
        file.pread_kernel(&mut hdr_data, 0)?;
        let elf_hdr = bytemuck::pod_read_unaligned::<ElfHdr>(&hdr_data);

        // TODO: Do the rest of IDENT checks.
        if elf_hdr.e_ident[EI_VERSION] != EV_CURRENT {
            return Err(Errno::ENOEXEC);
        }
        if elf_hdr.e_machine != EM_CURRENT {
            return Err(Errno::ENOEXEC);
        }

        // Start mapping a relocatable ELF at this address.
        // If it's fixed, don't account for an extra base address.
        let base = if elf_hdr.e_type == ET_DYN { base } else { 0 };

        let page_size = arch::virt::get_page_size();
        let mut phdr_addr = 0usize;

        // Iterate all PHDRs.
        for i in 0..elf_hdr.e_phnum {
            let mut phdr_data = vec![0u8; elf_hdr.e_phentsize as usize];
            file.pread_kernel(
                &mut phdr_data,
                elf_hdr.e_phoff + (elf_hdr.e_phentsize as u64 * i as u64),
            )?;
            let phdr = bytemuck::pod_read_unaligned::<ElfPhdr>(&phdr_data);

            match phdr.p_type {
                PT_LOAD => {
                    let mut prot = VmFlags::empty();
                    if phdr.p_flags & PF_READ != 0 {
                        prot |= VmFlags::Read;
                    }
                    if phdr.p_flags & PF_WRITE != 0 {
                        prot |= VmFlags::Write;
                    }
                    if phdr.p_flags & PF_EXECUTE != 0 {
                        prot |= VmFlags::Exec;
                    }

                    debug_assert!(phdr.p_offset % phdr.p_align == phdr.p_vaddr % phdr.p_align);
                    let misalign = phdr.p_vaddr as usize & (page_size - 1);
                    let map_address = base + phdr.p_vaddr as usize - misalign;
                    let backed_map_size =
                        (phdr.p_filesz as usize + misalign + page_size - 1) & !(page_size - 1);
                    let total_map_size =
                        (phdr.p_memsz as usize + misalign + page_size - 1) & !(page_size - 1);

                    // Copy the file data into its own mapping.
                    file.mmap(
                        &mut info.space,
                        (base + phdr.p_vaddr as usize).into(),
                        NonZeroUsize::new(phdr.p_filesz as usize).unwrap(),
                        prot,
                        MmapFlags::Private,
                        (phdr.p_offset) as _,
                    )?;
                    //info.space.map_object(
                    //    backed.clone(),
                    //    prot,
                    //    (phdr.p_offset as usize - misalign) as _,
                    //)?;

                    if total_map_size > backed_map_size {
                        let private_map = Arc::new(PagedMemoryObject::new_phys());
                        info.space.map_object(
                            private_map,
                            (map_address + backed_map_size).into(),
                            NonZeroUsize::new(total_map_size - backed_map_size).unwrap(),
                            prot,
                            0,
                        )?;
                    }
                }
                PT_PHDR => {
                    phdr_addr = phdr.p_vaddr as usize;
                }
                PT_INTERP => {
                    let mut interp_name = vec![0u8; phdr.p_filesz as usize - 1]; // Minus the trailing NUL.
                    file.pread_kernel(&mut interp_name, phdr.p_offset as _)?;
                    // Open the interpreter and save it in the info.
                    info.interpreter = Some(File::open(
                        proc.root_dir.lock().clone(),
                        proc.working_dir.lock().clone(),
                        &interp_name,
                        OpenFlags::Read | OpenFlags::Executable,
                        Mode::empty(),
                        &proc.identity.lock(),
                    )?)
                }
                _ => (),
            }
        }

        Ok(ElfInfo {
            at_phdr: phdr_addr + base,
            at_phnum: elf_hdr.e_phnum as usize,
            at_phent: elf_hdr.e_phentsize as usize,
            at_entry: elf_hdr.e_entry as usize + base,
        })
    }
}

impl ExecFormat for ElfFormat {
    fn identify(&self, file: &File) -> bool {
        let mut buffer = [0u8; size_of::<ElfHdr>()];
        match file.pread_kernel(&mut buffer, 0) {
            Ok(x) => {
                if x != buffer.len() as _ {
                    return false;
                }
            }
            Err(_) => return false,
        }
        let header = bytemuck::pod_read_unaligned::<ElfHdr>(&buffer);

        if header.e_ident[0..4] != ELF_MAG {
            return false;
        }

        return true;
    }

    fn load(&self, proc: &Arc<Process>, info: &mut ExecInfo) -> EResult<Task> {
        let page_size = arch::virt::get_page_size();

        // Load the main executable. The base address only matters if the type is ET_DYN.
        // Base could technically be 0, but we don't want to get anywhere near the NULL address.
        let elf = Self::load_file(&info.executable.clone(), proc, info, 0x10000)?;

        // If we have an interpreter, we need to use its entry point.
        let entry = if let Some(x) = &info.interpreter {
            let interp = Self::load_file(
                &x.clone(),
                proc,
                info,
                1usize << (arch::virt::get_highest_bit_shift() - 2),
            )?;
            interp.at_entry
        } else {
            elf.at_entry
        };

        // Setup stack.
        // Calculate the start of the user address.
        let highest = (1usize << (arch::virt::get_highest_bit_shift() - 1)) - page_size;
        let stack_size = 2 * 1024 * 1024; // 2MiB stack.
        let stack_start = highest - stack_size;

        let stack: Arc<dyn MemoryObject> = Arc::new(PagedMemoryObject::new_phys());
        info.space.map_object(
            stack.clone(),
            stack_start.into(),
            NonZeroUsize::new(stack_size).unwrap(),
            VmFlags::Read | VmFlags::Write,
            0,
        )?;

        let mut stack_off = stack_size;
        let mut envp_offsets = Vec::with_capacity(info.envp.len());
        let mut argv_offsets = Vec::with_capacity(info.argv.len());

        for env in &info.envp {
            stack_off -= 1;
            stack.write(&[0u8], stack_off);
            stack_off -= env.len();
            stack.write(env, stack_off);
            envp_offsets.push(stack_start + stack_off);
        }

        for arg in &info.argv {
            stack_off -= 1;
            stack.write(&[0u8], stack_off);
            stack_off -= arg.len();
            stack.write(arg, stack_off);
            argv_offsets.push(stack_start + stack_off);
        }

        stack_off = align_down(stack_off, 16);
        // Align the stack if argc + argv + envp does not add up to 16 byte alignment.
        if (1 + info.argv.len() + info.envp.len()) % 2 == 1 {
            stack_off -= size_of::<usize>();
            stack.write(&0usize.to_ne_bytes(), stack_off);
        }

        // Write auxiliary values.
        let mut write_auxv = |auxv: u32, value: usize| {
            stack_off -= size_of::<usize>();
            stack.write(&value.to_ne_bytes(), stack_off);
            stack_off -= size_of::<usize>();
            stack.write(&(auxv as usize).to_ne_bytes(), stack_off);
        };

        write_auxv(AT_NULL, 0); // Terminator.
        write_auxv(AT_SECURE, 0);
        write_auxv(AT_PHDR, elf.at_phdr);
        write_auxv(AT_PHNUM, elf.at_phnum);
        write_auxv(AT_PHENT, elf.at_phent);
        write_auxv(AT_ENTRY, elf.at_entry);

        // envp pointers
        stack_off -= size_of::<usize>();
        stack.write(&0usize.to_ne_bytes(), stack_off);
        for env in envp_offsets.iter().rev() {
            stack_off -= size_of::<usize>();
            stack.write(&env.to_ne_bytes(), stack_off);
        }

        // argv pointers
        stack_off -= size_of::<usize>();
        stack.write(&0usize.to_ne_bytes(), stack_off);
        for arg in argv_offsets.iter().rev() {
            stack_off -= size_of::<usize>();
            stack.write(&arg.to_ne_bytes(), stack_off);
        }

        stack_off -= size_of::<usize>();
        stack.write(&info.argv.len().to_ne_bytes(), stack_off);

        assert!(stack_off % 16 == 0);

        // Create the main thread.
        Task::new(to_user, entry, highest - stack_size + stack_off, proc, true)
    }
}

#[initgraph::task(
    name = "generic.vfs.exec-elf",
    depends = [crate::memory::MEMORY_STAGE],
    entails = [crate::vfs::VFS_STAGE],
)]
fn ELF_STAGE() {
    super::register("elf", Arc::new(ElfFormat));
}
