# Filesystem Implementation Guide

## Overview
The filesystem module provides virtual filesystem (VFS) abstraction and FAT32 implementation for Orbita OS, enabling file and directory operations.

## Module Structure

### 1. Virtual Filesystem (vfs.rs)

```rust
//! Virtual Filesystem (VFS) layer
//! Provides a unified interface for different filesystem implementations

use alloc::string::String;
use alloc::vec::Vec;
use alloc::sync::Arc;
use spin::RwLock;
use core::fmt;

/// File types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileType {
    Regular,
    Directory,
    CharDevice,
    BlockDevice,
    Symlink,
    Socket,
    Pipe,
}

/// File permissions
#[derive(Debug, Clone, Copy)]
pub struct Permissions {
    pub owner_read: bool,
    pub owner_write: bool,
    pub owner_execute: bool,
    pub group_read: bool,
    pub group_write: bool,
    pub group_execute: bool,
    pub other_read: bool,
    pub other_write: bool,
    pub other_execute: bool,
}

impl Permissions {
    /// Create new permissions with default values
    pub fn new() -> Self {
        Self {
            owner_read: true,
            owner_write: true,
            owner_execute: false,
            group_read: true,
            group_write: false,
            group_execute: false,
            other_read: true,
            other_write: false,
            other_execute: false,
        }
    }
    
    /// Create permissions from Unix mode
    pub fn from_mode(mode: u16) -> Self {
        Self {
            owner_read: mode & 0o400 != 0,
            owner_write: mode & 0o200 != 0,
            owner_execute: mode & 0o100 != 0,
            group_read: mode & 0o040 != 0,
            group_write: mode & 0o020 != 0,
            group_execute: mode & 0o010 != 0,
            other_read: mode & 0o004 != 0,
            other_write: mode & 0o002 != 0,
            other_execute: mode & 0o001 != 0,
        }
    }
    
    /// Convert to Unix mode
    pub fn to_mode(&self) -> u16 {
        let mut mode = 0;
        if self.owner_read { mode |= 0o400; }
        if self.owner_write { mode |= 0o200; }
        if self.owner_execute { mode |= 0o100; }
        if self.group_read { mode |= 0o040; }
        if self.group_write { mode |= 0o020; }
        if self.group_execute { mode |= 0o010; }
        if self.other_read { mode |= 0o004; }
        if self.other_write { mode |= 0o002; }
        if self.other_execute { mode |= 0o001; }
        mode
    }
}

/// File metadata
#[derive(Debug, Clone)]
pub struct Metadata {
    pub file_type: FileType,
    pub size: u64,
    pub permissions: Permissions,
    pub created: u64,
    pub modified: u64,
    pub accessed: u64,
    pub uid: u32,
    pub gid: u32,
    pub nlinks: u32,
}

/// Directory entry
#[derive(Debug, Clone)]
pub struct DirEntry {
    pub name: String,
    pub inode: u64,
    pub file_type: FileType,
}

/// File operations trait
pub trait FileOps: Send + Sync {
    /// Read data from file
    fn read(&self, offset: u64, buffer: &mut [u8]) -> Result<usize, FsError>;
    
    /// Write data to file
    fn write(&self, offset: u64, buffer: &[u8]) -> Result<usize, FsError>;
    
    /// Truncate file to specified size
    fn truncate(&self, size: u64) -> Result<(), FsError>;
    
    /// Sync file data to disk
    fn sync(&self) -> Result<(), FsError>;
    
    /// Get file metadata
    fn metadata(&self) -> Result<Metadata, FsError>;
    
    /// Set file metadata
    fn set_metadata(&self, metadata: &Metadata) -> Result<(), FsError>;
}

/// Directory operations trait
pub trait DirOps: Send + Sync {
    /// List directory entries
    fn readdir(&self) -> Result<Vec<DirEntry>, FsError>;
    
    /// Lookup entry by name
    fn lookup(&self, name: &str) -> Result<Arc<dyn VfsNode>, FsError>;
    
    /// Create a new file
    fn create(&self, name: &str, permissions: Permissions) -> Result<Arc<dyn VfsNode>, FsError>;
    
    /// Create a new directory
    fn mkdir(&self, name: &str, permissions: Permissions) -> Result<Arc<dyn VfsNode>, FsError>;
    
    /// Remove an entry
    fn unlink(&self, name: &str) -> Result<(), FsError>;
    
    /// Rename an entry
    fn rename(&self, old_name: &str, new_name: &str) -> Result<(), FsError>;
}

/// VFS node trait - represents any filesystem object
pub trait VfsNode: Send + Sync {
    /// Get node type
    fn node_type(&self) -> FileType;
    
    /// Get file operations (if applicable)
    fn as_file(&self) -> Option<&dyn FileOps>;
    
    /// Get directory operations (if applicable)
    fn as_dir(&self) -> Option<&dyn DirOps>;
    
    /// Get metadata
    fn metadata(&self) -> Result<Metadata, FsError>;
    
    /// Set metadata
    fn set_metadata(&self, metadata: &Metadata) -> Result<(), FsError>;
}

/// Filesystem operations trait
pub trait FilesystemOps: Send + Sync {
    /// Get filesystem root
    fn root(&self) -> Arc<dyn VfsNode>;
    
    /// Get filesystem statistics
    fn statfs(&self) -> Result<FsStats, FsError>;
    
    /// Sync filesystem to disk
    fn sync(&self) -> Result<(), FsError>;
    
    /// Unmount filesystem
    fn unmount(&self) -> Result<(), FsError>;
}

/// Filesystem statistics
#[derive(Debug, Clone)]
pub struct FsStats {
    pub total_blocks: u64,
    pub free_blocks: u64,
    pub available_blocks: u64,
    pub total_inodes: u64,
    pub free_inodes: u64,
    pub block_size: u32,
    pub name_max: u32,
}

/// Filesystem error types
#[derive(Debug, Clone)]
pub enum FsError {
    NotFound,
    PermissionDenied,
    Exists,
    NotDirectory,
    IsDirectory,
    NotEmpty,
    NoSpace,
    IoError,
    InvalidPath,
    ReadOnly,
    NotSupported,
    Busy,
    NameTooLong,
    InvalidArgument,
}

impl fmt::Display for FsError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            FsError::NotFound => write!(f, "No such file or directory"),
            FsError::PermissionDenied => write!(f, "Permission denied"),
            FsError::Exists => write!(f, "File exists"),
            FsError::NotDirectory => write!(f, "Not a directory"),
            FsError::IsDirectory => write!(f, "Is a directory"),
            FsError::NotEmpty => write!(f, "Directory not empty"),
            FsError::NoSpace => write!(f, "No space left on device"),
            FsError::IoError => write!(f, "I/O error"),
            FsError::InvalidPath => write!(f, "Invalid path"),
            FsError::ReadOnly => write!(f, "Read-only filesystem"),
            FsError::NotSupported => write!(f, "Operation not supported"),
            FsError::Busy => write!(f, "Device or resource busy"),
            FsError::NameTooLong => write!(f, "File name too long"),
            FsError::InvalidArgument => write!(f, "Invalid argument"),
        }
    }
}

/// Mount point structure
struct MountPoint {
    path: String,
    filesystem: Arc<dyn FilesystemOps>,
}

/// VFS manager
pub struct Vfs {
    root: Arc<dyn VfsNode>,
    mounts: RwLock<Vec<MountPoint>>,
}

impl Vfs {
    /// Create a new VFS with a root filesystem
    pub fn new(root_fs: Arc<dyn FilesystemOps>) -> Self {
        Self {
            root: root_fs.root(),
            mounts: RwLock::new(Vec::new()),
        }
    }
    
    /// Mount a filesystem at the specified path
    pub fn mount(&self, path: &str, filesystem: Arc<dyn FilesystemOps>) -> Result<(), FsError> {
        // Validate mount path
        let node = self.lookup_path(path)?;
        if node.node_type() != FileType::Directory {
            return Err(FsError::NotDirectory);
        }
        
        // Add to mount table
        let mut mounts = self.mounts.write();
        mounts.push(MountPoint {
            path: path.to_string(),
            filesystem,
        });
        
        Ok(())
    }
    
    /// Unmount a filesystem
    pub fn unmount(&self, path: &str) -> Result<(), FsError> {
        let mut mounts = self.mounts.write();
        
        // Find and remove mount point
        let index = mounts.iter().position(|m| m.path == path)
            .ok_or(FsError::NotFound)?;
        
        let mount = mounts.remove(index);
        mount.filesystem.unmount()?;
        
        Ok(())
    }
    
    /// Lookup a path in the VFS
    pub fn lookup_path(&self, path: &str) -> Result<Arc<dyn VfsNode>, FsError> {
        if path.is_empty() || !path.starts_with('/') {
            return Err(FsError::InvalidPath);
        }
        
        let components: Vec<&str> = path.split('/')
            .filter(|c| !c.is_empty())
            .collect();
        
        let mut current = self.root.clone();
        
        for component in components {
            // Check for mount points
            let current_path = self.get_current_path(&current);
            if let Some(mount) = self.find_mount(&current_path) {
                current = mount.filesystem.root();
            }
            
            // Lookup in current directory
            let dir = current.as_dir()
                .ok_or(FsError::NotDirectory)?;
            
            current = dir.lookup(component)?;
        }
        
        Ok(current)
    }
    
    /// Find mount point for a path
    fn find_mount(&self, path: &str) -> Option<MountPoint> {
        let mounts = self.mounts.read();
        mounts.iter()
            .find(|m| m.path == path)
            .cloned()
    }
    
    /// Get the path of a node (simplified)
    fn get_current_path(&self, _node: &Arc<dyn VfsNode>) -> String {
        // This would require tracking paths or implementing a proper path resolution system
        String::from("/")
    }
    
    /// Open a file
    pub fn open(&self, path: &str, flags: OpenFlags) -> Result<File, FsError> {
        let node = if flags.contains(OpenFlags::CREATE) {
            // Create file if it doesn't exist
            match self.lookup_path(path) {
                Ok(node) => {
                    if flags.contains(OpenFlags::EXCLUSIVE) {
                        return Err(FsError::Exists);
                    }
                    node
                }
                Err(FsError::NotFound) => {
                    // Create the file
                    let parent_path = path.rsplit_once('/')
                        .ok_or(FsError::InvalidPath)?
                        .0;
                    let filename = path.rsplit_once('/')
                        .ok_or(FsError::InvalidPath)?
                        .1;
                    
                    let parent = self.lookup_path(parent_path)?;
                    let dir = parent.as_dir()
                        .ok_or(FsError::NotDirectory)?;
                    
                    dir.create(filename, Permissions::new())?
                }
                Err(e) => return Err(e),
            }
        } else {
            self.lookup_path(path)?
        };
        
        // Check if it's a regular file
        if node.node_type() != FileType::Regular {
            return Err(FsError::IsDirectory);
        }
        
        Ok(File {
            node,
            position: 0,
            flags,
        })
    }
    
    /// Create a directory
    pub fn mkdir(&self, path: &str, permissions: Permissions) -> Result<(), FsError> {
        let parent_path = path.rsplit_once('/')
            .ok_or(FsError::InvalidPath)?
            .0;
        let dirname = path.rsplit_once('/')
            .ok_or(FsError::InvalidPath)?
            .1;
        
        let parent = self.lookup_path(parent_path)?;
        let dir = parent.as_dir()
            .ok_or(FsError::NotDirectory)?;
        
        dir.mkdir(dirname, permissions)?;
        Ok(())
    }
    
    /// Remove a file or empty directory
    pub fn remove(&self, path: &str) -> Result<(), FsError> {
        let parent_path = path.rsplit_once('/')
            .ok_or(FsError::InvalidPath)?
            .0;
        let name = path.rsplit_once('/')
            .ok_or(FsError::InvalidPath)?
            .1;
        
        let parent = self.lookup_path(parent_path)?;
        let dir = parent.as_dir()
            .ok_or(FsError::NotDirectory)?;
        
        dir.unlink(name)
    }
    
    /// Read directory entries
    pub fn readdir(&self, path: &str) -> Result<Vec<DirEntry>, FsError> {
        let node = self.lookup_path(path)?;
        let dir = node.as_dir()
            .ok_or(FsError::NotDirectory)?;
        
        dir.readdir()
    }
    
    /// Get file metadata
    pub fn stat(&self, path: &str) -> Result<Metadata, FsError> {
        let node = self.lookup_path(path)?;
        node.metadata()
    }
    
    /// Rename a file or directory
    pub fn rename(&self, old_path: &str, new_path: &str) -> Result<(), FsError> {
        // Get parent directories and names
        let (old_parent_path, old_name) = old_path.rsplit_once('/')
            .ok_or(FsError::InvalidPath)?;
        let (new_parent_path, new_name) = new_path.rsplit_once('/')
            .ok_or(FsError::InvalidPath)?;
        
        if old_parent_path == new_parent_path {
            // Same directory - simple rename
            let parent = self.lookup_path(old_parent_path)?;
            let dir = parent.as_dir()
                .ok_or(FsError::NotDirectory)?;
            
            dir.rename(old_name, new_name)
        } else {
            // Different directories - need to move
            // This is more complex and might not be supported by all filesystems
            Err(FsError::NotSupported)
        }
    }
}

/// File handle
pub struct File {
    node: Arc<dyn VfsNode>,
    position: u64,
    flags: OpenFlags,
}

impl File {
    /// Read data from file
    pub fn read(&mut self, buffer: &mut [u8]) -> Result<usize, FsError> {
        if !self.flags.contains(OpenFlags::READ) {
            return Err(FsError::PermissionDenied);
        }
        
        let file_ops = self.node.as_file()
            .ok_or(FsError::InvalidArgument)?;
        
        let bytes_read = file_ops.read(self.position, buffer)?;
        self.position += bytes_read as u64;
        
        Ok(bytes_read)
    }
    
    /// Write data to file
    pub fn write(&mut self, buffer: &[u8]) -> Result<usize, FsError> {
        if !self.flags.contains(OpenFlags::WRITE) {
            return Err(FsError::PermissionDenied);
        }
        
        let file_ops = self.node.as_file()
            .ok_or(FsError::InvalidArgument)?;
        
        let bytes_written = file_ops.write(self.position, buffer)?;
        self.position += bytes_written as u64;
        
        Ok(bytes_written)
    }
    
    /// Seek to position in file
    pub fn seek(&mut self, position: SeekFrom) -> Result<u64, FsError> {
        let metadata = self.node.metadata()?;
        
        self.position = match position {
            SeekFrom::Start(offset) => offset,
            SeekFrom::Current(offset) => {
                if offset >= 0 {
                    self.position + offset as u64
                } else {
                    self.position.saturating_sub((-offset) as u64)
                }
            }
            SeekFrom::End(offset) => {
                if offset >= 0 {
                    metadata.size + offset as u64
                } else {
                    metadata.size.saturating_sub((-offset) as u64)
                }
            }
        };
        
        Ok(self.position)
    }
    
    /// Truncate file to specified size
    pub fn truncate(&self, size: u64) -> Result<(), FsError> {
        if !self.flags.contains(OpenFlags::WRITE) {
            return Err(FsError::PermissionDenied);
        }
        
        let file_ops = self.node.as_file()
            .ok_or(FsError::InvalidArgument)?;
        
        file_ops.truncate(size)
    }
    
    /// Sync file to disk
    pub fn sync(&self) -> Result<(), FsError> {
        let file_ops = self.node.as_file()
            .ok_or(FsError::InvalidArgument)?;
        
        file_ops.sync()
    }
    
    /// Get file metadata
    pub fn metadata(&self) -> Result<Metadata, FsError> {
        self.node.metadata()
    }
}

/// Open flags
bitflags! {
    pub struct OpenFlags: u32 {
        const READ = 0x0001;
        const WRITE = 0x0002;
        const CREATE = 0x0004;
        const EXCLUSIVE = 0x0008;
        const TRUNCATE = 0x0010;
        const APPEND = 0x0020;
    }
}

/// Seek position
#[derive(Debug, Clone, Copy)]
pub enum SeekFrom {
    Start(u64),
    Current(i64),
    End(i64),
}

/// Global VFS instance
static VFS: RwLock<Option<Arc<Vfs>>> = RwLock::new(None);

/// Initialize VFS with root filesystem
pub fn init(root_fs: Arc<dyn FilesystemOps>) {
    let vfs = Arc::new(Vfs::new(root_fs));
    *VFS.write() = Some(vfs);
}

/// Get VFS instance
pub fn get() -> Result<Arc<Vfs>, FsError> {
    VFS.read()
        .as_ref()
        .cloned()
        .ok_or(FsError::NotFound)
}
```

### 2. FAT32 Implementation (fat32.rs)

```rust
//! FAT32 filesystem implementation

use alloc::string::String;
use alloc::vec::Vec;
use alloc::sync::Arc;
use core::mem;
use spin::RwLock;

use super::vfs::*;

/// FAT32 boot sector
#[repr(C, packed)]
struct BootSector {
    jump_boot: [u8; 3],
    oem_name: [u8; 8],
    bytes_per_sector: u16,
    sectors_per_cluster: u8,
    reserved_sectors: u16,
    num_fats: u8,
    root_entries: u16,
    total_sectors_16: u16,
    media_type: u8,
    fat_size_16: u16,
    sectors_per_track: u16,
    heads: u16,
    hidden_sectors: u32,
    total_sectors_32: u32,
    // FAT32 specific
    fat_size_32: u32,
    ext_flags: u16,
    fs_version: u16,
    root_cluster: u32,
    fs_info: u16,
    backup_boot_sector: u16,
    reserved: [u8; 12],
    drive_number: u8,
    reserved1: u8,
    boot_signature: u8,
    volume_id: u32,
    volume_label: [u8; 11],
    fs_type: [u8; 8],
}

/// FAT32 FSInfo structure
#[repr(C, packed)]
struct FsInfo {
    lead_signature: u32,
    reserved1: [u8; 480],
    struct_signature: u32,
    free_count: u32,
    next_free: u32,
    reserved2: [u8; 12],
    trail_signature: u32,
}

/// FAT entry values
const FAT_ENTRY_FREE: u32 = 0x00000000;
const FAT_ENTRY_END: u32 = 0x0FFFFFF8;
const FAT_ENTRY_BAD: u32 = 0x0FFFFFF7;
const FAT_ENTRY_MASK: u32 = 0x0FFFFFFF;

/// Directory entry attributes
const ATTR_READ_ONLY: u8 = 0x01;
const ATTR_HIDDEN: u8 = 0x02;
const ATTR_SYSTEM: u8 = 0x04;
const ATTR_VOLUME_ID: u8 = 0x08;
const ATTR_DIRECTORY: u8 = 0x10;
const ATTR_ARCHIVE: u8 = 0x20;
const ATTR_LONG_NAME: u8 = ATTR_READ_ONLY | ATTR_HIDDEN | ATTR_SYSTEM | ATTR_VOLUME_ID;

/// FAT32 directory entry
#[repr(C, packed)]
#[derive(Clone, Copy)]
struct DirEntry {
    name: [u8; 11],
    attributes: u8,
    nt_reserved: u8,
    creation_time_tenth: u8,
    creation_time: u16,
    creation_date: u16,
    last_access_date: u16,
    first_cluster_hi: u16,
    write_time: u16,
    write_date: u16,
    first_cluster_lo: u16,
    file_size: u32,
}

impl DirEntry {
    /// Check if entry is free
    fn is_free(&self) -> bool {
        self.name[0] == 0x00 || self.name[0] == 0xE5
    }
    
    /// Check if entry is last
    fn is_last(&self) -> bool {
        self.name[0] == 0x00
    }
    
    /// Check if entry is long name
    fn is_long_name(&self) -> bool {
        self.attributes == ATTR_LONG_NAME
    }
    
    /// Check if entry is directory
    fn is_directory(&self) -> bool {
        self.attributes & ATTR_DIRECTORY != 0
    }
    
    /// Check if entry is volume label
    fn is_volume_label(&self) -> bool {
        self.attributes & ATTR_VOLUME_ID != 0
    }
    
    /// Get first cluster number
    fn first_cluster(&self) -> u32 {
        ((self.first_cluster_hi as u32) << 16) | (self.first_cluster_lo as u32)
    }
    
    /// Set first cluster number
    fn set_first_cluster(&mut self, cluster: u32) {
        self.first_cluster_hi = (cluster >> 16) as u16;
        self.first_cluster_lo = cluster as u16;
    }
    
    /// Convert filename to string
    fn filename(&self) -> String {
        if self.name[0] == 0x05 {
            // Special case: 0x05 represents 0xE5
            let mut name = self.name;
            name[0] = 0xE5;
            Self::parse_filename(&name)
        } else {
            Self::parse_filename(&self.name)
        }
    }
    
    /// Parse 8.3 filename
    fn parse_filename(name: &[u8; 11]) -> String {
        let basename = &name[0..8];
        let extension = &name[8..11];
        
        let base = String::from_utf8_lossy(basename).trim_end().to_string();
        let ext = String::from_utf8_lossy(extension).trim_end().to_string();
        
        if ext.is_empty() {
            base
        } else {
            format!("{}.{}", base, ext)
        }
    }
    
    /// Create filename from string
    fn create_filename(name: &str) -> Result<[u8; 11], FsError> {
        let mut result = [b' '; 11];
        
        let parts: Vec<&str> = name.split('.').collect();
        if parts.len() > 2 {
            return Err(FsError::InvalidArgument);
        }
        
        // Copy basename (up to 8 chars)
        let basename = parts[0];
        if basename.is_empty() || basename.len() > 8 {
            return Err(FsError::NameTooLong);
        }
        
        for (i, ch) in basename.chars().enumerate() {
            if i >= 8 {
                break;
            }
            result[i] = ch.to_ascii_uppercase() as u8;
        }
        
        // Copy extension (up to 3 chars)
        if parts.len() == 2 {
            let extension = parts[1];
            if extension.len() > 3 {
                return Err(FsError::NameTooLong);
            }
            
            for (i, ch) in extension.chars().enumerate() {
                if i >= 3 {
                    break;
                }
                result[8 + i] = ch.to_ascii_uppercase() as u8;
            }
        }
        
        Ok(result)
    }
}

/// Long filename entry
#[repr(C, packed)]
#[derive(Clone, Copy)]
struct LongDirEntry {
    sequence: u8,
    name1: [u16; 5],
    attributes: u8,
    entry_type: u8,
    checksum: u8,
    name2: [u16; 6],
    first_cluster: u16,
    name3: [u16; 2],
}

impl LongDirEntry {
    /// Check if this is the last entry in sequence
    fn is_last(&self) -> bool {
        self.sequence & 0x40 != 0
    }
    
    /// Get sequence number
    fn sequence_number(&self) -> u8 {
        self.sequence & 0x3F
    }
    
    /// Extract characters from long name entry
    fn get_chars(&self) -> Vec<char> {
        let mut chars = Vec::new();
        
        // Extract from name1
        for &ch in &self.name1 {
            if ch == 0 || ch == 0xFFFF {
                return chars;
            }
            chars.push(char::from_u32(ch as u32).unwrap_or('?'));
        }
        
        // Extract from name2
        for &ch in &self.name2 {
            if ch == 0 || ch == 0xFFFF {
                return chars;
            }
            chars.push(char::from_u32(ch as u32).unwrap_or('?'));
        }
        
        // Extract from name3
        for &ch in &self.name3 {
            if ch == 0 || ch == 0xFFFF {
                return chars;
            }
            chars.push(char::from_u32(ch as u32).unwrap_or('?'));
        }
        
        chars
    }
}

/// FAT32 filesystem
pub struct Fat32 {
    device: Arc<dyn BlockDevice>,
    boot_sector: BootSector,
    fs_info: FsInfo,
    fat_start_sector: u32,
    data_start_sector: u32,
    sectors_per_cluster: u32,
    bytes_per_sector: u32,
    total_clusters: u32,
    // Caching
    fat_cache: RwLock<Option<Vec<u32>>>,
}

impl Fat32 {
    /// Create new FAT32 filesystem from block device
    pub fn new(device: Arc<dyn BlockDevice>) -> Result<Self, FsError> {
        // Read boot sector
        let mut boot_sector_data = vec![0u8; 512];
        device.read_blocks(0, &mut boot_sector_data)?;
        
        let boot_sector: BootSector = unsafe {
            mem::transmute_copy(&boot_sector_data[0])
        };
        
        // Validate FAT32
        if &boot_sector.fs_type != b"FAT32   " {
            return Err(FsError::InvalidArgument);
        }
        
        // Read FSInfo
        let mut fs_info_data = vec![0u8; 512];
        device.read_blocks(boot_sector.fs_info as u64, &mut fs_info_data)?;
        
        let fs_info: FsInfo = unsafe {
            mem::transmute_copy(&fs_info_data[0])
        };
        
        // Calculate filesystem parameters
        let bytes_per_sector = boot_sector.bytes_per_sector as u32;
        let sectors_per_cluster = boot_sector.sectors_per_cluster as u32;
        let reserved_sectors = boot_sector.reserved_sectors as u32;
        let fat_size = boot_sector.fat_size_32;
        let num_fats = boot_sector.num_fats as u32;
        
        let fat_start_sector = reserved_sectors;
        let data_start_sector = fat_start_sector + (num_fats * fat_size);
        
        let total_sectors = if boot_sector.total_sectors_32 != 0 {
            boot_sector.total_sectors_32
        } else {
            boot_sector.total_sectors_16 as u32
        };
        
        let data_sectors = total_sectors - data_start_sector;
        let total_clusters = data_sectors / sectors_per_cluster;
        
        Ok(Self {
            device,
            boot_sector,
            fs_info,
            fat_start_sector,
            data_start_sector,
            sectors_per_cluster,
            bytes_per_sector,
            total_clusters,
            fat_cache: RwLock::new(None),
        })
    }
    
    /// Read FAT entry
    fn read_fat_entry(&self, cluster: u32) -> Result<u32, FsError> {
        let fat_offset = cluster * 4;
        let fat_sector = self.fat_start_sector + (fat_offset / self.bytes_per_sector);
        let fat_entry_offset = (fat_offset % self.bytes_per_sector) as usize;
        
        let mut sector_data = vec![0u8; self.bytes_per_sector as usize];
        self.device.read_blocks(fat_sector as u64, &mut sector_data)?;
        
        let entry = u32::from_le_bytes([
            sector_data[fat_entry_offset],
            sector_data[fat_entry_offset + 1],
            sector_data[fat_entry_offset + 2],
            sector_data[fat_entry_offset + 3],
        ]);
        
        Ok(entry & FAT_ENTRY_MASK)
    }
    
    /// Write FAT entry
    fn write_fat_entry(&self, cluster: u32, value: u32) -> Result<(), FsError> {
        let fat_offset = cluster * 4;
        let fat_sector = self.fat_start_sector + (fat_offset / self.bytes_per_sector);
        let fat_entry_offset = (fat_offset % self.bytes_per_sector) as usize;
        
        let mut sector_data = vec![0u8; self.bytes_per_sector as usize];
        self.device.read_blocks(fat_sector as u64, &mut sector_data)?;
        
        let entry_bytes = (value & FAT_ENTRY_MASK).to_le_bytes();
        sector_data[fat_entry_offset..fat_entry_offset + 4].copy_from_slice(&entry_bytes);
        
        self.device.write_blocks(fat_sector as u64, &sector_data)?;
        
        // Update all FAT copies
        for i in 1..self.boot_sector.num_fats {
            let fat_copy_sector = fat_sector + (i as u32 * self.boot_sector.fat_size_32);
            self.device.write_blocks(fat_copy_sector as u64, &sector_data)?;
        }
        
        Ok(())
    }
    
    /// Find free cluster
    fn find_free_cluster(&self) -> Result<u32, FsError> {
        // Start from hint in FSInfo
        let start_cluster = self.fs_info.next_free.max(2);
        
        for cluster in start_cluster..self.total_clusters {
            if self.read_fat_entry(cluster)? == FAT_ENTRY_FREE {
                return Ok(cluster);
            }
        }
        
        // Wrap around search
        for cluster in 2..start_cluster {
            if self.read_fat_entry(cluster)? == FAT_ENTRY_FREE {
                return Ok(cluster);
            }
        }
        
        Err(FsError::NoSpace)
    }
    
    /// Allocate cluster chain
    fn allocate_cluster_chain(&self, count: u32) -> Result<Vec<u32>, FsError> {
        let mut clusters = Vec::new();
        let mut prev_cluster = None;
        
        for _ in 0..count {
            let cluster = self.find_free_cluster()?;
            
            if let Some(prev) = prev_cluster {
                self.write_fat_entry(prev, cluster)?;
            }
            
            self.write_fat_entry(cluster, FAT_ENTRY_END)?;
            clusters.push(cluster);
            prev_cluster = Some(cluster);
        }
        
        Ok(clusters)
    }
    
    /// Free cluster chain
    fn free_cluster_chain(&self, start_cluster: u32) -> Result<(), FsError> {
        let mut cluster = start_cluster;
        
        while cluster >= 2 && cluster < self.total_clusters {
            let next = self.read_fat_entry(cluster)?;
            self.write_fat_entry(cluster, FAT_ENTRY_FREE)?;
            
            if next >= FAT_ENTRY_END {
                break;
            }
            
            cluster = next;
        }
        
        Ok(())
    }
    
    /// Convert cluster to sector
    fn cluster_to_sector(&self, cluster: u32) -> u32 {
        ((cluster - 2) * self.sectors_per_cluster) + self.data_start_sector
    }
    
    /// Read cluster data
    fn read_cluster(&self, cluster: u32, buffer: &mut [u8]) -> Result<(), FsError> {
        let sector = self.cluster_to_sector(cluster);
        self.device.read_blocks(sector as u64, buffer)?;
        Ok(())
    }
    
    /// Write cluster data
    fn write_cluster(&self, cluster: u32, buffer: &[u8]) -> Result<(), FsError> {
        let sector = self.cluster_to_sector(cluster);
        self.device.write_blocks(sector as u64, buffer)?;
        Ok(())
    }
    
    /// Read directory entries from cluster chain
    fn read_directory(&self, start_cluster: u32) -> Result<Vec<(DirEntry, Option<String>)>, FsError> {
        let mut entries = Vec::new();
        let mut cluster = start_cluster;
        let cluster_size = self.sectors_per_cluster * self.bytes_per_sector;
        
        while cluster >= 2 && cluster < self.total_clusters {
            let mut cluster_data = vec![0u8; cluster_size as usize];
            self.read_cluster(cluster, &mut cluster_data)?;
            
            let dir_entries = unsafe {
                core::slice::from_raw_parts(
                    cluster_data.as_ptr() as *const DirEntry,
                    cluster_size as usize / mem::size_of::<DirEntry>()
                )
            };
            
            let mut long_name_parts = Vec::new();
            
            for entry in dir_entries {
                if entry.is_last() {
                    break;
                }
                
                if entry.is_free() {
                    continue;
                }
                
                if entry.is_long_name() {
                    // Long name entry
                    let long_entry = unsafe {
                        mem::transmute_copy::<DirEntry, LongDirEntry>(entry)
                    };
                    
                    long_name_parts.push(long_entry);
                } else {
                    // Regular entry
                    let long_name = if !long_name_parts.is_empty() {
                        // Reconstruct long name
                        let mut name = String::new();
                        
                        // Sort by sequence number (reverse order)
                        long_name_parts.sort_by(|a, b| b.sequence_number().cmp(&a.sequence_number()));
                        
                        for part in &long_name_parts {
                            for ch in part.get_chars() {
                                name.push(ch);
                            }
                        }
                        
                        long_name_parts.clear();
                        Some(name)
                    } else {
                        None
                    };
                    
                    entries.push((*entry, long_name));
                }
            }
            
            // Get next cluster
            let next = self.read_fat_entry(cluster)?;
            if next >= FAT_ENTRY_END {
                break;
            }
            
            cluster = next;
        }
        
        Ok(entries)
    }
    
    /// Create new directory entry
    fn create_directory_entry(
        &self,
        parent_cluster: u32,
        name: &str,
        attributes: u8,
    ) -> Result<DirEntry, FsError> {
        // Create short name
        let short_name = DirEntry::create_filename(name)?;
        
        // Allocate cluster for directory entries
        let cluster = self.find_free_cluster()?;
        self.write_fat_entry(cluster, FAT_ENTRY_END)?;
        
        // Create directory entry
        let mut entry = DirEntry {
            name: short_name,
            attributes,
            nt_reserved: 0,
            creation_time_tenth: 0,
            creation_time: 0,
            creation_date: 0,
            last_access_date: 0,
            first_cluster_hi: (cluster >> 16) as u16,
            write_time: 0,
            write_date: 0,
            first_cluster_lo: cluster as u16,
            file_size: 0,
        };
        
        // Add entry to parent directory
        self.add_directory_entry(parent_cluster, &entry)?;
        
        // If creating a directory, initialize it
        if attributes & ATTR_DIRECTORY != 0 {
            self.init_directory(cluster, parent_cluster)?;
        }
        
        Ok(entry)
    }
    
    /// Add entry to directory
    fn add_directory_entry(&self, dir_cluster: u32, entry: &DirEntry) -> Result<(), FsError> {
        let mut cluster = dir_cluster;
        let cluster_size = self.sectors_per_cluster * self.bytes_per_sector;
        let entries_per_cluster = cluster_size as usize / mem::size_of::<DirEntry>();
        
        loop {
            let mut cluster_data = vec![0u8; cluster_size as usize];
            self.read_cluster(cluster, &mut cluster_data)?;
            
            let dir_entries = unsafe {
                core::slice::from_raw_parts_mut(
                    cluster_data.as_mut_ptr() as *mut DirEntry,
                    entries_per_cluster
                )
            };
            
            // Find free entry
            for (i, dir_entry) in dir_entries.iter_mut().enumerate() {
                if dir_entry.is_free() {
                    *dir_entry = *entry;
                    self.write_cluster(cluster, &cluster_data)?;
                    return Ok(());
                }
            }
            
            // No free entry in this cluster, get next
            let next = self.read_fat_entry(cluster)?;
            if next >= FAT_ENTRY_END {
                // Allocate new cluster
                let new_cluster = self.find_free_cluster()?;
                self.write_fat_entry(cluster, new_cluster)?;
                self.write_fat_entry(new_cluster, FAT_ENTRY_END)?;
                
                // Initialize new cluster
                let mut new_cluster_data = vec![0u8; cluster_size as usize];
                self.write_cluster(new_cluster, &new_cluster_data)?;
                
                cluster = new_cluster;
            } else {
                cluster = next;
            }
        }
    }
    
    /// Initialize new directory with . and .. entries
    fn init_directory(&self, cluster: u32, parent_cluster: u32) -> Result<(), FsError> {
        let cluster_size = self.sectors_per_cluster * self.bytes_per_sector;
        let mut cluster_data = vec![0u8; cluster_size as usize];
        
        let dir_entries = unsafe {
            core::slice::from_raw_parts_mut(
                cluster_data.as_mut_ptr() as *mut DirEntry,
                cluster_size as usize / mem::size_of::<DirEntry>()
            )
        };
        
        // Create . entry
        dir_entries[0] = DirEntry {
            name: [b'.', b' ', b' ', b' ', b' ', b' ', b' ', b' ', b' ', b' ', b' '],
            attributes: ATTR_DIRECTORY,
            nt_reserved: 0,
            creation_time_tenth: 0,
            creation_time: 0,
            creation_date: 0,
            last_access_date: 0,
            first_cluster_hi: (cluster >> 16) as u16,
            write_time: 0,
            write_date: 0,
            first_cluster_lo: cluster as u16,
            file_size: 0,
        };
        
        // Create .. entry
        dir_entries[1] = DirEntry {
            name: [b'.', b'.', b' ', b' ', b' ', b' ', b' ', b' ', b' ', b' ', b' '],
            attributes: ATTR_DIRECTORY,
            nt_reserved: 0,
            creation_time_tenth: 0,
            creation_time: 0,
            creation_date: 0,
            last_access_date: 0,
            first_cluster_hi: (parent_cluster >> 16) as u16,
            write_time: 0,
            write_date: 0,
            first_cluster_lo: parent_cluster as u16,
            file_size: 0,
        };
        
        self.write_cluster(cluster, &cluster_data)?;
        Ok(())
    }
}

/// FAT32 node implementation
struct Fat32Node {
    fs: Arc<Fat32>,
    entry: DirEntry,
    path: String,
}

impl VfsNode for Fat32Node {
    fn node_type(&self) -> FileType {
        if self.entry.is_directory() {
            FileType::Directory
        } else {
            FileType::Regular
        }
    }
    
    fn as_file(&self) -> Option<&dyn FileOps> {
        if !self.entry.is_directory() {
            Some(self)
        } else {
            None
        }
    }
    
    fn as_dir(&self) -> Option<&dyn DirOps> {
        if self.entry.is_directory() {
            Some(self)
        } else {
            None
        }
    }
    
    fn metadata(&self) -> Result<Metadata, FsError> {
        Ok(Metadata {
            file_type: self.node_type(),
            size: self.entry.file_size as u64,
            permissions: Permissions::new(), // Default permissions
            created: 0, // TODO: Convert FAT32 timestamps
            modified: 0,
            accessed: 0,
            uid: 0,
            gid: 0,
            nlinks: 1,
        })
    }
    
    fn set_metadata(&self, _metadata: &Metadata) -> Result<(), FsError> {
        // TODO: Update file attributes and timestamps
        Ok(())
    }
}

impl FileOps for Fat32Node {
    fn read(&self, offset: u64, buffer: &mut [u8]) -> Result<usize, FsError> {
        if self.entry.is_directory() {
            return Err(FsError::IsDirectory);
        }
        
        let file_size = self.entry.file_size as u64;
        if offset >= file_size {
            return Ok(0);
        }
        
        let cluster_size = self.fs.sectors_per_cluster * self.fs.bytes_per_sector;
        let mut current_offset = 0u64;
        let mut bytes_read = 0usize;
        let mut cluster = self.entry.first_cluster();
        
        while cluster >= 2 && cluster < self.fs.total_clusters && bytes_read < buffer.len() {
            let cluster_data_size = cluster_size as u64;
            
            if current_offset + cluster_data_size <= offset {
                // Skip this cluster
                current_offset += cluster_data_size;
            } else {
                // Read from this cluster
                let mut cluster_buffer = vec![0u8; cluster_size as usize];
                self.fs.read_cluster(cluster, &mut cluster_buffer)?;
                
                let cluster_offset = if current_offset < offset {
                    (offset - current_offset) as usize
                } else {
                    0
                };
                
                let available = (cluster_size as usize - cluster_offset).min(file_size as usize - bytes_read);
                let to_read = available.min(buffer.len() - bytes_read);
                
                buffer[bytes_read..bytes_read + to_read]
                    .copy_from_slice(&cluster_buffer[cluster_offset..cluster_offset + to_read]);
                
                bytes_read += to_read;
                current_offset += cluster_data_size;
            }
            
            // Get next cluster
            let next = self.fs.read_fat_entry(cluster)?;
            if next >= FAT_ENTRY_END {
                break;
            }
            cluster = next;
        }
        
        Ok(bytes_read)
    }
    
    fn write(&self, offset: u64, buffer: &[u8]) -> Result<usize, FsError> {
        if self.entry.is_directory() {
            return Err(FsError::IsDirectory);
        }
        
        // TODO: Implement file writing
        Err(FsError::NotSupported)
    }
    
    fn truncate(&self, size: u64) -> Result<(), FsError> {
        // TODO: Implement file truncation
        Err(FsError::NotSupported)
    }
    
    fn sync(&self) -> Result<(), FsError> {
        // TODO: Implement file sync
        Ok(())
    }
    
    fn metadata(&self) -> Result<Metadata, FsError> {
        VfsNode::metadata(self)
    }
    
    fn set_metadata(&self, metadata: &Metadata) -> Result<(), FsError> {
        VfsNode::set_metadata(self, metadata)
    }
}

impl DirOps for Fat32Node {
    fn readdir(&self) -> Result<Vec<DirEntry as VfsDirEntry>, FsError> {
        if !self.entry.is_directory() {
            return Err(FsError::NotDirectory);
        }
        
        let entries = self.fs.read_directory(self.entry.first_cluster())?;
        
        Ok(entries.into_iter()
            .filter(|(entry, _)| !entry.is_volume_label())
            .filter(|(entry, _)| {
                let name = entry.filename();
                name != "." && name != ".."
            })
            .map(|(entry, long_name)| {
                let name = long_name.unwrap_or_else(|| entry.filename());
                let file_type = if entry.is_directory() {
                    FileType::Directory
                } else {
                    FileType::Regular
                };
                
                DirEntry {
                    name,
                    inode: entry.first_cluster() as u64,
                    file_type,
                }
            })
            .collect())
    }
    
    fn lookup(&self, name: &str) -> Result<Arc<dyn VfsNode>, FsError> {
        if !self.entry.is_directory() {
            return Err(FsError::NotDirectory);
        }
        
        let entries = self.fs.read_directory(self.entry.first_cluster())?;
        
        for (entry, long_name) in entries {
            let entry_name = long_name.unwrap_or_else(|| entry.filename());
            if entry_name == name {
                return Ok(Arc::new(Fat32Node {
                    fs: self.fs.clone(),
                    entry,
                    path: format!("{}/{}", self.path, name),
                }));
            }
        }
        
        Err(FsError::NotFound)
    }
    
    fn create(&self, name: &str, permissions: Permissions) -> Result<Arc<dyn VfsNode>, FsError> {
        if !self.entry.is_directory() {
            return Err(FsError::NotDirectory);
        }
        
        let entry = self.fs.create_directory_entry(
            self.entry.first_cluster(),
            name,
            ATTR_ARCHIVE,
        )?;
        
        Ok(Arc::new(Fat32Node {
            fs: self.fs.clone(),
            entry,
            path: format!("{}/{}", self.path, name),
        }))
    }
    
    fn mkdir(&self, name: &str, permissions: Permissions) -> Result<Arc<dyn VfsNode>, FsError> {
        if !self.entry.is_directory() {
            return Err(FsError::NotDirectory);
        }
        
        let entry = self.fs.create_directory_entry(
            self.entry.first_cluster(),
            name,
            ATTR_DIRECTORY,
        )?;
        
        Ok(Arc::new(Fat32Node {
            fs: self.fs.clone(),
            entry,
            path: format!("{}/{}", self.path, name),
        }))
    }
    
    fn unlink(&self, name: &str) -> Result<(), FsError> {
        // TODO: Implement file deletion
        Err(FsError::NotSupported)
    }
    
    fn rename(&self, old_name: &str, new_name: &str) -> Result<(), FsError> {
        // TODO: Implement file renaming
        Err(FsError::NotSupported)
    }
}

impl FilesystemOps for Fat32 {
    fn root(&self) -> Arc<dyn VfsNode> {
        // Create root directory entry
        let mut root_entry = DirEntry {
            name: [b' '; 11],
            attributes: ATTR_DIRECTORY,
            nt_reserved: 0,
            creation_time_tenth: 0,
            creation_time: 0,
            creation_date: 0,
            last_access_date: 0,
            first_cluster_hi: 0,
            write_time: 0,
            write_date: 0,
            first_cluster_lo: 0,
            file_size: 0,
        };
        
        root_entry.set_first_cluster(self.boot_sector.root_cluster);
        
        Arc::new(Fat32Node {
            fs: Arc::new(self.clone()),
            entry: root_entry,
            path: String::from("/"),
        })
    }
    
    fn statfs(&self) -> Result<FsStats, FsError> {
        // TODO: Calculate actual free space
        Ok(FsStats {
            total_blocks: self.total_clusters as u64,
            free_blocks: 0,
            available_blocks: 0,
            total_inodes: u64::MAX,
            free_inodes: u64::MAX,
            block_size: self.sectors_per_cluster * self.bytes_per_sector,
            name_max: 255,
        })
    }
    
    fn sync(&self) -> Result<(), FsError> {
        // TODO: Sync filesystem metadata
        Ok(())
    }
    
    fn unmount(&self) -> Result<(), FsError> {
        self.sync()
    }
}

/// Block device trait
pub trait BlockDevice: Send + Sync {
    /// Read blocks from device
    fn read_blocks(&self, start_block: u64, buffer: &mut [u8]) -> Result<(), FsError>;
    
    /// Write blocks to device
    fn write_blocks(&self, start_block: u64, buffer: &[u8]) -> Result<(), FsError>;
    
    /// Get block size
    fn block_size(&self) -> u32;
    
    /// Get total number of blocks
    fn total_blocks(&self) -> u64;
}
```

## Usage Examples

### VFS Usage

```rust
use orbita_os::filesystem::vfs::{self, OpenFlags};
use orbita_os::filesystem::fat32::Fat32;

// Initialize filesystem
let block_device = get_block_device();
let fat32_fs = Arc::new(Fat32::new(block_device)?);
vfs::init(fat32_fs);

// Get VFS instance
let vfs = vfs::get()?;

// Open a file
let mut file = vfs.open("/test.txt", OpenFlags::READ | OpenFlags::WRITE)?;

// Read from file
let mut buffer = vec![0u8; 1024];
let bytes_read = file.read(&mut buffer)?;

// Write to file
let data = b"Hello, Orbita OS!";
file.write(data)?;

// Create directory
vfs.mkdir("/documents", Permissions::new())?;

// List directory
let entries = vfs.readdir("/")?.;
for entry in entries {
    println!("{}: {:?}", entry.name, entry.file_type);
}
```

### Mount Additional Filesystem

```rust
// Mount USB drive
let usb_device = get_usb_block_device();
let usb_fs = Arc::new(Fat32::new(usb_device)?);
vfs.mount("/mnt/usb", usb_fs)?;

// Access mounted filesystem
let usb_file = vfs.open("/mnt/usb/data.txt", OpenFlags::READ)?;
```

### File Operations

```rust
use orbita_os::filesystem::vfs::{File, SeekFrom};

// Seek in file
file.seek(SeekFrom::Start(100))?;
file.seek(SeekFrom::End(-10))?;
file.seek(SeekFrom::Current(20))?;

// Truncate file
file.truncate(1024)?;

// Get file metadata
let metadata = file.metadata()?;
println!("File size: {} bytes", metadata.size);
println!("Type: {:?}", metadata.file_type);

// Sync to disk
file.sync()?;
```

## Common Errors and Solutions

### 1. File Not Found

**Error**: `FsError::NotFound` when opening file
**Solution**: 
- Check file path is correct
- Ensure filesystem is mounted
- Verify file exists with `readdir`

### 2. Permission Denied

**Error**: `FsError::PermissionDenied` on file operations
**Solution**: 
- Check file permissions
- Ensure correct open flags
- Verify user has access rights

### 3. Disk Full

**Error**: `FsError::NoSpace` when writing
**Solution**: 
- Check available space with `statfs`
- Delete unnecessary files
- Increase storage capacity

### 4. Corrupted Filesystem

**Error**: Invalid data or unexpected behavior
**Solution**: 
- Run filesystem check (fsck)
- Restore from backup
- Reformat if necessary

## Module Dependencies

1. **Block Device Layer**:
   - Storage drivers (IDE, SATA, USB)
   - Block cache
   - Partition table parsing

2. **Internal Dependencies**:
   - `memory`: Buffer allocation
   - `sync`: Locking primitives
   - `time`: Timestamps

3. **Used By**:
   - `process`: Executable loading
   - `syscall`: File system calls
   - `shell`: Command line operations

## Performance Optimizations

### 1. Block Cache

```rust
pub struct BlockCache {
    cache: RwLock<HashMap<u64, CacheEntry>>,
    capacity: usize,
}

struct CacheEntry {
    data: Vec<u8>,
    dirty: bool,
    last_access: u64,
}

impl BlockCache {
    pub fn read_block(&self, block: u64) -> Result<Vec<u8>, FsError> {
        if let Some(entry) = self.cache.read().get(&block) {
            return Ok(entry.data.clone());
        }
        
        // Cache miss - read from device
        let data = self.device.read_block(block)?;
        self.cache_block(block, data.clone());
        Ok(data)
    }
    
    pub fn write_block(&self, block: u64, data: Vec<u8>) -> Result<(), FsError> {
        self.cache.write().insert(block, CacheEntry {
            data,
            dirty: true,
            last_access: get_time(),
        });
        Ok(())
    }
}
```

### 2. Directory Entry Cache

```rust
pub struct DirCache {
    cache: RwLock<HashMap<String, Arc<dyn VfsNode>>>,
}

impl DirCache {
    pub fn lookup(&self, path: &str) -> Option<Arc<dyn VfsNode>> {
        self.cache.read().get(path).cloned()
    }
    
    pub fn insert(&self, path: String, node: Arc<dyn VfsNode>) {
        self.cache.write().insert(path, node);
    }
}
```

### 3. Write Coalescing

```rust
pub struct WriteBuffer {
    pending: Mutex<HashMap<u64, Vec<u8>>>,
    flush_timer: Timer,
}

impl WriteBuffer {
    pub fn write(&self, offset: u64, data: &[u8]) {
        let mut pending = self.pending.lock();
        pending.insert(offset, data.to_vec());
        
        // Schedule flush
        self.flush_timer.schedule(Duration::from_secs(5));
    }
    
    pub fn flush(&self) -> Result<(), FsError> {
        let mut pending = self.pending.lock();
        for (offset, data) in pending.drain() {
            self.device.write(offset, &data)?;
        }
        Ok(())
    }
}
```

## Advanced Features

### 1. Extended Attributes

```rust
pub trait ExtendedAttributes {
    fn get_xattr(&self, name: &str) -> Result<Vec<u8>, FsError>;
    fn set_xattr(&self, name: &str, value: &[u8]) -> Result<(), FsError>;
    fn list_xattrs(&self) -> Result<Vec<String>, FsError>;
    fn remove_xattr(&self, name: &str) -> Result<(), FsError>;
}
```

### 2. File Locking

```rust
pub struct FileLock {
    file: Arc<dyn VfsNode>,
    lock_type: LockType,
    range: Range<u64>,
}

pub enum LockType {
    Shared,
    Exclusive,
}

impl File {
    pub fn lock(&self, lock_type: LockType, range: Range<u64>) -> Result<FileLock, FsError> {
        // Implement file locking
    }
}
```

### 3. Filesystem Quotas

```rust
pub struct Quota {
    user_id: u32,
    blocks_used: u64,
    blocks_soft_limit: u64,
    blocks_hard_limit: u64,
    inodes_used: u64,
    inodes_soft_limit: u64,
    inodes_hard_limit: u64,
}

pub trait QuotaOps {
    fn get_quota(&self, user_id: u32) -> Result<Quota, FsError>;
    fn set_quota(&self, user_id: u32, quota: &Quota) -> Result<(), FsError>;
}
```

## Future Improvements

1. **Additional Filesystems**:
   - ext4 support
   - NTFS read support
   - Network filesystems (NFS, SMB)
   - FUSE support

2. **Advanced Features**:
   - Journaling
   - Copy-on-write
   - Deduplication
   - Compression
   - Encryption

3. **Performance**:
   - Read-ahead caching
   - Delayed allocation
   - Extent-based allocation
   - B-tree directories

4. **Reliability**:
   - Filesystem snapshots
   - Online fsck
   - RAID support
   - Backup integration