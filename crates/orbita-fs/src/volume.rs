use alloc::{string::String, vec::Vec};

use crate::{
    BlockDeviceError, BlockDeviceGeometry, BlockSize, DirectoryRecord, FileMode, FileType,
    FsCapabilities, FsChecksumPolicy, FsCompressionPolicy, FsFeature, FsLayout, FsObjectHandle,
    FsPartition, InodeId, InodeMetadata, InodePermissions, JournalPolicy, JournalReplayError,
    Superblock, SuperblockFlags, TransactionId, VolumeId,
};

/// Coarse volume space counters returned by the filesystem.
///
/// The numbers are expressed in blocks, with helpers for converting to bytes
/// using the mounted block size.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct VolumeSpaceStats {
    pub block_size: BlockSize,
    pub total_blocks: u64,
    pub free_blocks: u64,
    pub allocated_blocks: u64,
    pub reserved_blocks: u64,
    pub metadata_blocks: u64,
    pub data_blocks: u64,
    pub dirty_blocks: u64,
}

impl Default for VolumeSpaceStats {
    fn default() -> Self {
        Self {
            block_size: BlockSize(0),
            total_blocks: 0,
            free_blocks: 0,
            allocated_blocks: 0,
            reserved_blocks: 0,
            metadata_blocks: 0,
            data_blocks: 0,
            dirty_blocks: 0,
        }
    }
}

impl VolumeSpaceStats {
    /// Returns the total capacity in bytes.
    pub fn total_bytes(&self) -> u64 {
        self.block_size.0 as u64 * self.total_blocks
    }

    /// Returns the current free space in bytes.
    pub fn free_bytes(&self) -> u64 {
        self.block_size.0 as u64 * self.free_blocks
    }

    /// Returns the already allocated space in bytes.
    pub fn allocated_bytes(&self) -> u64 {
        self.block_size.0 as u64 * self.allocated_blocks
    }

    /// Returns the amount reserved for the allocator or transaction system.
    pub fn reserved_bytes(&self) -> u64 {
        self.block_size.0 as u64 * self.reserved_blocks
    }

    /// Returns the amount of space that can be consumed by new data.
    pub fn available_bytes(&self) -> u64 {
        self.block_size.0 as u64 * self.free_blocks.saturating_sub(self.reserved_blocks)
    }

    /// Returns a simple usage percentage.
    pub fn used_percent(&self) -> u8 {
        if self.total_blocks == 0 {
            return 0;
        }

        let used = self.allocated_blocks.saturating_add(self.reserved_blocks);
        let ratio = used.saturating_mul(100) / self.total_blocks;
        ratio.min(100) as u8
    }

    /// Creates a space snapshot from the block-device geometry and layout hints.
    pub fn from_layout(geometry: BlockDeviceGeometry, layout: FsLayout) -> Self {
        let total_blocks = geometry.block_count;
        let reserved_blocks = layout.reserved.total_blocks();
        let allocated_blocks = reserved_blocks.min(total_blocks);
        let free_blocks = total_blocks.saturating_sub(allocated_blocks);

        Self {
            block_size: geometry.block_size,
            total_blocks,
            free_blocks,
            allocated_blocks,
            reserved_blocks,
            metadata_blocks: layout.reserved.metadata_blocks,
            data_blocks: layout.reserved.data_blocks,
            dirty_blocks: 0,
        }
    }
}

/// Global counters for a mounted filesystem instance.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct VolumeStatistics {
    pub volume: VolumeId,
    pub mounted: bool,
    pub readonly: bool,
    pub clean: bool,
    pub files: u64,
    pub directories: u64,
    pub symlinks: u64,
    pub special_nodes: u64,
    pub inodes: u64,
    pub extents: u64,
    pub tx_count: u64,
    pub last_checkpoint_tx: Option<TransactionId>,
    pub space: VolumeSpaceStats,
}

impl Default for VolumeStatistics {
    fn default() -> Self {
        Self {
            volume: VolumeId(0),
            mounted: false,
            readonly: false,
            clean: false,
            files: 0,
            directories: 0,
            symlinks: 0,
            special_nodes: 0,
            inodes: 0,
            extents: 0,
            tx_count: 0,
            last_checkpoint_tx: None,
            space: VolumeSpaceStats::default(),
        }
    }
}

/// Stable handle kind for the high-level API.
#[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct FileHandle(pub FsObjectHandle);

/// Stable directory handle kind for the high-level API.
#[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct DirectoryHandle(pub FsObjectHandle);

/// Stable volume handle kind for the high-level API.
#[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct VolumeHandle(pub FsObjectHandle);

/// Cursor used to page through large directory listings.
#[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Default)]
pub struct DirectoryCursor(pub u64);

/// Result of a read operation.
#[derive(Debug, Copy, Clone, Eq, PartialEq, Default)]
pub struct ReadResult {
    pub bytes_read: usize,
    pub end_of_file: bool,
}

/// Result of a write operation.
#[derive(Debug, Copy, Clone, Eq, PartialEq, Default)]
pub struct WriteResult {
    pub bytes_written: usize,
    pub grew: bool,
}

/// Result of a sync or flush operation.
#[derive(Debug, Copy, Clone, Eq, PartialEq, Default)]
pub struct SyncReport {
    pub flushed_metadata: bool,
    pub flushed_data: bool,
    pub committed_transactions: u64,
}

/// Directory entry returned by the high-level listing API.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct DirectoryListingEntry {
    pub name: String,
    pub inode: InodeId,
    pub record: DirectoryRecord,
    pub metadata: InodeMetadata,
}

/// Snapshot of a directory listing.
#[derive(Debug, Clone, Eq, PartialEq, Default)]
pub struct DirectoryListing {
    pub entries: Vec<DirectoryListingEntry>,
    pub next_cursor: Option<DirectoryCursor>,
    pub total_entries: Option<u64>,
}

/// Common directory creation flags.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct DirectoryCreateOptions {
    pub permissions: InodePermissions,
    pub mode: FileMode,
    pub exclusive: bool,
    pub create_parents: bool,
}

impl Default for DirectoryCreateOptions {
    fn default() -> Self {
        Self {
            permissions: InodePermissions(0o755),
            mode: FileMode(0o040755),
            exclusive: false,
            create_parents: false,
        }
    }
}

/// Common file creation flags.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct FileCreateOptions {
    pub permissions: InodePermissions,
    pub mode: FileMode,
    pub file_type: FileType,
    pub exclusive: bool,
    pub create_parents: bool,
    pub sparse: bool,
    pub inline_data_hint: bool,
}

impl Default for FileCreateOptions {
    fn default() -> Self {
        Self {
            permissions: InodePermissions(0o644),
            mode: FileMode(0o100644),
            file_type: FileType::Regular,
            exclusive: false,
            create_parents: false,
            sparse: false,
            inline_data_hint: false,
        }
    }
}

/// Common open flags for regular files.
#[derive(Debug, Copy, Clone, Eq, PartialEq, Default)]
pub struct FileOpenOptions {
    pub read: bool,
    pub write: bool,
    pub append: bool,
    pub create: bool,
    pub create_new: bool,
    pub truncate: bool,
    pub direct_io: bool,
    pub sync_on_close: bool,
}

impl FileOpenOptions {
    /// Returns a read-only file open profile.
    pub fn read_only() -> Self {
        Self {
            read: true,
            ..Self::default()
        }
    }

    /// Returns a read-write file open profile.
    pub fn read_write() -> Self {
        Self {
            read: true,
            write: true,
            ..Self::default()
        }
    }
}

/// Common open flags for directories.
#[derive(Debug, Copy, Clone, Eq, PartialEq, Default)]
pub struct DirectoryOpenOptions {
    pub read: bool,
    pub create: bool,
    pub create_new: bool,
    pub follow_symlinks: bool,
}

/// Mount-time and format-time view of a volume.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct VolumeInfo {
    pub volume: VolumeId,
    pub geometry: BlockDeviceGeometry,
    pub superblock: Superblock,
    pub layout: FsLayout,
    pub capabilities: FsCapabilities,
    pub space: VolumeSpaceStats,
    pub statistics: VolumeStatistics,
}

/// Volume inspection contract.
pub trait VolumeInspector {
    fn volume_id(&self) -> VolumeId;
    fn geometry(&self) -> BlockDeviceGeometry;
    fn superblock(&self) -> Superblock;
    fn layout(&self) -> FsLayout;
    fn capabilities(&self) -> FsCapabilities;
    fn space_stats(&self) -> VolumeSpaceStats;
    fn volume_stats(&self) -> VolumeStatistics;
}

/// Error type used by the high-level filesystem API.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum FsError {
    NotFound,
    AlreadyExists,
    NotDirectory,
    IsDirectory,
    PermissionDenied,
    ReadOnly,
    NoSpace,
    NameTooLong,
    InvalidName,
    InvalidPath,
    InvalidHandle,
    NotEmpty,
    CrossDeviceLink,
    Busy,
    WouldBlock,
    Unsupported,
    Corrupted,
    Device(BlockDeviceError),
    Journal(JournalReplayError),
}

impl From<BlockDeviceError> for FsError {
    fn from(value: BlockDeviceError) -> Self {
        Self::Device(value)
    }
}

impl From<JournalReplayError> for FsError {
    fn from(value: JournalReplayError) -> Self {
        Self::Journal(value)
    }
}

/// Formatting error for a brand-new or rewritten volume.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum VolumeFormatError {
    Device(BlockDeviceError),
    InvalidLayout,
    UnsupportedFeature,
    TooSmall,
    AlreadyFormatted,
    LabelTooLong,
    CorruptedExistingData,
}

impl From<BlockDeviceError> for VolumeFormatError {
    fn from(value: BlockDeviceError) -> Self {
        Self::Device(value)
    }
}

/// Input contract for formatting a filesystem volume.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct VolumeFormatRequest<'a> {
    pub volume: VolumeId,
    pub geometry: BlockDeviceGeometry,
    pub layout: FsLayout,
    pub checksum_policy: FsChecksumPolicy,
    pub compression_policy: FsCompressionPolicy,
    pub journal_policy: JournalPolicy,
    pub superblock_flags: SuperblockFlags,
    pub root_permissions: InodePermissions,
    pub root_mode: FileMode,
    pub features: &'a [FsFeature],
    pub label: Option<&'a str>,
    pub wipe_existing: bool,
    pub create_journal: bool,
}

impl<'a> VolumeFormatRequest<'a> {
    /// Creates a conservative format request from a layout description.
    pub fn new(volume: VolumeId, geometry: BlockDeviceGeometry, layout: FsLayout) -> Self {
        Self {
            volume,
            geometry,
            layout,
            checksum_policy: FsChecksumPolicy::MetadataAndData,
            compression_policy: FsCompressionPolicy::Adaptive,
            journal_policy: JournalPolicy::Hybrid,
            superblock_flags: SuperblockFlags::CLEAN,
            root_permissions: InodePermissions(0o755),
            root_mode: FileMode(0o040755),
            features: layout.capabilities.features,
            label: None,
            wipe_existing: false,
            create_journal: true,
        }
    }

    /// Adds a label to the format request.
    pub fn with_label(mut self, label: &'a str) -> Self {
        self.label = Some(label);
        self
    }

    /// Overrides the advertised feature list for the new volume.
    pub fn with_features(mut self, features: &'a [FsFeature]) -> Self {
        self.features = features;
        self
    }
}

/// Report returned once formatting has completed.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct VolumeFormatReport {
    pub volume: VolumeId,
    pub geometry: BlockDeviceGeometry,
    pub superblock: Superblock,
    pub partition: FsPartition,
    pub space: VolumeSpaceStats,
    pub root_inode: InodeId,
    pub journal_inode: Option<InodeId>,
    pub free_space_root: InodeId,
    pub features_written: u32,
    pub blocks_written: u64,
}

/// Contract for a backend that can format a volume.
pub trait VolumeFormatter {
    fn format_volume(
        &mut self,
        request: VolumeFormatRequest<'_>,
    ) -> Result<VolumeFormatReport, VolumeFormatError>;
}

/// A mounted file or directory handle that can be tracked by the VFS.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct OpenFileHandle {
    pub handle: FileHandle,
    pub inode: InodeId,
    pub metadata: InodeMetadata,
    pub options: FileOpenOptions,
}

/// A mounted directory handle that can be tracked by the VFS.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct OpenDirectoryHandle {
    pub handle: DirectoryHandle,
    pub inode: InodeId,
    pub metadata: InodeMetadata,
    pub options: DirectoryOpenOptions,
}

/// High-level filesystem operations exposed to the kernel or userspace VFS.
pub trait FilesystemVolume: VolumeInspector {
    fn root_directory(&self) -> Result<OpenDirectoryHandle, FsError>;

    fn create_file(
        &mut self,
        parent: DirectoryHandle,
        name: &str,
        options: FileCreateOptions,
    ) -> Result<OpenFileHandle, FsError>;

    fn create_directory(
        &mut self,
        parent: DirectoryHandle,
        name: &str,
        options: DirectoryCreateOptions,
    ) -> Result<OpenDirectoryHandle, FsError>;

    fn open_file(
        &mut self,
        parent: DirectoryHandle,
        name: &str,
        options: FileOpenOptions,
    ) -> Result<OpenFileHandle, FsError>;

    fn open_directory(
        &mut self,
        parent: DirectoryHandle,
        name: &str,
        options: DirectoryOpenOptions,
    ) -> Result<OpenDirectoryHandle, FsError>;

    fn list_directory(
        &self,
        directory: DirectoryHandle,
        cursor: DirectoryCursor,
        limit: usize,
    ) -> Result<DirectoryListing, FsError>;

    fn read_file(
        &mut self,
        file: FileHandle,
        offset: u64,
        dst: &mut [u8],
    ) -> Result<ReadResult, FsError>;

    fn write_file(
        &mut self,
        file: FileHandle,
        offset: u64,
        src: &[u8],
    ) -> Result<WriteResult, FsError>;

    fn truncate_file(&mut self, file: FileHandle, size: u64) -> Result<(), FsError>;

    fn remove_entry(&mut self, parent: DirectoryHandle, name: &str) -> Result<(), FsError>;

    fn rename_entry(
        &mut self,
        old_parent: DirectoryHandle,
        old_name: &str,
        new_parent: DirectoryHandle,
        new_name: &str,
    ) -> Result<(), FsError>;

    fn flush_file(&mut self, file: FileHandle) -> Result<(), FsError>;

    fn sync_volume(&mut self) -> Result<SyncReport, FsError>;
}

impl DirectoryListingEntry {
    /// Builds a listing entry from an inode record and directory payload.
    pub fn new(name: String, inode: InodeId, record: DirectoryRecord, metadata: InodeMetadata) -> Self {
        Self {
            name,
            inode,
            record,
            metadata,
        }
    }
}

impl DirectoryListing {
    /// Returns an empty directory snapshot.
    pub fn empty() -> Self {
        Self::default()
    }

    /// Appends one entry to the snapshot.
    pub fn push(&mut self, entry: DirectoryListingEntry) {
        self.entries.push(entry);
    }
}
