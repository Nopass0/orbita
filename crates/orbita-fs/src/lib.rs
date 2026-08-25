#![no_std]

extern crate alloc;

// Orbita FS, also referred to as NebulaFS in higher-level docs.
//
// This crate defines the core filesystem contracts and on-disk metadata
// model. It does not talk to a specific block device yet; that integration
// belongs to a storage backend layer.

mod checksum;
mod compression;
mod device;
pub mod diskfs;
pub mod fat;
pub mod fat_writer;
mod directory;
mod extent;
mod inode;
mod journal;
mod layout;
mod object;
mod mount;
mod memory;
mod superblock;
mod runtime;
mod volume;

pub use checksum::{ChecksumAlgorithm, ChecksumDigest, ChecksumHook};
pub use compression::{CompressionAlgorithm, CompressionHook, CompressionLevel};
pub use device::{
    BlockDevice, BlockDeviceError, BlockDeviceGeometry, BlockDeviceInfo, BlockDeviceStats,
    BlockRequest, BlockRequestKind, BlockResponse,
};
pub use directory::{DirectoryEntry, DirectoryIndex, DirectoryKey, DirectoryRecord};
pub use extent::{Extent, ExtentFlags, ExtentKey, ExtentNode, ExtentTree, FileOffset};
pub use inode::{
    FileMode, FileType, Inode, InodeFlags, InodeId, InodeKind, InodeMetadata, InodePermissions,
};
pub use journal::{
    JournalAction, JournalCommit, JournalEntry, JournalKind, JournalPolicy, JournalReplay,
    JournalReplayError, JournalReplayProgress, JournalReplayState, TransactionId,
};
pub use layout::{
    BlockAddress, BlockAllocator, BlockSize, FsCapabilities, FsFeature, FsLayout, FsPartition,
    SpaceReservation,
};
pub use mount::{FsMountDescriptor, MountedVolumeState};
pub use memory::MemoryVolume;
pub use object::{FsObject, FsObjectHandle, ObjectAttributes, ObjectKind};
pub use superblock::{FsChecksumPolicy, FsCompressionPolicy, Superblock, SuperblockFlags};
pub use layout::VolumeId;
pub use runtime::{FilesystemRuntime, RuntimeMountError};
pub use volume::{
    DirectoryCreateOptions, DirectoryCursor, DirectoryHandle, DirectoryListing,
    DirectoryListingEntry, DirectoryOpenOptions, FileCreateOptions, FileHandle, FileOpenOptions,
    FilesystemVolume, FsError, OpenDirectoryHandle, OpenFileHandle, ReadResult, SyncReport,
    VolumeFormatError, VolumeFormatReport, VolumeFormatRequest, VolumeHandle, VolumeInfo,
    VolumeInspector, VolumeSpaceStats, VolumeStatistics, VolumeFormatter, WriteResult,
};
