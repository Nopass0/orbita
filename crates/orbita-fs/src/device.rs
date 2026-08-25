use crate::{BlockAddress, BlockSize};

/// Stable geometry information for a block device.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct BlockDeviceGeometry {
    pub block_size: BlockSize,
    pub block_count: u64,
}

impl BlockDeviceGeometry {
    /// Returns the total device capacity in bytes.
    pub fn capacity_bytes(self) -> u64 {
        self.block_size.0 as u64 * self.block_count
    }
}

/// Runtime statistics collected from the storage backend.
#[derive(Debug, Copy, Clone, Eq, PartialEq, Default)]
pub struct BlockDeviceStats {
    pub read_ops: u64,
    pub write_ops: u64,
    pub flush_ops: u64,
}

impl BlockDeviceStats {
    /// Returns the total number of recorded I/O operations.
    pub fn total_ops(self) -> u64 {
        self.read_ops + self.write_ops + self.flush_ops
    }
}

/// Read/write errors surfaced by a block backend.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum BlockDeviceError {
    OutOfBounds,
    Unsupported,
    IoError,
    NotReady,
}

/// High-level block device identity and geometry.
pub trait BlockDeviceInfo {
    fn geometry(&self) -> BlockDeviceGeometry;
    fn stats(&self) -> BlockDeviceStats {
        BlockDeviceStats::default()
    }
}

/// Block request kind used by storage backends.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum BlockRequestKind {
    Read,
    Write,
    Flush,
}

/// Request descriptor for block I/O.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct BlockRequest {
    pub kind: BlockRequestKind,
    pub start: BlockAddress,
    pub blocks: u64,
}

/// Result descriptor for block I/O.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct BlockResponse {
    pub completed_blocks: u64,
}

/// Minimal block-facing interface for Orbita FS.
pub trait BlockDevice: BlockDeviceInfo {
    fn read_blocks(&mut self, start: BlockAddress, blocks: u64, dst: &mut [u8])
        -> Result<BlockResponse, BlockDeviceError>;

    fn write_blocks(&mut self, start: BlockAddress, blocks: u64, src: &[u8])
        -> Result<BlockResponse, BlockDeviceError>;

    fn flush(&mut self) -> Result<(), BlockDeviceError>;
}
