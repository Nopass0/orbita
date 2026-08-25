#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct MmioRegion {
    pub base: u64,
    pub size: u64,
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct PortIoRange {
    pub base: u16,
    pub size: u16,
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct SharedMemoryWindow {
    pub base: u64,
    pub size: usize,
}
