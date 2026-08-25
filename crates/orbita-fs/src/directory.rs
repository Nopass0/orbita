use crate::InodeId;

/// Key used in directory indexes. Names are kept as borrowed UTF-8 bytes.
#[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct DirectoryKey<'a> {
    pub name: &'a str,
}

/// One directory entry on disk.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct DirectoryEntry<'a> {
    pub key: DirectoryKey<'a>,
    pub inode: InodeId,
    pub record: DirectoryRecord,
}

/// Additional metadata stored with a directory entry.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct DirectoryRecord {
    pub file_type_tag: u8,
    pub name_len: u16,
    pub checksum: u32,
}

impl DirectoryRecord {
    /// Builds a compact directory record from the name and file type tag.
    pub fn new(file_type_tag: u8, name_len: u16, checksum: u32) -> Self {
        Self {
            file_type_tag,
            name_len,
            checksum,
        }
    }
}

/// Directory index contract.
///
/// A production backend can use a B-tree, radix tree, or hashed index. The
/// API stays abstract so lookups can be optimized without changing callers.
pub trait DirectoryIndex {
    fn lookup<'a>(&'a self, key: DirectoryKey<'a>) -> Option<DirectoryEntry<'a>>;
    fn insert<'a>(&mut self, entry: DirectoryEntry<'a>) -> bool;
    fn remove<'a>(&mut self, key: DirectoryKey<'a>) -> bool;
    fn len(&self) -> usize;

    /// Returns true when a name exists in the index.
    fn contains<'a>(&'a self, key: DirectoryKey<'a>) -> bool {
        self.lookup(key).is_some()
    }
}
