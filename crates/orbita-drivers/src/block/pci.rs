use crate::block::candidate::BlockPciClassCode;

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum BlockPciDeviceKind {
    Controller,
    Bridge,
    Endpoint,
    Unknown,
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct BlockPciClassPattern {
    pub kind: BlockPciDeviceKind,
    pub class_code: BlockPciClassCode,
}
