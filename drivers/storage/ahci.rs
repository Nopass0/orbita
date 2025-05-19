#![no_std]

//! AHCI (SATA) driver for Orbita OS
//!
//! Provides controller initialization, port discovery and NCQ command skeletons.

use core::fmt;
use bit_field::BitField;

/// Host Bus Adapter memory structure (simplified).
#[repr(C)]
pub struct HbaMem {
    pub host_cap: u32,
    pub global_host_control: u32,
    pub interrupt_status: u32,
    pub ports_implemented: u32,
    _reserved: [u32; 11],
    pub ports: [HbaPort; 32],
}

/// AHCI Port registers (simplified).
#[repr(C)]
pub struct HbaPort {
    pub command_list_base: u64,
    pub fis_base: u64,
    pub interrupt_status: u32,
    pub interrupt_enable: u32,
    pub command_and_status: u32,
    pub reserved: u32,
    pub task_file_data: u32,
    pub signature: u32,
    pub sata_status: u32,
    pub sata_control: u32,
    pub sata_error: u32,
    pub sata_active: u32,
    pub command_issue: u32,
    pub sata_notification: u32,
    pub fis_switch_control: u32,
    _reserved2: [u32; 11],
}

/// AHCI controller abstraction.
pub struct AhciController {
    hba: &'static mut HbaMem,
}

impl AhciController {
    /// Create controller from a memory mapped address.
    ///
    /// # Safety
    /// Caller must ensure the address contains valid HBA registers.
    pub unsafe fn new(hba_address: usize) -> Self {
        Self { hba: &mut *(hba_address as *mut HbaMem) }
    }

    /// Initialize AHCI mode.
    pub fn init(&mut self) {
        unsafe {
            // Set AHCI enable bit
            self.hba.global_host_control.set_bit(31, true);
        }
    }

    /// Return a bitmap of implemented ports.
    pub fn discover_ports(&self) -> u32 {
        self.hba.ports_implemented
    }

    /// Read sectors using a normal command (skeleton).
    pub fn read(&mut self, _port: usize, _lba: u64, _buffer: &mut [u8]) -> Result<(), AhciError> {
        // TODO: Implement FIS based read
        Ok(())
    }

    /// Write sectors using a normal command (skeleton).
    pub fn write(&mut self, _port: usize, _lba: u64, _buffer: &[u8]) -> Result<(), AhciError> {
        // TODO: Implement FIS based write
        Ok(())
    }

    /// Issue an NCQ command (skeleton).
    pub fn read_ncq(&mut self, _port: usize, _tag: u8, _lba: u64, _buffer: &mut [u8]) -> Result<(), AhciError> {
        // TODO: Implement NCQ support
        Ok(())
    }
}

#[derive(Debug, Clone, Copy)]
pub enum AhciError {
    NoPort,
    CommandFailed,
}

impl fmt::Display for AhciError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AhciError::NoPort => write!(f, "Port not available"),
            AhciError::CommandFailed => write!(f, "Command failed"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_port_bitmap() {
        let mem = HbaMem {
            host_cap: 0,
            global_host_control: 0,
            interrupt_status: 0,
            ports_implemented: 0x5,
            _reserved: [0; 11],
            ports: unsafe { core::mem::zeroed() },
        };
        let controller = AhciController { hba: unsafe { &mut *( &mem as *const _ as *mut HbaMem ) } };
        assert_eq!(controller.discover_ports(), 0x5);
    }
}
