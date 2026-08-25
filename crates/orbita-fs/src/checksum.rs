/// Supported checksum algorithms.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum ChecksumAlgorithm {
    Crc32,
    Crc64,
    Blake3,
}

/// Digest produced by the checksum hook.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct ChecksumDigest {
    pub algorithm: ChecksumAlgorithm,
    pub bytes: [u8; 32],
}

/// Hook used by the filesystem to validate metadata or file payloads.
pub trait ChecksumHook {
    fn algorithm(&self) -> ChecksumAlgorithm;
    fn digest(&self, data: &[u8]) -> ChecksumDigest;
    fn verify(&self, data: &[u8], expected: &ChecksumDigest) -> bool;
}
