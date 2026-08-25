//! 4-level x86_64 page tables (PML4 → PDPT → PD → PT).
//!
//! Stage-A building block: the mapper walks a page-table tree located in
//! an abstract "physical" memory ([`FrameMemory`]), so the logic is fully
//! host-testable. The kernel instantiates it over its real frame
//! allocator once paging is switched on (see `docs/stages/stage-a-*`).
//!
//! Layout constants per level: every table holds 512 entries; a
//! present entry stores `phys | flags`, where bit 63 = NX.

extern crate alloc;

use crate::PAGE_SIZE;

/// Number of entries in one page table.
pub const ENTRIES: usize = 512;

// Entry flag bits (x86_64).
pub const PRESENT: u64 = 1 << 0;
pub const WRITABLE: u64 = 1 << 1;
pub const USER: u64 = 1 << 2;
pub const NO_EXEC: u64 = 1 << 63;
/// Everything except the address bits and reserved — mask for flags.
pub const FLAGS_MASK: u64 = 0x000F_FFFF_0000_0FFF & !NO_EXEC | NO_EXEC;

/// Canonical virtual address.
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Virt(pub u64);

/// Physical address.
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Phys(pub u64);

/// Page-table mapping failures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MapError {
    /// The virtual page is already mapped.
    AlreadyMapped,
    /// No free frame to build an intermediate table.
    FrameExhausted,
    /// `unmap`/`translate` hit a non-present entry.
    NotMapped,
}

/// Abstract backing store for page-table frames (one frame = one table).
///
/// `alloc_frame` returns a fresh zeroed frame; `frame_mut`/`frame`
/// address the frame's 512 entries.
pub trait FrameMemory {
    fn alloc_frame(&mut self) -> Option<u64>;
    fn frame_mut(&mut self, frame: u64) -> Option<&mut [u64; ENTRIES]>;
    fn frame(&self, frame: u64) -> Option<&[u64; ENTRIES]>;
}

/// Page-table mapper rooted at a PML4 frame.
pub struct PageTableMapper {
    pub pml4_frame: u64,
}

/// Index of `virt` at the given level (0 = PML4, 3 = PT).
fn index_at(virt: Virt, level: usize) -> usize {
    ((virt.0 >> (12 + 9 * (3 - level))) & 0x1FF) as usize
}

impl PageTableMapper {
    /// Mapper over an existing PML4 frame (must be zeroed/valid).
    pub fn new(pml4_frame: u64) -> Self {
        Self { pml4_frame }
    }

    /// Map `virt → phys` with `flags` (flags are OR-ed into the entry).
    ///
    /// Creates intermediate tables lazily; leaf entries are created with
    /// `flags | PRESENT`. Intermediate entries get PRESENT | WRITABLE |
    /// USER so user pages are reachable.
    pub fn map_page(
        &mut self,
        mem: &mut dyn FrameMemory,
        virt: Virt,
        phys: Phys,
        flags: u64,
    ) -> Result<(), MapError> {
        let mut frame = self.pml4_frame;
        for level in 0..3 {
            // Read/decide first, mutate after — keeps the frame borrow short.
            let existing = {
                let table = mem.frame(frame).ok_or(MapError::FrameExhausted)?;
                table[index_at(virt, level)]
            };
            let next = if existing & PRESENT == 0 {
                let fresh = mem.alloc_frame().ok_or(MapError::FrameExhausted)?;
                let table = mem.frame_mut(frame).ok_or(MapError::FrameExhausted)?;
                table[index_at(virt, level)] = fresh | PRESENT | WRITABLE | USER;
                fresh
            } else {
                existing & !FLAGS_MASK
            };
            frame = next;
        }
        let table = mem.frame_mut(frame).ok_or(MapError::FrameExhausted)?;
        let slot = &mut table[index_at(virt, 3)];
        if *slot & PRESENT != 0 {
            return Err(MapError::AlreadyMapped);
        }
        *slot = phys.0 | flags | PRESENT;
        Ok(())
    }

    /// Translate `virt` to its physical frame base + entry flags.
    pub fn translate(&self, mem: &dyn FrameMemory, virt: Virt) -> Option<(Phys, u64)> {
        let mut frame = self.pml4_frame;
        for level in 0..4 {
            let table = mem.frame(frame)?;
            let entry = table[index_at(virt, level)];
            if entry & PRESENT == 0 {
                return None;
            }
            let next = entry & !FLAGS_MASK;
            if level == 3 {
                return Some((Phys(next), entry & FLAGS_MASK));
            }
            frame = next;
        }
        None
    }

    /// Unmap `virt` (clears the leaf entry). Returns the old physical
    /// frame base.
    pub fn unmap_page(&mut self, mem: &mut dyn FrameMemory, virt: Virt) -> Result<Phys, MapError> {
        let mut frame = self.pml4_frame;
        for level in 0..3 {
            let table = mem.frame_mut(frame).ok_or(MapError::NotMapped)?;
            let entry = table[index_at(virt, level)];
            if entry & PRESENT == 0 {
                return Err(MapError::NotMapped);
            }
            frame = entry & !FLAGS_MASK;
        }
        let table = mem.frame_mut(frame).ok_or(MapError::NotMapped)?;
        let slot = &mut table[index_at(virt, 3)];
        if *slot & PRESENT == 0 {
            return Err(MapError::NotMapped);
        }
        let phys = Phys(*slot & !FLAGS_MASK);
        *slot = 0;
        Ok(phys)
    }
}

/// Address of the page containing `virt`.
pub const fn page_of(virt: Virt) -> u64 {
    virt.0 & !(PAGE_SIZE as u64 - 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;
    use alloc::vec::Vec;

    /// In-memory frame store: frame 0..N, zero-initialized.
    struct RamFrames {
        frames: Vec<[u64; ENTRIES]>,
        next: u64,
    }

    impl RamFrames {
        fn new(count: u64) -> Self {
            Self {
                frames: vec![[0; ENTRIES]; count as usize],
                next: 0,
            }
        }
    }

    impl FrameMemory for RamFrames {
        fn alloc_frame(&mut self) -> Option<u64> {
            if self.next < self.frames.len() as u64 {
                let f = self.next;
                self.next += 1;
                self.frames[f as usize] = [0; ENTRIES];
                Some(f * PAGE_SIZE as u64)
            } else {
                None
            }
        }

        fn frame_mut(&mut self, frame: u64) -> Option<&mut [u64; ENTRIES]> {
            let index = (frame / PAGE_SIZE as u64) as usize;
            self.frames.get_mut(index)
        }

        fn frame(&self, frame: u64) -> Option<&[u64; ENTRIES]> {
            let index = (frame / PAGE_SIZE as u64) as usize;
            self.frames.get(index)
        }
    }

    #[test]
    fn map_and_translate_roundtrip() {
        let mut ram = RamFrames::new(64);
        let pml4 = ram.alloc_frame().unwrap();
        let mut mapper = PageTableMapper::new(pml4);
        mapper
            .map_page(&mut ram, Virt(0x0040_0000), Phys(0x5000), WRITABLE | USER)
            .expect("map");
        let (phys, flags) = mapper.translate(&ram, Virt(0x0040_0002)).expect("translate");
        assert_eq!(phys, Phys(0x5000));
        assert_eq!(flags & WRITABLE, WRITABLE);
        assert_eq!(flags & USER, USER);
    }

    #[test]
    fn remapping_rejected() {
        let mut ram = RamFrames::new(64);
        let mut mapper = PageTableMapper::new(ram.alloc_frame().unwrap());
        mapper.map_page(&mut ram, Virt(0x1000), Phys(0x2000), 0).unwrap();
        assert_eq!(
            mapper.map_page(&mut ram, Virt(0x1000), Phys(0x3000), 0),
            Err(MapError::AlreadyMapped)
        );
    }

    #[test]
    fn unmap_then_translate_is_none() {
        let mut ram = RamFrames::new(64);
        let mut mapper = PageTableMapper::new(ram.alloc_frame().unwrap());
        mapper.map_page(&mut ram, Virt(0x2000), Phys(0x9000), 0).unwrap();
        let removed = mapper.unmap_page(&mut ram, Virt(0x2000)).expect("unmap");
        assert_eq!(removed, Phys(0x9000));
        assert!(mapper.translate(&ram, Virt(0x2000)).is_none());
        assert_eq!(mapper.unmap_page(&mut ram, Virt(0x2000)), Err(MapError::NotMapped));
    }

    #[test]
    fn unmapped_translate_is_none() {
        let ram = RamFrames::new(64);
        let mapper = PageTableMapper::new(0);
        assert!(mapper.translate(&ram, Virt(0x0040_0000)).is_none());
    }

    #[test]
    fn tables_created_lazily_and_shared() {
        let mut ram = RamFrames::new(64);
        let mut mapper = PageTableMapper::new(ram.alloc_frame().unwrap());
        // Two pages in the same 2MiB region share PD/PT path tables.
        mapper.map_page(&mut ram, Virt(0x0040_0000), Phys(0x1_0000), 0).unwrap();
        let used_after_first = ram.next;
        assert!(used_after_first >= 4, "PML4+PDPT+PD+PT created");
        mapper.map_page(&mut ram, Virt(0x0040_1000), Phys(0x1_1000), 0).unwrap();
        assert_eq!(ram.next, used_after_first, "no new tables for the same region");
    }

    #[test]
    fn full_pt_boundary() {
        let mut ram = RamFrames::new(2048);
        let mut mapper = PageTableMapper::new(ram.alloc_frame().unwrap());
        // Fill 512 pages (one whole PT) — all must map and translate back.
        for i in 0..ENTRIES as u64 {
            let virt = Virt(0x0040_0000 + i * PAGE_SIZE as u64);
            mapper.map_page(&mut ram, virt, Phys(i * 0x1000 + 0x10_0000), WRITABLE).unwrap();
        }
        let (phys, _) = mapper.translate(&ram, Virt(0x0040_0000 + 511 * 0x1000)).unwrap();
        assert_eq!(phys, Phys(511 * 0x1000 + 0x10_0000));
    }

    #[test]
    fn page_of_aligns_down() {
        assert_eq!(page_of(Virt(0x1234)), 0x1000);
        assert_eq!(page_of(Virt(0xFFFF_F000_0000_0FFF)), 0xFFFF_F000_0000_0000);
    }
}
