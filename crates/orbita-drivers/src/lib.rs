#![no_std]
//! The Orbita driver platform.
//!
//! - `driver` — the [`Driver`](driver::Driver) contract (probe → attach
//!   → start) and the dynamic `DriverManager`
//!   that binds discovered devices to drivers.
//! - [`manager`] — PCI classification/inventory (`DeviceManager`).
//! - - `block` — storage transport matching metadata.
//! - - `monitor` — attached-monitor bridging model.
//! - - `domains` — the static maturity catalog of planned/known drivers.

extern crate alloc;

mod device;
pub mod block;
pub mod driver;
mod domains;
pub mod manager;
pub mod monitor;
mod registry;

pub use device::{DeviceClass, DriverDescriptor, DriverMaturity};
pub use driver::{BindRecord, BindReport, BindStatus, DeviceProbe, Driver as DriverTrait, DriverManager};
pub use manager::{DeviceManager, PciObservation, SystemDevice, SystemDeviceKind};
pub use monitor::{MonitorDevice, MonitorList, MonitorSource};
pub use block::{
    BlockCandidate, BlockCandidateMatch, BlockPciSnapshot, BlockTransportKind,
    BlockTransportRegistry, BlockTransportSummary,
};
pub use domains::{
    builtin_block_drivers, builtin_gpu_drivers, builtin_input_drivers, builtin_net_drivers,
    builtin_sound_drivers, builtin_storage_drivers,
};
pub use registry::{DriverRegistry, RegistrySummary};
