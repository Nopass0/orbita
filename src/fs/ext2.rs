//! Simplified ext2 filesystem placeholder.
//!
//! Only minimal structures are provided. Actual ext2 parsing and on-disk layout
//! handling are left as future work.

use alloc::{string::String, sync::Arc};

use super::vfs::{DirEntry, DirOps, FileOps, FileType, FilesystemOps, FsError, Metadata, Permissions, VfsNode, BlockDevice};

/// ext2 filesystem node (in-memory placeholder).
pub struct Ext2Node {
    name: String,
    node_type: FileType,
}

impl Ext2Node {
    fn new(name: &str, node_type: FileType) -> Arc<Self> {
        Arc::new(Self { name: name.to_string(), node_type })
    }
}

impl VfsNode for Ext2Node {
    fn node_type(&self) -> FileType { self.node_type }
    fn metadata(&self) -> Result<Metadata, FsError> {
        Ok(Metadata { file_type: self.node_type, size: 0, permissions: Permissions::new() })
    }
}

impl FileOps for Ext2Node {
    fn read(&self, _offset: u64, _buf: &mut [u8]) -> Result<usize, FsError> { Err(FsError::Unsupported) }
    fn write(&self, _offset: u64, _buf: &[u8]) -> Result<usize, FsError> { Err(FsError::Unsupported) }
    fn truncate(&self, _size: u64) -> Result<(), FsError> { Err(FsError::Unsupported) }
    fn sync(&self) -> Result<(), FsError> { Ok(()) }
}

impl DirOps for Ext2Node {
    fn readdir(&self) -> Result<Vec<DirEntry>, FsError> { Ok(Vec::new()) }
    fn lookup(&self, _name: &str) -> Result<Arc<dyn VfsNode>, FsError> { Err(FsError::NotFound) }
    fn create(&self, _name: &str, _perms: Permissions) -> Result<Arc<dyn VfsNode>, FsError> { Err(FsError::Unsupported) }
    fn mkdir(&self, _name: &str, _perms: Permissions) -> Result<Arc<dyn VfsNode>, FsError> { Err(FsError::Unsupported) }
    fn unlink(&self, _name: &str) -> Result<(), FsError> { Err(FsError::Unsupported) }
}

/// ext2 filesystem structure placeholder.
pub struct Ext2Fs {
    #[allow(dead_code)]
    device: Arc<dyn BlockDevice>,
    root: Arc<Ext2Node>,
}

impl Ext2Fs {
    pub fn new(device: Arc<dyn BlockDevice>) -> Self {
        let root = Ext2Node::new("", FileType::Directory);
        Self { device, root }
    }
}

impl FilesystemOps for Ext2Fs {
    fn root(&self) -> Arc<dyn VfsNode> { self.root.clone() }
}

