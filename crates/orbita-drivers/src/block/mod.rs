//! Block transport registry and PCI matchers.
//!
//! The goal is to identify storage controllers from a PCI inventory snapshot
//! and attach the correct backend contract without coupling the registry to a
//! concrete kernel driver implementation.

mod candidate;
mod pci;
mod registry;

pub use candidate::{
    BlockCandidate, BlockCandidateMatch, BlockPciClassCode, BlockPciSnapshot, BlockTransportKind,
};
pub use pci::{BlockPciClassPattern, BlockPciDeviceKind};
pub use registry::{AttachedBlockDevice, BlockTransportRegistry, BlockTransportSummary};
