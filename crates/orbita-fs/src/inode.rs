use crate::{ExtentTree, FileOffset};

/// Identifier of an inode on disk.
#[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct InodeId(pub u64);

/// A broad file type classification.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum FileType {
    Regular,
    Directory,
    Symlink,
    Device,
    Socket,
    Pipe,
}

/// Lower-level inode kind, kept distinct from user-facing file type.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum InodeKind {
    File,
    Directory,
    InlineData,
    Special,
}

/// Permission model used by the filesystem metadata.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct InodePermissions(pub u16);

/// File mode metadata.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct FileMode(pub u32);

/// Flags stored in the inode record.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct InodeFlags(pub u32);

impl InodeFlags {
    /// Returns true if the inode contains the requested flag.
    pub fn contains(self, flag: InodeFlags) -> bool {
        (self.0 & flag.0) != 0
    }
}

impl InodeFlags {
    pub const IMMUTABLE: Self = Self(1 << 0);
    pub const APPEND_ONLY: Self = Self(1 << 1);
    pub const IMMORTAL: Self = Self(1 << 2);
    pub const INLINE_DATA: Self = Self(1 << 3);
    pub const COW_ROOT: Self = Self(1 << 4);
}

/// Metadata that can be queried without reading the full inode payload.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct InodeMetadata {
    pub inode: InodeId,
    pub kind: InodeKind,
    pub file_type: FileType,
    pub permissions: InodePermissions,
    pub mode: FileMode,
    pub size_bytes: u64,
    pub blocks: u64,
    pub generation: u64,
}

impl InodeMetadata {
    /// Returns true when the inode describes a directory.
    pub fn is_directory(&self) -> bool {
        self.kind == InodeKind::Directory || self.file_type == FileType::Directory
    }

    /// Returns true when the inode describes a regular file.
    pub fn is_file(&self) -> bool {
        self.kind == InodeKind::File || self.file_type == FileType::Regular
    }
}

/// The on-disk inode object.
#[derive(Debug, Clone)]
pub struct Inode {
    pub meta: InodeMetadata,
    pub flags: InodeFlags,
    pub extents: ExtentTree,
    pub parent: Option<InodeId>,
    pub checksum: Option<u64>,
    pub inline_data_len: u32,
    pub inline_data_offset: Option<FileOffset>,
}
