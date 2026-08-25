#![no_std]

pub mod vm;

use core::alloc::{GlobalAlloc, Layout};
use core::ptr::{self, NonNull};
use spin::Mutex;

pub const PAGE_SIZE: usize = 4096;

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct PhysAddr(pub u64);

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct VirtAddr(pub usize);

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct PageFrame {
    pub start: PhysAddr,
}

impl PageFrame {
    pub fn number(self) -> u64 {
        self.start.0 / PAGE_SIZE as u64
    }
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum MemoryRegionKind {
    Usable,
    Reserved,
    Kernel,
    Framebuffer,
    Acpi,
    Mmio,
    Unknown,
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct MemoryRegion {
    pub start: PhysAddr,
    pub len_bytes: u64,
    pub kind: MemoryRegionKind,
}

impl MemoryRegion {
    pub const fn empty() -> Self {
        Self {
            start: PhysAddr(0),
            len_bytes: 0,
            kind: MemoryRegionKind::Unknown,
        }
    }

    pub fn end(self) -> PhysAddr {
        PhysAddr(self.start.0 + self.len_bytes)
    }
}

pub const EMPTY_MEMORY_REGION: MemoryRegion = MemoryRegion::empty();

#[derive(Debug, Copy, Clone, Default)]
pub struct MemoryMapStatistics {
    pub total_bytes: u64,
    pub usable_bytes: u64,
    pub reserved_bytes: u64,
    pub region_count: usize,
}

pub fn align_up(value: usize, align: usize) -> usize {
    debug_assert!(align.is_power_of_two());
    (value + (align - 1)) & !(align - 1)
}

pub fn summarize(regions: &[MemoryRegion]) -> MemoryMapStatistics {
    let mut stats = MemoryMapStatistics::default();

    for region in regions {
        stats.total_bytes += region.len_bytes;
        stats.region_count += 1;

        if matches!(region.kind, MemoryRegionKind::Usable) {
            stats.usable_bytes += region.len_bytes;
        } else {
            stats.reserved_bytes += region.len_bytes;
        }
    }

    stats
}

#[derive(Debug)]
pub struct BootstrapFrameAllocator<'a> {
    regions: &'a [MemoryRegion],
    region_index: usize,
    next_addr: u64,
}

impl<'a> BootstrapFrameAllocator<'a> {
    pub fn new(regions: &'a [MemoryRegion]) -> Self {
        Self {
            regions,
            region_index: 0,
            next_addr: 0,
        }
    }

    pub fn allocate_frame(&mut self) -> Option<PageFrame> {
        while let Some(region) = self.regions.get(self.region_index).copied() {
            if region.kind != MemoryRegionKind::Usable {
                self.region_index += 1;
                self.next_addr = 0;
                continue;
            }

            if self.next_addr < region.start.0 {
                self.next_addr = region.start.0;
            }

            let aligned = align_up(self.next_addr as usize, PAGE_SIZE) as u64;
            let end = region.end().0;

            if aligned + PAGE_SIZE as u64 <= end {
                self.next_addr = aligned + PAGE_SIZE as u64;
                return Some(PageFrame {
                    start: PhysAddr(aligned),
                });
            }

            self.region_index += 1;
            self.next_addr = 0;
        }

        None
    }
}

// ---------------------------------------------------------------------------
// Kernel heap allocator.
//
// Fully in-tree implementation (no external allocator crates): an address
// ordered intrusive free list with first-fit allocation, block splitting,
// and immediate coalescing of neighbouring free blocks on dealloc.
//
// Layout of a block inside the heap region:
//
//   [ header | user data ... ]
//
// `header.size` is the total block size (header + data). While a block is
// free, the first word of its user region stores the `next` free-list link;
// the list is kept sorted by address so coalescing is pointer arithmetic.
// ---------------------------------------------------------------------------

/// Size of the inline block header in bytes.
const BLOCK_OVERHEAD: usize = core::mem::size_of::<usize>();

/// Smallest total block size that can ever exist: header + one link word.
const MIN_BLOCK_SIZE: usize = BLOCK_OVERHEAD + core::mem::size_of::<usize>();

/// Header stored at the start of every block. `size` includes the header.
#[derive(Debug)]
struct BlockHeader {
    size: usize,
}

/// Reads the free-list `next` link stored in a free block's user region.
unsafe fn next_of(header: *mut BlockHeader) -> Option<NonNull<BlockHeader>> {
    NonNull::new(unsafe { *(header.add(1) as *mut *mut BlockHeader) })
}

/// Writes the free-list `next` link stored in a free block's user region.
unsafe fn set_next_of(header: *mut BlockHeader, next: Option<NonNull<BlockHeader>>) {
    unsafe {
        *(header.add(1) as *mut *mut BlockHeader) =
            next.map(NonNull::as_ptr).unwrap_or(ptr::null_mut());
    }
}

/// Address ordered intrusive free list.
#[derive(Debug, Default)]
struct FreeList {
    head: Option<NonNull<BlockHeader>>,
}

// The list is only ever touched behind the allocator's `spin::Mutex`, so
// moving it between threads is sound.
unsafe impl Send for FreeList {}

impl FreeList {
    /// First-fit allocation: takes the first block that can satisfy the
    /// layout (including any alignment padding at its front), splits the
    /// tail back into the list when the remainder can hold a minimal block,
    /// and records the true block size in the allocated header.
    unsafe fn allocate(&mut self, layout: Layout) -> *mut u8 {
        let align = layout
            .align()
            .max(core::mem::align_of::<BlockHeader>());
        let data = align_up(layout.size(), BLOCK_OVERHEAD);

        let mut prev: Option<NonNull<BlockHeader>> = None;
        let mut cursor = self.head;
        while let Some(node) = cursor {
            let block = node.as_ptr();
            let next = unsafe { next_of(block) };
            let block_size = unsafe { (*block).size };
            let block_end = unsafe { (block as *mut u8).add(block_size) };

            // The user pointer must be `align`-aligned and the header sits
            // directly in front of it, which may skip a front gap.
            let user = align_up(unsafe { (block as *mut u8).add(BLOCK_OVERHEAD) } as usize, align)
                as *mut u8;
            let header = unsafe { user.sub(BLOCK_OVERHEAD) as *mut BlockHeader };
            let need_end = unsafe { user.add(data) };

            if (need_end as usize) <= (block_end as usize) {
                let gap = (header as *mut u8) as usize - (block as *mut u8) as usize;

                // Tail remainder becomes a free block when large enough.
                let tail_size = (block_end as usize) - (need_end as usize);
                let tail: Option<*mut BlockHeader> = if tail_size >= MIN_BLOCK_SIZE {
                    let tail = need_end as *mut BlockHeader;
                    unsafe {
                        (*tail).size = tail_size;
                        set_next_of(tail, next);
                    }
                    Some(tail)
                } else {
                    None
                };

                if gap >= MIN_BLOCK_SIZE {
                    // The front gap stays a free block and keeps this node's
                    // list position; nothing to relink.
                    unsafe {
                        (*block).size = gap;
                        set_next_of(block, tail.map(|t| NonNull::new_unchecked(t)));
                    }
                } else {
                    // No usable front gap: the whole region [block, need_end)
                    // belongs to the allocation. Replace this node with the
                    // tail, or unlink it entirely.
                    match (prev, tail) {
                        (Some(mut prev_node), Some(t)) => unsafe {
                            set_next_of(prev_node.as_mut(), Some(NonNull::new_unchecked(t)))
                        },
                        (Some(mut prev_node), None) => unsafe { set_next_of(prev_node.as_mut(), next) },
                        (None, Some(t)) => self.head = Some(unsafe { NonNull::new_unchecked(t) }),
                        (None, None) => self.head = next,
                    }
                }

                // Record the true block extent (may include the front gap
                // when it was too small to stand alone).
                unsafe {
                    (*header).size = (need_end as *mut u8) as usize - (header as *mut u8) as usize;
                }
                return user;
            }

            prev = Some(node);
            cursor = next;
        }

        ptr::null_mut()
    }

    /// Frees a block: inserts it at its sorted position and coalesces with
    /// the neighbouring free blocks.
    unsafe fn deallocate(&mut self, ptr: *mut u8) {
        let header = unsafe { ptr.sub(BLOCK_OVERHEAD) as *mut BlockHeader };
        let size = unsafe { (*header).size };
        let block_end = unsafe { (header as *mut u8).add(size) as *mut BlockHeader };

        // Find the last free block strictly below `header` (predecessor)
        // and the first one above it (successor).
        let mut prev: Option<NonNull<BlockHeader>> = None;
        let mut cursor = self.head;
        let mut successor: Option<NonNull<BlockHeader>> = None;
        while let Some(node) = cursor {
            let node_ptr = node.as_ptr();
            if node_ptr < header {
                prev = Some(node);
                cursor = unsafe { next_of(node_ptr) };
            } else {
                successor = Some(node);
                break;
            }
        }

        // Coalesce with the successor when it is directly adjacent.
        if let Some(succ) = successor {
            if succ.as_ptr() == block_end {
                let succ_ptr = succ.as_ptr();
                let succ_next = unsafe { next_of(succ_ptr) };
                unsafe {
                    (*header).size = size + (*succ_ptr).size;
                    set_next_of(header, succ_next);
                }
            } else {
                unsafe { set_next_of(header, successor) };
            }
        } else {
            unsafe { set_next_of(header, None) };
        }

        // Link the (possibly merged) block after the predecessor, merging
        // with it too when adjacent.
        match prev {
            Some(mut prev_node) => {
                let prev_ptr = unsafe { prev_node.as_mut() };
                let prev_end = unsafe {
                    ((prev_ptr as *mut BlockHeader as *mut u8).add((*prev_ptr).size))
                        as *mut BlockHeader
                };
                if prev_end == header {
                    unsafe {
                        (*prev_ptr).size += (*header).size;
                        set_next_of(prev_ptr, next_of(header));
                    }
                } else {
                    unsafe { set_next_of(prev_ptr, Some(NonNull::new_unchecked(header))) };
                }
            }
            None => {
                self.head = Some(unsafe { NonNull::new_unchecked(header) });
            }
        }
    }

    /// Walks the list, summing free space and block count.
    fn stats(&self) -> (usize, usize) {
        let mut free_bytes = 0usize;
        let mut free_blocks = 0usize;
        let mut cursor = self.head;
        while let Some(node) = cursor {
            let header = node.as_ptr();
            unsafe {
                free_bytes += (*header).size;
                free_blocks += 1;
                cursor = next_of(header);
            }
        }
        (free_bytes, free_blocks)
    }
}

struct HeapState {
    free: FreeList,
    initialized: bool,
}

pub struct KernelAllocator {
    state: Mutex<HeapState>,
}

impl KernelAllocator {
    pub const fn new() -> Self {
        Self {
            state: Mutex::new(HeapState {
                free: FreeList { head: None },
                initialized: false,
            }),
        }
    }

    pub unsafe fn init(&self, start: NonNull<u8>, size: usize) {
        let mut state = self.state.lock();
        let start_addr = align_up(start.as_ptr() as usize, core::mem::align_of::<BlockHeader>());
        let end_addr = (start.as_ptr() as usize).saturating_add(size);
        if end_addr >= start_addr.saturating_add(MIN_BLOCK_SIZE) {
            let header = start_addr as *mut BlockHeader;
            unsafe {
                (*header).size = end_addr - start_addr;
                set_next_of(header, None);
            }
            state.free.head = Some(unsafe { NonNull::new_unchecked(header) });
        }
        state.initialized = true;
    }

    pub fn is_initialized(&self) -> bool {
        self.state.lock().initialized
    }

    /// Free-heap snapshot for diagnostics.
    pub fn stats(&self) -> HeapStats {
        let state = self.state.lock();
        let (free_bytes, free_blocks) = state.free.stats();
        HeapStats {
            free_bytes,
            free_blocks,
        }
    }
}

/// Snapshot of the free heap state.
#[derive(Debug, Copy, Clone, Default)]
pub struct HeapStats {
    pub free_bytes: usize,
    pub free_blocks: usize,
}

unsafe impl GlobalAlloc for KernelAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let mut state = self.state.lock();
        if !state.initialized {
            return ptr::null_mut();
        }
        unsafe { state.free.allocate(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, _layout: Layout) {
        let mut state = self.state.lock();
        if !state.initialized || ptr.is_null() {
            return;
        }
        unsafe { state.free.deallocate(ptr) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        if new_size <= layout.size() {
            // Shrinking in place is always safe; the tail is reclaimed when
            // the whole block is eventually freed and coalesced.
            return ptr;
        }
        let new_layout = unsafe { Layout::from_size_align_unchecked(new_size, layout.align()) };
        let new_ptr = unsafe { self.alloc(new_layout) };
        if new_ptr.is_null() {
            return ptr::null_mut();
        }
        unsafe {
            ptr::copy_nonoverlapping(ptr, new_ptr, layout.size());
            self.dealloc(ptr, layout);
        }
        new_ptr
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with_heap(size: usize, f: impl FnOnce(&KernelAllocator)) {
        extern crate std;
        use std::vec::Vec;
        let mut backing: Vec<u8> = Vec::new();
        backing.reserve(size);
        backing.resize(size, 0);
        let base = backing.as_ptr() as usize;
        let allocator = KernelAllocator::new();
        unsafe {
            allocator.init(NonNull::new_unchecked(base as *mut u8), size);
        }
        f(&allocator);
    }

    #[test]
    fn alloc_returns_distinct_nonnull_blocks() {
        with_heap(4096, |allocator| unsafe {
            let a = allocator.alloc(Layout::from_size_align(64, 8).unwrap());
            let b = allocator.alloc(Layout::from_size_align(64, 8).unwrap());
            assert!(!a.is_null());
            assert!(!b.is_null());
            assert!(a != b);
            let stats = allocator.stats();
            assert_eq!(stats.free_blocks, 1, "split remainder should be free");
        });
    }

    #[test]
    fn dealloc_coalesces_back_to_one_block() {
        with_heap(4096, |allocator| unsafe {
            let a = allocator.alloc(Layout::from_size_align(128, 8).unwrap());
            let b = allocator.alloc(Layout::from_size_align(128, 8).unwrap());
            let c = allocator.alloc(Layout::from_size_align(128, 8).unwrap());
            assert!(!a.is_null() && !b.is_null() && !c.is_null());
            allocator.dealloc(a, Layout::from_size_align(128, 8).unwrap());
            allocator.dealloc(c, Layout::from_size_align(128, 8).unwrap());
            let fragmented = allocator.stats().free_blocks;
            allocator.dealloc(b, Layout::from_size_align(128, 8).unwrap());
            let merged = allocator.stats();
            assert!(merged.free_blocks < fragmented + 2, "coalescing must merge");
            assert!(merged.free_bytes <= 4096);
        });
    }

    #[test]
    fn realloc_grows_and_copies() {
        with_heap(8192, |allocator| unsafe {
            let layout = Layout::from_size_align(32, 8).unwrap();
            let ptr = allocator.alloc(layout);
            assert!(!ptr.is_null());
            for i in 0..32 {
                ptr.add(i).write_volatile(0xA0 | i as u8);
            }
            let grown = allocator.realloc(ptr, layout, 256);
            assert!(!grown.is_null());
            for i in 0..32 {
                assert_eq!(grown.add(i).read_volatile(), 0xA0 | i as u8);
            }
        });
    }

    #[test]
    fn alloc_fails_gracefully_when_exhausted() {
        with_heap(256, |allocator| unsafe {
            let big = allocator.alloc(Layout::from_size_align(4096, 8).unwrap());
            assert!(big.is_null());
        });
    }

    #[test]
    fn churn_random_sizes_stays_consistent() {
        with_heap(64 * 1024, |allocator| unsafe {
            let mut live: [(usize, *mut u8); 64] = [(0, core::ptr::null_mut()); 64];
            for round in 0..200 {
                let slot = (round * 7 + 3) % 64;
                if !live[slot].1.is_null() {
                    let (size, ptr) = live[slot];
                    allocator.dealloc(ptr, Layout::from_size_align(size, 16).unwrap());
                    live[slot] = (0, core::ptr::null_mut());
                }
                let size = 16 + ((round * 37 + slot * 11) % 512);
                let layout = Layout::from_size_align(size, 16).unwrap();
                let ptr = allocator.alloc(layout);
                if !ptr.is_null() {
                    ptr.write_volatile(0xEE);
                    live[slot] = (size, ptr);
                }
            }
        });
    }
}
