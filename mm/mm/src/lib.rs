//! Memory management implementation for a no_std environment
//! 
//! This module provides virtual memory management functionality, including:
//! - Virtual memory areas (VMA) management
//! - Page table operations
//! - Memory mapping and unmapping
//! - Process memory space management
//! 
//! # Features
//! - No standard library dependency
//! - Support for file-backed mappings
//! - Process memory isolation
//! - Memory permission control
//!
//! # Core Components
//! - [`MmStruct`]: Main memory management structure for a process
//! - [`VmAreaStruct`]: Virtual memory area descriptor
//! 

#![no_std]
#![feature(btree_cursors)]

#[macro_use]
extern crate log;
extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::sync::Arc;
use core::cell::OnceCell;
use axfile::fops::File;
use page_table::paging::pgd_alloc;
use page_table::paging::MappingFlags;
use page_table::paging::PageTable;
use page_table::paging::PagingResult;
use axhal::mem::virt_to_phys;
use axtype::PAGE_SIZE;
use core::sync::atomic::AtomicUsize;
use core::sync::atomic::Ordering;
use spinbase::SpinNoIrq;
use mutex::Mutex;

pub type FileRef = Arc<Mutex<File>>;

static MM_UNIQUE_ID: AtomicUsize = AtomicUsize::new(1);

/*
 * vm_flags in vm_area_struct, see mm_types.h.
 * When changing, update also include/trace/events/mmflags.h
 */
/// Memory management related constants for virtual memory flags
/// These flags control memory area permissions and behaviors
/// 
/// No permissions
pub const VM_NONE: usize = 0x00000000;
/// Memory can be read
pub const VM_READ: usize = 0x00000001;
/// Memory can be written
pub const VM_WRITE: usize = 0x00000002;
/// Memory can be executed  
pub const VM_EXEC: usize = 0x00000004;
/// Memory is shared
pub const VM_SHARED: usize = 0x00000008;
/// May read in the future
pub const VM_MAYREAD : usize = 0x00000010;
/// May write in the future 
pub const VM_MAYWRITE: usize = 0x00000020;
/// May execute in the future
pub const VM_MAYEXEC : usize = 0x00000040;
/// May be shared in the future
pub const VM_MAYSHARE: usize = 0x00000080;
/// Stack segment that grows downward
pub const VM_GROWSDOWN: usize = 0x00000100;
/// Pages are locked in memory
pub const VM_LOCKED: usize = 0x00002000;
/// Synchronous page faults
pub const VM_SYNC: usize = 0x00800000;

/// Represents a virtual memory area with its properties and permissions
#[derive(Clone)]
pub struct VmAreaStruct {
    pub vm_start: usize,
    pub vm_end: usize,
    pub vm_pgoff: usize,
    pub vm_file: OnceCell<FileRef>,
    pub vm_flags: usize,
}

impl VmAreaStruct {
    /// Creates a new virtual memory area with specified parameters
    pub fn new(
        vm_start: usize,
        vm_end: usize,
        vm_pgoff: usize,
        vm_file: Option<FileRef>,
        vm_flags: usize,
    ) -> Self {
        let vma = Self {
            vm_start,
            vm_end,
            vm_pgoff,
            vm_file: OnceCell::new(),
            vm_flags,
        };
        if let Some(f) = vm_file {
            let _ = vma.vm_file.set(f);
        }
        vma
    }
}

/// Represents the memory management structure for a process
pub struct MmStruct {
    id: usize,
    pub vmas: BTreeMap<usize, VmAreaStruct>,
    pgd: Arc<SpinNoIrq<PageTable>>,
    brk: usize,

    // Todo: temprarily record mapped (va, pa)
    pub mapped: BTreeMap<usize, usize>,

    /// Pages that have PG_mlocked set
    pub locked_vm: usize,
}

impl MmStruct {
    /// Creates a new memory management structure with default values
    pub fn new() -> Self {
        Self {
            id: MM_UNIQUE_ID.fetch_add(1, Ordering::SeqCst),
            vmas: BTreeMap::new(),
            pgd: Arc::new(SpinNoIrq::new(pgd_alloc())),
            brk: 0,

            // Todo: temprarily record mapped (va, pa)
            mapped: BTreeMap::new(),
            locked_vm: 0,
        }
    }

    /// Creates a deep copy of the current memory management structure
    /// including all virtual memory areas and page mappings
    pub fn deep_dup(&self) -> Self {
        let mut pgd = pgd_alloc();

        let mut vmas = BTreeMap::new();
        for vma in self.vmas.values() {
            debug!("vma: {:#X} - {:#X}, {:#X}", vma.vm_start, vma.vm_end, vma.vm_pgoff);
            let new_vma = vma.clone();
            vmas.insert(vma.vm_start, new_vma);
        }

        let mut mapped = BTreeMap::<usize, usize>::new();
        for (va, dva) in &self.mapped {
            let va = *va;
            let old_page = *dva;
            debug!("mapped: {:#X} -> {:#X}", va, old_page);
            let new_page: usize = axalloc::global_allocator()
                .alloc_pages(1, PAGE_SIZE) .unwrap();

            unsafe {
                core::ptr::copy_nonoverlapping(
                    old_page as *const u8,
                    new_page as *mut u8,
                    PAGE_SIZE
                );
            }

            let pa = virt_to_phys(new_page.into());

            let flags = MappingFlags::READ | MappingFlags::WRITE |
                MappingFlags::EXECUTE | MappingFlags::USER;
            pgd.map_region(va.into(), pa.into(), PAGE_SIZE, flags, true).unwrap();
            mapped.insert(va, new_page);
        }
        Self {
            id: MM_UNIQUE_ID.fetch_add(1, Ordering::SeqCst),
            vmas,
            pgd: Arc::new(SpinNoIrq::new(pgd)),
            brk: self.brk,

            mapped,
            locked_vm: self.locked_vm,
        }
    }

    /// Returns a reference to the page global directory
    pub fn pgd(&self) -> Arc<SpinNoIrq<PageTable>> {
        self.pgd.clone()
    }

    /// Returns the physical address of the root page table
    pub fn root_paddr(&self) -> usize {
        self.pgd.lock().root_paddr().into()
    }

    /// Returns the unique identifier of this memory management structure
    pub fn id(&self) -> usize {
        self.id
    }

    
    /// Returns the current program break(heap end) location
    pub fn brk(&self) -> usize {
        self.brk
    }

    /// Sets a new program break(heap end) location
    pub fn set_brk(&mut self, brk: usize) {
        self.brk = brk;
    }

    /// Maps a virtual address region to a physical address with specified flags
    pub fn map_region(&self, va: usize, pa: usize, len: usize, _uflags: usize) -> PagingResult {
        let flags =
            MappingFlags::READ | MappingFlags::WRITE | MappingFlags::EXECUTE | MappingFlags::USER;
        self.pgd
            .lock()
            .map_region(va.into(), pa.into(), len, flags, true)
    }

    /// Unmaps a region of virtual memory
    pub fn unmap_region(&self, va: usize, len: usize) -> PagingResult {
        self.pgd.lock().unmap_region(va.into(), len)
    }
}
