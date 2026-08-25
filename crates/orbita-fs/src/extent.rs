/// File offsets and extent tree indexing are expressed in bytes.
#[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct FileOffset(pub u64);

/// A stable key used by the extent tree for binary search and merge logic.
#[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct ExtentKey {
    pub offset: FileOffset,
}

/// Flags that describe how an extent should be interpreted.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct ExtentFlags(pub u32);

impl ExtentFlags {
    pub const HOLE: Self = Self(1 << 0);
    pub const WRITTEN: Self = Self(1 << 1);
    pub const DIRTY: Self = Self(1 << 2);
    pub const COW: Self = Self(1 << 3);
    pub const COMPRESSED: Self = Self(1 << 4);
}

/// One mapping from file bytes to physical blocks.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct Extent {
    pub key: ExtentKey,
    pub physical_start: u64,
    pub length_bytes: u64,
    pub allocated_blocks: u64,
    pub flags: ExtentFlags,
}

impl Extent {
    pub fn end_offset(&self) -> FileOffset {
        FileOffset(self.key.offset.0 + self.length_bytes)
    }
}

/// Node type used by the extent tree. This keeps the API backend-agnostic.
#[derive(Debug, Clone)]
pub struct ExtentNode {
    pub extents: alloc::vec::Vec<Extent>,
}

/// The extent tree manages sparse file mappings and merge/split behavior.
#[derive(Debug, Clone)]
pub struct ExtentTree {
    pub root: Option<ExtentNode>,
    pub height: u8,
}

impl ExtentTree {
    pub fn empty() -> Self {
        Self {
            root: None,
            height: 0,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.root.is_none()
    }
}
