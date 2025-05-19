//! Simplified FAT32 filesystem implementation.
//!
//! This is **not** a full FAT32 implementation. It provides only very basic
//! in-memory structures so that the VFS layer can be exercised. Parsing of real
//! on-disk data is out of scope and left as future work.

use alloc::{collections::BTreeMap, string::String, sync::Arc, vec::Vec};
use spin::RwLock;

use super::vfs::{BlockDevice, DirEntry, DirOps, FileOps, FileType, FilesystemOps, FsError, Metadata, Permissions, VfsNode};

/// Node within the simple FAT32 structure.
enum NodeKind {
    File(RwLock<Vec<u8>>),
    Dir(RwLock<BTreeMap<String, Arc<Fat32Node>>>),
}

/// FAT32 filesystem node.
pub struct Fat32Node {
    name: String,
    kind: NodeKind,
    perms: Permissions,
}

impl Fat32Node {
    fn new_dir(name: &str) -> Arc<Self> {
        Arc::new(Self {
            name: name.to_string(),
            kind: NodeKind::Dir(RwLock::new(BTreeMap::new())),
            perms: Permissions::new(),
        })
    }

    fn new_file(name: &str) -> Arc<Self> {
        Arc::new(Self {
            name: name.to_string(),
            kind: NodeKind::File(RwLock::new(Vec::new())),
            perms: Permissions::new(),
        })
    }
}

impl VfsNode for Fat32Node {
    fn node_type(&self) -> FileType {
        match self.kind {
            NodeKind::File(_) => FileType::Regular,
            NodeKind::Dir(_) => FileType::Directory,
        }
    }

    fn as_file(&self) -> Option<&dyn FileOps> {
        if matches!(self.kind, NodeKind::File(_)) { Some(self) } else { None }
    }

    fn as_dir(&self) -> Option<&dyn DirOps> {
        if matches!(self.kind, NodeKind::Dir(_)) { Some(self) } else { None }
    }

    fn metadata(&self) -> Result<Metadata, FsError> {
        let size = match &self.kind {
            NodeKind::File(buf) => buf.read().len() as u64,
            NodeKind::Dir(_) => 0,
        };
        Ok(Metadata { file_type: self.node_type(), size, permissions: self.perms })
    }
}

impl FileOps for Fat32Node {
    fn read(&self, offset: u64, buf: &mut [u8]) -> Result<usize, FsError> {
        if let NodeKind::File(ref data) = self.kind {
            let data = data.read();
            if offset as usize >= data.len() {
                return Ok(0);
            }
            let end = core::cmp::min(data.len(), offset as usize + buf.len());
            let slice = &data[offset as usize..end];
            buf[..slice.len()].copy_from_slice(slice);
            Ok(slice.len())
        } else {
            Err(FsError::InvalidArgument)
        }
    }

    fn write(&self, offset: u64, buf: &[u8]) -> Result<usize, FsError> {
        if let NodeKind::File(ref data) = self.kind {
            let mut data = data.write();
            if offset as usize > data.len() {
                data.resize(offset as usize, 0);
            }
            if offset as usize + buf.len() > data.len() {
                data.resize(offset as usize + buf.len(), 0);
            }
            data[offset as usize..offset as usize + buf.len()].copy_from_slice(buf);
            Ok(buf.len())
        } else {
            Err(FsError::InvalidArgument)
        }
    }

    fn truncate(&self, size: u64) -> Result<(), FsError> {
        if let NodeKind::File(ref data) = self.kind {
            let mut data = data.write();
            data.resize(size as usize, 0);
            Ok(())
        } else {
            Err(FsError::InvalidArgument)
        }
    }

    fn sync(&self) -> Result<(), FsError> {
        // No-op for in-memory implementation
        Ok(())
    }
}

impl DirOps for Fat32Node {
    fn readdir(&self) -> Result<Vec<DirEntry>, FsError> {
        if let NodeKind::Dir(ref map) = self.kind {
            let map = map.read();
            Ok(map
                .values()
                .map(|node| DirEntry { name: node.name.clone(), inode: 0, file_type: node.node_type() })
                .collect())
        } else {
            Err(FsError::NotDirectory)
        }
    }

    fn lookup(&self, name: &str) -> Result<Arc<dyn VfsNode>, FsError> {
        if let NodeKind::Dir(ref map) = self.kind {
            let map = map.read();
            map.get(name)
                .cloned()
                .map(|n| n as Arc<dyn VfsNode>)
                .ok_or(FsError::NotFound)
        } else {
            Err(FsError::NotDirectory)
        }
    }

    fn create(&self, name: &str, _perms: Permissions) -> Result<Arc<dyn VfsNode>, FsError> {
        if let NodeKind::Dir(ref map) = self.kind {
            let mut map = map.write();
            if map.contains_key(name) {
                return Err(FsError::AlreadyExists);
            }
            let node = Fat32Node::new_file(name);
            map.insert(name.to_string(), node.clone());
            Ok(node)
        } else {
            Err(FsError::NotDirectory)
        }
    }

    fn mkdir(&self, name: &str, _perms: Permissions) -> Result<Arc<dyn VfsNode>, FsError> {
        if let NodeKind::Dir(ref map) = self.kind {
            let mut map = map.write();
            if map.contains_key(name) {
                return Err(FsError::AlreadyExists);
            }
            let node = Fat32Node::new_dir(name);
            map.insert(name.to_string(), node.clone());
            Ok(node)
        } else {
            Err(FsError::NotDirectory)
        }
    }

    fn unlink(&self, name: &str) -> Result<(), FsError> {
        if let NodeKind::Dir(ref map) = self.kind {
            let mut map = map.write();
            map.remove(name).map(|_| ()).ok_or(FsError::NotFound)
        } else {
            Err(FsError::NotDirectory)
        }
    }
}

/// Simplified FAT32 filesystem structure.
pub struct Fat32 {
    #[allow(dead_code)]
    device: Arc<dyn BlockDevice>,
    root: Arc<Fat32Node>,
}

impl Fat32 {
    /// Create new FAT32 instance backed by a block device.
    pub fn new(device: Arc<dyn BlockDevice>) -> Self {
        let root = Fat32Node::new_dir("");
        Fat32 { device, root }
    }
}

impl FilesystemOps for Fat32 {
    fn root(&self) -> Arc<dyn VfsNode> {
        self.root.clone()
    }
}

