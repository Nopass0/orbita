#![no_std]

//! OHCI (Open Host Controller Interface) driver skeleton

use core::fmt;
use crate::drivers::usb::UsbError;

/// OHCI controller structure
pub struct OHCIDriver {
    mem_base: u32,
    initialized: bool,
}

impl OHCIDriver {
    /// Create a new OHCI controller driver
    pub fn new(mem_base: u32) -> Self {
        Self { mem_base, initialized: false }
    }

    /// Initialize the OHCI controller
    pub fn init(&mut self) -> Result<(), UsbError> {
        // Memory mapped initialization would go here
        self.initialized = true;
        Ok(())
    }
}

impl fmt::Debug for OHCIDriver {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OHCIDriver")
            .field("mem_base", &self.mem_base)
            .field("initialized", &self.initialized)
            .finish()
    }
}
