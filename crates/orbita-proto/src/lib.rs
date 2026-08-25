#![no_std]

//! Boot handoff protocol between the UEFI entry stub and `kernel_main`.
//!
//! [`BootInfo`] carries everything the kernel needs after
//! `ExitBootServices`: the framebuffer description, the platform kind,
//! and the normalized physical memory map (bounded by
//! [`MAX_MEMORY_REGIONS`]).

use orbita_mm::{EMPTY_MEMORY_REGION, MemoryMapStatistics, MemoryRegion, summarize};
use orbita_video::FramebufferInfo;

pub const MAX_MEMORY_REGIONS: usize = 256;

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum PlatformKind {
    X86_64Uefi,
}

#[derive(Debug, Copy, Clone)]
pub struct BootInfo {
    pub platform: PlatformKind,
    pub framebuffer: FramebufferInfo,
    memory_regions: [MemoryRegion; MAX_MEMORY_REGIONS],
    memory_region_count: usize,
}

impl BootInfo {
    pub const fn new(platform: PlatformKind, framebuffer: FramebufferInfo) -> Self {
        Self {
            platform,
            framebuffer,
            memory_regions: [EMPTY_MEMORY_REGION; MAX_MEMORY_REGIONS],
            memory_region_count: 0,
        }
    }

    pub fn push_memory_region(&mut self, region: MemoryRegion) -> bool {
        if self.memory_region_count >= MAX_MEMORY_REGIONS {
            return false;
        }

        self.memory_regions[self.memory_region_count] = region;
        self.memory_region_count += 1;
        true
    }

    pub fn memory_regions(&self) -> &[MemoryRegion] {
        &self.memory_regions[..self.memory_region_count]
    }

    pub fn memory_statistics(&self) -> MemoryMapStatistics {
        summarize(self.memory_regions())
    }
}
