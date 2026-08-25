#![no_std]

//! Hardware primitives shared by Orbita kernel and driver layers.
//!
//! This crate stays intentionally low-level: MMIO ranges, IRQ lines, APIC
//! identities, PCI addresses, and timer metadata live here so the rest of the
//! workspace can talk about hardware without binding to one backend.

extern crate alloc;

mod ahci;
mod apic;
mod e1000;
mod irq;
mod pci;
mod ps2;
mod smp;
mod timer;
mod transport;

pub use ahci::{AhciDisk, AhciError, StorageKind, set_debug_hook};
pub use e1000::{E1000, E1000Error, E1000Stats};
pub use apic::{
    ApicId, ApicKind, CpuLocalId, InterruptBootstrap, InterruptController,
    InterruptDeliveryMode, IoApicInfo, IoApicRoute, LocalApicInfo, bootstrap_interrupts,
    probe_io_apic, probe_local_apic,
};
pub use irq::{
    IRQ_BASE_VECTOR, IdtInstallReport, InterruptLine, IrqEdge, IrqPolarity, KEYBOARD_VECTOR,
    SPURIOUS_VECTOR, TIMER_VECTOR, dispatch, install_bootstrap_idt, register_handler,
};
pub use pci::{enable_bus_master, write_config_u32,
    PciAddress, PciBar, PciBarKind, PciClassCode, PciDevice, PciDeviceId, PciInventory,
    PciVendorId,
};
pub use ps2::{
    Ps2Status, initialize_keyboard, poll_data as poll_ps2_data, probe_controller as probe_ps2_controller,
};
pub use smp::{SmpInfo, probe as probe_smp};
pub use timer::{
    ClockSource, LapicTimerState, TimerPlan, TimerTick, TimerTopology, bootstrap_plan,
    prepare_lapic_timer,
};
pub use transport::{MmioRegion, PortIoRange, SharedMemoryWindow};
