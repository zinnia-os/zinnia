use crate::arch;
use crate::memory::pmm::{AllocFlags, KernelAlloc, PageAllocator};
use crate::memory::virt::{self, KERNEL_VIRTUAL_ALLOCATOR, VmFlags, mmu::PageTable};
use crate::memory::{PhysAddr, VirtAddr};
use crate::posix::errno::{EResult, Errno};
use crate::util::{align_down, align_up, mutex::spin::SpinMutex};
use crate::vfs::exec::elf::{self, ElfHashTable, ElfHdr, ElfPhdr, ElfRela, ElfSym};
use alloc::{borrow::ToOwned, collections::btree_map::BTreeMap, string::String, vec::Vec};
use core::str;
use core::{ffi::CStr, num::NonZeroUsize, slice};

// TODO: This can use RwLocks.
pub(crate) static SYMBOL_TABLE: SpinMutex<BTreeMap<String, (elf::ElfSym, Option<&ModuleInfo>)>> =
    SpinMutex::new(BTreeMap::new());

pub(crate) static MODULE_TABLE: SpinMutex<BTreeMap<String, ModuleInfo>> =
    SpinMutex::new(BTreeMap::new());

unsafe extern "C" {
    unsafe static LD_DYNSYM_START: u8;
    unsafe static LD_DYNSYM_END: u8;
    unsafe static LD_DYNSTR_START: u8;
    unsafe static LD_DYNSTR_END: u8;
}

type ModuleEntryFn = extern "C" fn(*const u8, usize);

/// Stores metadata about a module.
#[derive(Debug)]
pub struct ModuleInfo {
    pub version: String,
    pub description: String,
    pub author: String,
    pub entry: Option<ModuleEntryFn>,
    pub mappings: Vec<(PhysAddr, VirtAddr, usize, VmFlags)>,
}

/// Records all mappings made while loading.
/// If [`Self::disarm()`] is not called, before it is dropped, it reverts all mappings made by the kernel.
/// This is used to prevent resource leaks after memory is freed.
struct ModuleLoadGuard {
    virt: VirtAddr,
    length: NonZeroUsize,
    mappings: Vec<(PhysAddr, VirtAddr, usize)>,
    active: bool,
}

impl ModuleLoadGuard {
    fn new(virt: VirtAddr, length: NonZeroUsize) -> Self {
        Self {
            virt,
            length,
            mappings: Vec::new(),
            active: true,
        }
    }

    fn disarm(&mut self) {
        self.active = false;
    }
}

impl Drop for ModuleLoadGuard {
    fn drop(&mut self) {
        if !self.active {
            return;
        }

        let page_size = arch::virt::get_page_size();
        let page_table = PageTable::get_kernel();
        for (phys, virt, length) in &self.mappings {
            let length = align_up(*length, page_size);
            for page in (0..length).step_by(page_size) {
                _ = page_table.unmap_single::<KernelAlloc>(*virt + page);
            }

            unsafe { KernelAlloc::dealloc_bytes(*phys, length) };
        }

        _ = KERNEL_VIRTUAL_ALLOCATOR
            .get()
            .lock()
            .release(self.virt, self.length);
    }
}

/// Sets up the module system.
#[initgraph::task(
    name = "generic.module",
    depends = [super::memory::MEMORY_STAGE],
)]
fn MODULE_STAGE() {
    let dynsym_start = &raw const LD_DYNSYM_START;
    let dynsym_end = &raw const LD_DYNSYM_END;
    let dynstr_start = &raw const LD_DYNSTR_START;
    let dynstr_end = &raw const LD_DYNSTR_END;

    // Add all kernel symbols to a table so we can perform dynamic linking.
    {
        let symbols = unsafe {
            slice::from_raw_parts_mut(
                dynsym_start as *mut elf::ElfSym,
                (dynsym_end as usize - dynsym_start as usize) / size_of::<elf::ElfSym>(),
            )
        };
        let strings = unsafe {
            slice::from_raw_parts(dynstr_start, dynstr_end as usize - dynstr_start as usize)
        };

        let mut symbol_table = SYMBOL_TABLE.lock();
        for sym in symbols {
            // Fix the addresses in the symbols because relocating doesn't relocate the symbol address.
            sym.st_value += &raw const virt::LD_KERNEL_START as u64;

            let name = CStr::from_bytes_until_nul(&strings[sym.st_name as usize..]);
            if let Ok(x) = name
                && let Ok(s) = x.to_str()
                && !s.is_empty()
            {
                let result = symbol_table.insert(s.to_owned(), (*sym, None));
                assert!(result.is_none(), "Duplicate symbol names!");
            }
        }
        log!("Registered {} kernel symbols", symbol_table.len());
    }
}

/// Loads a module from an ELF in memory.
pub fn load(data: &[u8], cmdline: &[u8]) -> EResult<()> {
    let elf_hdr: &ElfHdr =
        bytemuck::try_from_bytes(&data[0..size_of::<ElfHdr>()]).map_err(|_| Errno::ENOEXEC)?;

    if elf_hdr.e_ident[0..4] != elf::ELF_MAG
        || elf_hdr.e_ident[elf::EI_VERSION] != elf::EV_CURRENT
        // TODO: This is set to _LINUX on my toolchain.
        // || elf_hdr.e_ident[elf::EI_OSABI] != elf::ELFOSABI_SYSV
        || elf_hdr.e_machine != elf::EM_CURRENT
    {
        return Err(Errno::ENOEXEC);
    }

    #[cfg(target_pointer_width = "32")]
    if elf_hdr.e_ident[elf::EI_CLASS] != elf::ELFCLASS32 {
        return Err(Errno::ENOEXEC);
    }
    #[cfg(target_pointer_width = "64")]
    if elf_hdr.e_ident[elf::EI_CLASS] != elf::ELFCLASS64 {
        return Err(Errno::ENOEXEC);
    }
    #[cfg(target_endian = "little")]
    if elf_hdr.e_ident[elf::EI_DATA] != elf::ELFDATA2LSB {
        return Err(Errno::ENOEXEC);
    }
    #[cfg(target_endian = "big")]
    if elf_hdr.e_ident[EI_DATA] != ELFDATA2MSB {
        return Err(Errno::ENOEXEC);
    }

    // Start by evaluating the program headers.
    let phdrs: &[ElfPhdr] = bytemuck::try_cast_slice(
        &data[elf_hdr.e_phoff as usize
            ..(elf_hdr.e_phoff as usize + elf_hdr.e_phnum as usize * size_of::<ElfPhdr>())],
    )
    .map_err(|_| Errno::ENOEXEC)?;

    let page_size = arch::virt::get_page_size();
    let mut load_min = usize::MAX;
    let mut load_end = 0usize;

    for phdr in phdrs.iter().filter(|phdr| phdr.p_type == elf::PT_LOAD) {
        let aligned_virt = align_down(phdr.p_vaddr as usize, page_size);
        let misalign = phdr.p_vaddr as usize - aligned_virt;
        let memsz = align_up(phdr.p_memsz as usize + misalign, page_size);
        let end = aligned_virt.checked_add(memsz).ok_or(Errno::ENOMEM)?;

        load_min = load_min.min(aligned_virt);
        load_end = load_end.max(end);
    }

    if load_min == usize::MAX || load_end <= load_min {
        return Err(Errno::EINVAL);
    }

    let load_size = NonZeroUsize::new(load_end - load_min).ok_or(Errno::EINVAL)?;
    let load_addr = KERNEL_VIRTUAL_ALLOCATOR
        .get()
        .lock()
        .allocate(load_size)
        .map_err(|_| Errno::ENOMEM)?;
    let load_base = load_addr
        .value()
        .checked_sub(load_min)
        .ok_or(Errno::ENOMEM)?;
    let mut load_guard = ModuleLoadGuard::new(load_addr, load_size);

    let mut info = ModuleInfo {
        version: String::new(),
        description: String::new(),
        author: String::new(),
        entry: None,
        mappings: Vec::new(),
    };

    // Variables read from the dynamic segment.
    let mut dt_strtab = None;
    let mut dt_strsz = None;
    let mut dt_symtab = None;
    let mut dt_rela = None;
    let mut dt_relasz = None;
    let mut dt_pltrelsz = None;
    let mut dt_jmprel = None;
    let mut dt_init_array = None;
    let mut dt_hash = None;
    let mut dt_soname = None;
    let mut dt_needed = Vec::new();

    for phdr in phdrs {
        match phdr.p_type {
            // Load the segment into memory.
            elf::PT_LOAD => {
                // Fix potentially unaligned addresses.
                let aligned_virt = align_down(phdr.p_vaddr as usize, page_size);
                let misalign = phdr.p_vaddr as usize - aligned_virt;
                let memsz = align_up(phdr.p_memsz as usize + misalign, page_size);

                // Allocate physical memory.
                let phys = KernelAlloc::alloc_bytes(memsz, AllocFlags::empty())?;

                let page_table = PageTable::get_kernel();
                let virt = (load_base + aligned_virt).into();
                load_guard.mappings.push((phys, virt, memsz));

                // Map memory with RW permissions.
                for page in (0..memsz).step_by(page_size) {
                    page_table
                        .map_single::<KernelAlloc>(
                            (load_base + aligned_virt + page).into(),
                            phys + page,
                            VmFlags::Read | VmFlags::Write,
                        )
                        .map_err(|_| Errno::ENOMEM)?;
                }

                let virt = load_base + phdr.p_vaddr as usize;

                // Copy data to allocated memory.
                let buf =
                    unsafe { slice::from_raw_parts_mut(virt as *mut u8, phdr.p_memsz as usize) };
                buf.copy_from_slice(&data[phdr.p_offset as usize..][..phdr.p_filesz as usize]);
                buf[phdr.p_filesz as usize..].fill(0);

                // Convert the flags to our format.
                let mut flags = VmFlags::empty();
                if phdr.p_flags & elf::PF_EXECUTE != 0 {
                    flags |= VmFlags::Exec;
                }
                if phdr.p_flags & elf::PF_READ != 0 {
                    flags |= VmFlags::Read;
                }
                if phdr.p_flags & elf::PF_WRITE != 0 {
                    flags |= VmFlags::Write;
                }

                // Record this mapping.
                info.mappings
                    .push((phys, (load_base + aligned_virt).into(), memsz, flags));
            }
            elf::PT_DYNAMIC => {
                let dyntab: &[elf::ElfDyn] = bytemuck::try_cast_slice(
                    &data[phdr.p_offset as usize..][..phdr.p_filesz as usize],
                )
                .map_err(|_| Errno::EINVAL)?;

                for entry in dyntab {
                    match entry.d_tag as u32 {
                        elf::DT_STRTAB => dt_strtab = Some(entry.d_val),
                        elf::DT_SYMTAB => dt_symtab = Some(entry.d_val),
                        elf::DT_STRSZ => dt_strsz = Some(entry.d_val),
                        elf::DT_RELA => dt_rela = Some(entry.d_val),
                        elf::DT_RELASZ => dt_relasz = Some(entry.d_val),
                        elf::DT_PLTRELSZ => dt_pltrelsz = Some(entry.d_val),
                        elf::DT_JMPREL => dt_jmprel = Some(entry.d_val),
                        elf::DT_INIT_ARRAY => dt_init_array = Some(entry.d_val),
                        elf::DT_HASH => dt_hash = Some(entry.d_val),
                        elf::DT_NEEDED => dt_needed.push(entry.d_val),
                        elf::DT_SONAME => dt_soname = Some(entry.d_val),
                        elf::DT_NULL => break,
                        _ => (),
                    }
                }
            }
            elf::PT_MODVERSION => {
                info.version =
                    str::from_utf8(&data[phdr.p_offset as usize..][..phdr.p_filesz as usize])
                        .map_err(|_| Errno::EBADMSG)?
                        .to_owned();
            }
            elf::PT_MODAUTHOR => {
                info.author =
                    str::from_utf8(&data[phdr.p_offset as usize..][..phdr.p_filesz as usize])
                        .map_err(|_| Errno::EBADMSG)?
                        .to_owned();
            }
            elf::PT_MODDESC => {
                info.description =
                    str::from_utf8(&data[phdr.p_offset as usize..][..phdr.p_filesz as usize])
                        .map_err(|_| Errno::EBADMSG)?
                        .to_owned();
            }
            // Unknown or unhandled type. Do nothing.
            _ => (),
        }
    }

    // Convert addresses to offsets in the binary so we can read their values.
    let fix_addr = |opt: &mut Option<_>| {
        if let Some(x) = opt {
            for phdr in phdrs {
                if *x >= phdr.p_vaddr && *x < phdr.p_vaddr + phdr.p_filesz {
                    *x -= phdr.p_vaddr;
                    *x += phdr.p_offset;
                    break;
                }
            }
        }
    };

    fix_addr(&mut dt_strtab);
    fix_addr(&mut dt_symtab);
    fix_addr(&mut dt_rela);
    fix_addr(&mut dt_jmprel);
    fix_addr(&mut dt_init_array);
    fix_addr(&mut dt_hash);

    let strtab = &data[dt_strtab.unwrap() as usize..][..dt_strsz.unwrap() as usize];

    // Load symbol table. To get the size of it, we need to look at the DT_HASH tag.
    let symtab_len = bytemuck::try_from_bytes::<ElfHashTable>(
        &data[dt_hash.unwrap() as usize..][..size_of::<ElfHashTable>()],
    )
    .map_err(|_| Errno::EINVAL)?
    .nchain as usize;

    let symtab: &[ElfSym] = bytemuck::try_cast_slice(
        &data[dt_symtab.unwrap() as usize..][..symtab_len * size_of::<ElfSym>()],
    )
    .map_err(|_| Errno::EINVAL)?;

    let name = CStr::from_bytes_until_nul(&strtab[dt_soname.unwrap() as usize..])
        .unwrap()
        .to_str()
        .unwrap();

    let dependencies = dt_needed
        .as_slice()
        .iter()
        .map(|x| {
            CStr::from_bytes_until_nul(&strtab[(*x) as usize..])
                .unwrap()
                .to_str()
                .unwrap()
        })
        // "zinnia.kso" is the kernel itself. We don't actually have to load that :)
        .filter(|x| *x != "zinnia.kso")
        .collect::<Vec<_>>();

    // TODO: Load dependencies
    for dep in dependencies.iter() {
        if !MODULE_TABLE.lock().contains_key(*dep) {
            error!(
                "Missing module dependency \"{}\", required to load \"{}\"",
                dep, name
            );
            return Err(Errno::ENOENT);
        }
    }

    // Handle relocations.
    let do_reloc = |addr: _, size: _| -> _ {
        let relas: &[ElfRela] = bytemuck::try_cast_slice(&data[addr as usize..][..size as usize])
            .map_err(|_| Errno::EINVAL)?;

        for rela in relas {
            // The symbol index is stored in the upper 32 bits.
            let sym = (rela.r_info >> 32) as u32;
            let typ = (rela.r_info & 0xFFFF_FFFF) as u32;

            let symbol = symtab[sym as usize];

            // The address where to write the relocated address to.
            let location = (load_base + rela.r_offset as usize) as *mut usize;

            // Do the relocation.
            match typ {
                elf::R_COMMON_NONE => (),
                // Some ISAs have multiple relocation types with the same value.
                #[allow(unreachable_patterns)]
                elf::R_COMMON_64 | elf::R_COMMON_GLOB_DAT | elf::R_COMMON_JUMP_SLOT => {
                    // Check if this symbol has an associated section.
                    // If it does not, we need to look the symbol up in our own list.
                    let resolved = if symbol.st_shndx == 0 {
                        // Get the symbol name.
                        let name = CStr::from_bytes_until_nul(&strtab[symbol.st_name as usize..])
                            .map_err(|_| Errno::EINVAL)?
                            .to_str()
                            .map_err(|_| Errno::EINVAL)?;
                        let kernel_symbol = SYMBOL_TABLE.lock().get(name).ok_or(Errno::EINVAL)?.0;

                        kernel_symbol.st_value as usize
                    } else {
                        load_base + symbol.st_value as usize
                    };

                    unsafe {
                        *location = resolved + rela.r_addend as usize;
                    }
                }
                elf::R_COMMON_RELATIVE => unsafe {
                    *location = load_base + rela.r_addend as usize;
                },
                _ => return Err(Errno::EINVAL),
            }
        }
        Ok(())
    };

    if let Some(addr) = dt_rela {
        do_reloc(addr, dt_relasz.ok_or(Errno::EINVAL)?)?;
    }
    if let Some(addr) = dt_jmprel {
        do_reloc(addr, dt_pltrelsz.ok_or(Errno::EINVAL)?)?;
    }

    // Finally, remap everything so the permissions are as described.
    for (_, virt, length, flags) in &info.mappings {
        let length = align_up(*length, arch::virt::get_page_size());
        let page_table = PageTable::get_kernel();
        for page in (0..length).step_by(arch::virt::get_page_size()) {
            page_table
                .remap_single::<KernelAlloc>(*virt + page, *flags)
                .map_err(|_| Errno::ENOMEM)?;
        }
    }

    // Register newly added symbols for dependencies.
    for _symbol in symtab {
        // TODO: Add symbols
    }

    log!(
        "Loaded module \"{}\" ({}, {}) at {:#x}",
        name,
        info.description,
        info.version,
        load_base
    );

    // TODO: Call init array

    // Call the entry.
    info.entry = unsafe {
        Some(core::mem::transmute::<usize, ModuleEntryFn>(
            load_base + elf_hdr.e_entry as usize,
        ))
    };

    if let Some(entry_point) = info.entry {
        // Make sure it's a valid string.
        let cmd_str = str::from_utf8(cmdline).map_err(|_| Errno::EINVAL)?;
        (entry_point)(cmd_str.as_ptr(), cmd_str.len());
    }

    load_guard.disarm();
    MODULE_TABLE.lock().insert(name.to_owned(), info);

    return Ok(());
}

#[doc(hidden)]
#[macro_export]
macro_rules! define_string_section {
    (expanded $(#[$meta:meta])* $name:ident $src:expr) => {
        #[doc(hidden)]
        #[used]
        $(#[$meta])*
        static $name: [u8; $src.len()] = {
            let mut buf = [0u8; $src.len()];
            let src = $src;
            let mut idx = 0;
            while idx < src.len() {
                buf[idx] = src[idx];
                idx += 1;
            }
            buf
        };
    };
    ($($(#[$meta:meta])* static $name:ident = $str:expr;)*) => {
        $(
            $crate::define_string_section!(expanded $(#[$meta])* $name $str.as_bytes());
        )*
    };
}

/// Used to declare a crate as a module. The function passed to the macro is used as an entry point when the module is loaded.
#[macro_export]
macro_rules! module {
    ($desc: expr, $author: expr, $entry: ident) => {
        $crate::define_string_section! {
            #[unsafe(link_section = ".mod.version")]
            static MODULE_VERSION = env!("CARGO_PKG_VERSION");

            #[unsafe(link_section = ".mod.desc")]
            static MODULE_DESC = $desc;

            #[unsafe(link_section = ".mod.author")]
            static MODULE_AUTHOR = $author;
        }

        #[feature(str_from_raw_parts)]
        #[unsafe(no_mangle)]
        unsafe extern "C" fn _start(cmdline_ptr: *const u8, cmdline_len: usize) {
            let cmdline = unsafe {
                str::from_utf8_unchecked(core::slice::from_raw_parts(cmdline_ptr, cmdline_len))
            };

            $entry(cmdline);
        }
    };
}
