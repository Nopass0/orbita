//! Virtual-memory region bookkeeping for processes.
//!
//! v1 model: the kernel runs identity-mapped (no hardware paging yet —
//! that is the `user-mode` roadmap milestone), so a [`RegionMap`] tracks
//! *logical* mappings: which byte ranges belong to which process, with
//! which permissions, and what backs them. The ELF loader records image
//! segments here, and the ABI `mem` calls record application allocations;
//! `ps`/diagnostics can then attribute memory per process.
//!
//! When hardware paging arrives, this map becomes the source of truth
//! that page tables are built from — the API already matches
//! (map/unmap/protect, anonymous + shared backing).

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::vec::Vec;

/// Permissions of a mapped region.
#[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct Protection {
    pub read: bool,
    pub write: bool,
    pub execute: bool,
}

impl Protection {
    /// Read/write data region.
    pub const RW: Self = Self { read: true, write: true, execute: false };
    /// Read-only data region.
    pub const RO: Self = Self { read: true, write: false, execute: false };
    /// Executable code region.
    pub const RX: Self = Self { read: true, write: false, execute: true };
}

/// What backs a mapped region.
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum Backing {
    /// Allocated from the kernel heap (v1 anonymous memory).
    Heap { size: usize },
    /// A loaded application image segment (ELF PT_LOAD).
    Image { offset: u64, size: usize },
    /// Shared memory: same `key` in two maps = same storage (IPC).
    Shared { key: u64, size: usize },
}

/// One mapped region.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct VmRegion {
    pub start: u64,
    pub size: usize,
    pub protection: Protection,
    pub backing: Backing,
}

impl VmRegion {
    /// End address (exclusive).
    pub const fn end(&self) -> u64 {
        self.start + self.size as u64
    }

    /// Whether this region covers `address`.
    pub const fn covers(&self, address: u64) -> bool {
        address >= self.start && address < self.end()
    }
}

/// Region map of one process, keyed by start address.
#[derive(Debug, Default)]
pub struct RegionMap {
    regions: BTreeMap<u64, VmRegion>,
    next_shared_key: u64,
}

impl RegionMap {
    pub fn new() -> Self {
        Self {
            regions: BTreeMap::new(),
            next_shared_key: 1,
        }
    }

    /// Map a region at `start` (fails on overlap with an existing one).
    pub fn map(&mut self, region: VmRegion) -> Result<(), MapError> {
        if region.size == 0 {
            return Err(MapError::ZeroSize);
        }
        if let Some(existing) = self.regions.range(..region.end()).next_back() {
            if existing.1.end() > region.start {
                return Err(MapError::Overlap);
            }
        }
        self.regions.insert(region.start, region);
        Ok(())
    }

    /// Unmap the region containing `address`; returns it if found.
    pub fn unmap_at(&mut self, address: u64) -> Option<VmRegion> {
        let start = self.find(address)?.start;
        self.regions.remove(&start)
    }

    /// Change protection of the region containing `address`.
    pub fn protect_at(&mut self, address: u64, protection: Protection) -> Result<(), MapError> {
        let start = self.find(address).ok_or(MapError::NotFound)?.start;
        if let Some(region) = self.regions.get_mut(&start) {
            region.protection = protection;
        }
        Ok(())
    }

    /// The region containing `address`, if any.
    pub fn find(&self, address: u64) -> Option<&VmRegion> {
        self.regions.range(..=address).next_back().filter(|(_, r)| r.covers(address)).map(|(_, r)| r)
    }

    /// All regions (ordered by address).
    pub fn regions(&self) -> impl Iterator<Item = &VmRegion> {
        self.regions.values()
    }

    /// Total bytes mapped.
    pub fn total_bytes(&self) -> usize {
        self.regions.values().map(|r| r.size).sum()
    }

    /// Allocate a fresh shared-memory key for IPC mapping.
    pub fn next_shared_key(&mut self) -> u64 {
        let key = self.next_shared_key;
        self.next_shared_key += 1;
        key
    }
}

/// Region mapping failures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MapError {
    /// Regions must be non-empty.
    ZeroSize,
    /// The range intersects an existing mapping.
    Overlap,
    /// No region at the given address.
    NotFound,
}

/// Registry of shared-memory keys to backing storage (kernel-side).
#[derive(Debug, Default)]
pub struct SharedMemoryRegistry {
    blocks: BTreeMap<u64, Vec<u8>>,
}

impl SharedMemoryRegistry {
    pub fn new() -> Self {
        Self { blocks: BTreeMap::new() }
    }

    /// Create a shared block of `size` bytes (zero-initialized).
    pub fn create(&mut self, key: u64, size: usize) -> Result<(), MapError> {
        if size == 0 {
            return Err(MapError::ZeroSize);
        }
        if self.blocks.contains_key(&key) {
            return Err(MapError::Overlap);
        }
        self.blocks.insert(key, alloc::vec![0u8; size]);
        Ok(())
    }

    /// Borrow a shared block for reading/writing.
    pub fn get(&mut self, key: u64) -> Option<&mut [u8]> {
        self.blocks.get_mut(&key).map(|v| v.as_mut_slice())
    }

    /// Remove a shared block.
    pub fn destroy(&mut self, key: u64) -> bool {
        self.blocks.remove(&key).is_some()
    }

    /// Number of live shared blocks.
    pub fn len(&self) -> usize {
        self.blocks.len()
    }

    /// Whether no shared block exists.
    pub fn is_empty(&self) -> bool {
        self.blocks.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn map_find_unmap_lifecycle() {
        let mut map = RegionMap::new();
        map.map(VmRegion {
            start: 0x1000,
            size: 0x100,
            protection: Protection::RW,
            backing: Backing::Heap { size: 0x100 },
        })
        .expect("map");

        assert!(map.find(0x1080).is_some());
        assert!(map.find(0x1100).is_none());
        assert_eq!(map.total_bytes(), 0x100);

        let removed = map.unmap_at(0x10ff).expect("unmap");
        assert_eq!(removed.start, 0x1000);
        assert!(map.find(0x1080).is_none());
    }

    #[test]
    fn overlapping_map_rejected() {
        let mut map = RegionMap::new();
        map.map(VmRegion {
            start: 0x1000,
            size: 0x100,
            protection: Protection::RO,
            backing: Backing::Heap { size: 0x100 },
        })
        .expect("map");
        let conflict = map.map(VmRegion {
            start: 0x10ff,
            size: 0x10,
            protection: Protection::RW,
            backing: Backing::Heap { size: 0x10 },
        });
        assert_eq!(conflict, Err(MapError::Overlap));
    }

    #[test]
    fn shared_registry_roundtrip() {
        let mut registry = SharedMemoryRegistry::new();
        registry.create(7, 16).expect("create");
        let block = registry.get(7).expect("get");
        block[..4].copy_from_slice(&1u32.to_le_bytes());
        assert_eq!(&registry.get(7).unwrap()[..4], &1u32.to_le_bytes());
        assert!(registry.destroy(7));
        assert!(registry.get(7).is_none());
    }
}
