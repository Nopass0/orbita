use crate::{InodeId, InodeMetadata};

/// Kernel-facing handle to an object inside the filesystem namespace.
#[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct FsObjectHandle(pub u64);

/// Object classification used by higher layers.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum ObjectKind {
    File,
    Directory,
    Symlink,
    Device,
}

/// Object attributes that can be requested from the VFS layer.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct ObjectAttributes {
    pub kind: ObjectKind,
    pub inode: InodeId,
    pub metadata: InodeMetadata,
}

/// Stable object record that a VFS can keep in caches.
#[derive(Debug, Clone)]
pub struct FsObject {
    pub handle: FsObjectHandle,
    pub attributes: ObjectAttributes,
}
