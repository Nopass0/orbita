/// Logical block number within a filesystem volume.
#[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct BlockAddress(pub u64);

impl BlockAddress {
    /// Converts a block address into a byte offset.
    pub fn to_bytes(self, block_size: BlockSize) -> u64 {
        self.0 * block_size.0 as u64
    }
}

/// Bytes per block.
#[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct BlockSize(pub u32);

impl BlockSize {
    /// Returns the block size as a byte count.
    pub fn as_u64(self) -> u64 {
        self.0 as u64
    }
}

/// Identifier for a mounted volume.
#[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct VolumeId(pub u128);

/// Features that a filesystem implementation can advertise.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum FsFeature {
    Journaling,
    CopyOnWrite,
    Extents,
    Compression,
    Checksums,
    AtomicRename,
    DedupeHooks,
}

impl FsFeature {
    /// Stable feature label used by diagnostics and docs.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Journaling => "journaling",
            Self::CopyOnWrite => "copy_on_write",
            Self::Extents => "extents",
            Self::Compression => "compression",
            Self::Checksums => "checksums",
            Self::AtomicRename => "atomic_rename",
            Self::DedupeHooks => "dedupe_hooks",
        }
    }
}

/// Static capability description for a mounted filesystem.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct FsCapabilities {
    pub block_size: BlockSize,
    pub features: &'static [FsFeature],
}

impl FsCapabilities {
    /// Returns true when the capability set includes the requested feature.
    pub fn supports(&self, feature: FsFeature) -> bool {
        self.features.iter().copied().any(|known| known == feature)
    }
}

/// How much capacity should be reserved in a transaction.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct SpaceReservation {
    pub data_blocks: u64,
    pub metadata_blocks: u64,
}

impl SpaceReservation {
    /// Returns the total reservation in blocks.
    pub fn total_blocks(&self) -> u64 {
        self.data_blocks + self.metadata_blocks
    }
}

/// Partition layout hints for the kernel or installer.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct FsPartition {
    pub volume: VolumeId,
    pub superblock: BlockAddress,
    pub inode_table: BlockAddress,
    pub journal_start: BlockAddress,
}

/// High-level storage contract used by the filesystem layer.
pub trait BlockAllocator {
    fn block_size(&self) -> BlockSize;
    fn reserve(&mut self, blocks: u64) -> Option<BlockAddress>;
    fn release(&mut self, start: BlockAddress, blocks: u64);
}

/// Layout description for an Orbita FS volume.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct FsLayout {
    pub partition: FsPartition,
    pub capacity_blocks: u64,
    pub reserved: SpaceReservation,
    pub capabilities: FsCapabilities,
}

impl FsLayout {
    /// Returns the theoretical capacity in bytes.
    pub fn capacity_bytes(&self) -> u64 {
        self.capacity_blocks * self.capabilities.block_size.as_u64()
    }

    /// Returns the reserved bytes for allocator and metadata regions.
    pub fn reserved_bytes(&self) -> u64 {
        self.reserved.total_blocks() * self.capabilities.block_size.as_u64()
    }

    /// Returns the estimated usable blocks after static reservations.
    pub fn usable_blocks(&self) -> u64 {
        self.capacity_blocks.saturating_sub(self.reserved.total_blocks())
    }
}
