/// Supported compression algorithms.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum CompressionAlgorithm {
    None,
    Lz4,
    Zstd,
    Deflate,
}

/// Compression aggressiveness.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum CompressionLevel {
    Fastest,
    Balanced,
    Maximum,
}

/// Hook used by the filesystem to compress or decompress data blocks.
pub trait CompressionHook {
    fn algorithm(&self) -> CompressionAlgorithm;
    fn level(&self) -> CompressionLevel;
    fn compress(&self, input: &[u8], output: &mut [u8]) -> Result<usize, ()>;
    fn decompress(&self, input: &[u8], output: &mut [u8]) -> Result<usize, ()>;
}
