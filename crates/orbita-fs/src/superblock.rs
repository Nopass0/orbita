use crate::{BlockSize, layout::VolumeId};

/// On-disk flags for the filesystem volume.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct SuperblockFlags(pub u64);

impl SuperblockFlags {
    pub const CLEAN: Self = Self(1 << 0);
    pub const JOURNAL_PRESENT: Self = Self(1 << 1);
    pub const COW_ENABLED: Self = Self(1 << 2);
    pub const CHECKSUMS_ENABLED: Self = Self(1 << 3);
    pub const COMPRESSION_ENABLED: Self = Self(1 << 4);
}

/// Policy selected for metadata and payload checksums.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum FsChecksumPolicy {
    None,
    MetadataOnly,
    MetadataAndData,
}

/// Policy selected for compression.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum FsCompressionPolicy {
    None,
    Adaptive,
    Forced,
}

/// The root on-disk metadata block for Orbita FS.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct Superblock {
    pub magic: [u8; 8],
    pub version_major: u16,
    pub version_minor: u16,
    pub block_size: BlockSize,
    pub volume_id: VolumeId,
    pub flags: SuperblockFlags,
    pub checksum_policy: FsChecksumPolicy,
    pub compression_policy: FsCompressionPolicy,
    pub root_inode: u64,
    pub journal_inode: u64,
    pub free_space_root: u64,
    pub features_crc: u32,
}

impl Superblock {
    /// Returns true when the volume is marked clean.
    pub fn is_clean(&self) -> bool {
        (self.flags.0 & SuperblockFlags::CLEAN.0) != 0
    }

    /// Returns true when the superblock advertises a specific flag.
    pub fn has_flag(&self, flag: SuperblockFlags) -> bool {
        (self.flags.0 & flag.0) != 0
    }
}
