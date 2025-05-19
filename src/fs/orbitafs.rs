//! Beginnings of OrbitaFS implementation.
//!
//! OrbitaFS is planned to provide journaling and snapshot support. At the
//! moment only the skeleton structures are defined.

use alloc::{sync::Arc, string::String};

use super::vfs::{DirEntry, DirOps, FileOps, FileType, FilesystemOps, FsError, Metadata, Permissions, VfsNode, BlockDevice};

/// OrbitaFS node placeholder with journaling information.
pub struct OrbitaNode {
    name: String,
    node_type: FileType,
}

impl OrbitaNode {
    fn new(name: &str, node_type: FileType) -> Arc<Self> {
        Arc::new(Self { name: name.to_string(), node_type })
    }
}

impl VfsNode for OrbitaNode {
    fn node_type(&self) -> FileType { self.node_type }
    fn metadata(&self) -> Result<Metadata, FsError> { Ok(Metadata { file_type: self.node_type, size: 0, permissions: Permissions::new() }) }
}

impl FileOps for OrbitaNode {
    fn read(&self, _offset: u64, _buf: &mut [u8]) -> Result<usize, FsError> { Err(FsError::Unsupported) }
    fn write(&self, _offset: u64, _buf: &[u8]) -> Result<usize, FsError> { Err(FsError::Unsupported) }
    fn truncate(&self, _size: u64) -> Result<(), FsError> { Err(FsError::Unsupported) }
    fn sync(&self) -> Result<(), FsError> { Ok(()) }
}

impl DirOps for OrbitaNode {
    fn readdir(&self) -> Result<Vec<DirEntry>, FsError> { Ok(Vec::new()) }
    fn lookup(&self, _name: &str) -> Result<Arc<dyn VfsNode>, FsError> { Err(FsError::NotFound) }
    fn create(&self, _name: &str, _perms: Permissions) -> Result<Arc<dyn VfsNode>, FsError> { Err(FsError::Unsupported) }
    fn mkdir(&self, _name: &str, _perms: Permissions) -> Result<Arc<dyn VfsNode>, FsError> { Err(FsError::Unsupported) }
    fn unlink(&self, _name: &str) -> Result<(), FsError> { Err(FsError::Unsupported) }
}

/// OrbitaFS structure.
/// TODO: journaling and snapshot support.
pub struct OrbitaFs {
    #[allow(dead_code)]
    device: Arc<dyn BlockDevice>,
    root: Arc<OrbitaNode>,
}

impl OrbitaFs {
    pub fn new(device: Arc<dyn BlockDevice>) -> Self {
        let root = OrbitaNode::new("", FileType::Directory);
        Self { device, root }
    }
}

impl FilesystemOps for OrbitaFs {
    fn root(&self) -> Arc<dyn VfsNode> { self.root.clone() }
}

