//! Virtual File System interfaces and manager.
//!
//! This module provides minimal VFS traits and a simple manager that can mount
//! different filesystem implementations. The implementation is intentionally
//! lightweight and many advanced features are left as TODO items.

use alloc::string::String;
use alloc::vec::Vec;
use alloc::sync::Arc;
use spin::RwLock;
use core::fmt;

/// File type enumeration used by VFS nodes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileType {
    Regular,
    Directory,
    Other,
}

/// File permissions structure.
#[derive(Debug, Clone, Copy)]
pub struct Permissions {
    pub read: bool,
    pub write: bool,
    pub execute: bool,
}

impl Permissions {
    /// Create default permissions (read/write).
    pub fn new() -> Self {
        Self { read: true, write: true, execute: false }
    }
}

/// Metadata for a filesystem node.
#[derive(Debug, Clone)]
pub struct Metadata {
    pub file_type: FileType,
    pub size: u64,
    pub permissions: Permissions,
}

/// Directory entry structure.
#[derive(Debug, Clone)]
pub struct DirEntry {
    pub name: String,
    pub inode: u64,
    pub file_type: FileType,
}

/// Error type for filesystem operations.
#[derive(Debug, Clone)]
pub enum FsError {
    NotFound,
    NotDirectory,
    AlreadyExists,
    IoError,
    InvalidArgument,
    Unsupported,
}

impl fmt::Display for FsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FsError::NotFound => write!(f, "not found"),
            FsError::NotDirectory => write!(f, "not a directory"),
            FsError::AlreadyExists => write!(f, "already exists"),
            FsError::IoError => write!(f, "i/o error"),
            FsError::InvalidArgument => write!(f, "invalid argument"),
            FsError::Unsupported => write!(f, "operation unsupported"),
        }
    }
}

/// Trait for file-like operations.
pub trait FileOps: Send + Sync {
    fn read(&self, offset: u64, buf: &mut [u8]) -> Result<usize, FsError>;
    fn write(&self, offset: u64, buf: &[u8]) -> Result<usize, FsError>;
    fn truncate(&self, size: u64) -> Result<(), FsError>;
    fn sync(&self) -> Result<(), FsError>;
}

/// Trait for directory operations.
pub trait DirOps: Send + Sync {
    fn readdir(&self) -> Result<Vec<DirEntry>, FsError>;
    fn lookup(&self, name: &str) -> Result<Arc<dyn VfsNode>, FsError>;
    fn create(&self, name: &str, perms: Permissions) -> Result<Arc<dyn VfsNode>, FsError>;
    fn mkdir(&self, name: &str, perms: Permissions) -> Result<Arc<dyn VfsNode>, FsError>;
    fn unlink(&self, name: &str) -> Result<(), FsError>;
}

/// VFS node trait. A node may represent a file or directory.
pub trait VfsNode: Send + Sync {
    fn node_type(&self) -> FileType;
    fn as_file(&self) -> Option<&dyn FileOps> { None }
    fn as_dir(&self) -> Option<&dyn DirOps> { None }
    fn metadata(&self) -> Result<Metadata, FsError>;
    fn set_metadata(&self, _metadata: &Metadata) -> Result<(), FsError> {
        Err(FsError::Unsupported)
    }
}

/// Filesystem level operations used by the VFS manager.
pub trait FilesystemOps: Send + Sync {
    fn root(&self) -> Arc<dyn VfsNode>;
    fn sync(&self) -> Result<(), FsError> { Ok(()) }
    fn unmount(&self) -> Result<(), FsError> { Ok(()) }
}

/// Block device abstraction used by filesystems.
pub trait BlockDevice: Send + Sync {
    fn read_blocks(&self, lba: u64, buf: &mut [u8]) -> Result<(), FsError>;
    fn write_blocks(&self, lba: u64, buf: &[u8]) -> Result<(), FsError>;
    fn sector_size(&self) -> usize { 512 }
}

struct MountPoint {
    path: String,
    fs: Arc<dyn FilesystemOps>,
}

/// Simple VFS manager.
pub struct Vfs {
    root: Arc<dyn VfsNode>,
    mounts: RwLock<Vec<MountPoint>>,
}

impl Vfs {
    /// Create new VFS instance from root filesystem.
    pub fn new(root: Arc<dyn FilesystemOps>) -> Self {
        Self { root: root.root(), mounts: RwLock::new(Vec::new()) }
    }

    /// Mount filesystem at path.
    pub fn mount(&self, path: &str, fs: Arc<dyn FilesystemOps>) -> Result<(), FsError> {
        let node = self.lookup(path)?;
        if node.node_type() != FileType::Directory {
            return Err(FsError::NotDirectory);
        }
        let mut mounts = self.mounts.write();
        mounts.push(MountPoint { path: path.to_string(), fs });
        Ok(())
    }

    /// Lookup path starting from root.
    pub fn lookup(&self, path: &str) -> Result<Arc<dyn VfsNode>, FsError> {
        if !path.starts_with('/') {
            return Err(FsError::InvalidArgument);
        }
        let mut current = self.root.clone();
        if path == "/" {
            return Ok(current);
        }
        for part in path.trim_start_matches('/').split('/') {
            let dir = current.as_dir().ok_or(FsError::NotDirectory)?;
            current = dir.lookup(part)?;
        }
        Ok(current)
    }
}

