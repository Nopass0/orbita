#![no_std]

//! Template driver for Orbita OS
//! 
//! This is a template for creating new device drivers.
//! Copy this file and modify according to your needs.

use core::fmt;
use x86_64::instructions::port::Port;

/// Driver structure
pub struct TemplateDriver {
    base_port: u16,
    initialized: bool,
}

impl TemplateDriver {
    /// Create a new driver instance
    pub fn new(base_port: u16) -> Self {
        Self {
            base_port,
            initialized: false,
        }
    }

    /// Initialize the device
    pub fn init(&mut self) -> Result<(), DriverError> {
        // Check if device exists
        if !self.detect_device()? {
            return Err(DriverError::DeviceNotFound);
        }

        // Initialize device
        unsafe {
            let mut cmd_port = Port::new(self.base_port);
            // Write initialization commands
            cmd_port.write(0x00u8);
        }

        self.initialized = true;
        Ok(())
    }

    /// Detect if device is present
    fn detect_device(&self) -> Result<bool, DriverError> {
        unsafe {
            let mut status_port = Port::<u8>::new(self.base_port + 1);
            let status = status_port.read();
            
            // Check device signature
            Ok(status == 0xFF)
        }
    }

    /// Read data from device
    pub fn read(&mut self) -> Result<u8, DriverError> {
        if !self.initialized {
            return Err(DriverError::NotInitialized);
        }

        unsafe {
            let mut data_port = Port::new(self.base_port);
            Ok(data_port.read())
        }
    }

    /// Write data to device
    pub fn write(&mut self, data: u8) -> Result<(), DriverError> {
        if !self.initialized {
            return Err(DriverError::NotInitialized);
        }

        unsafe {
            let mut data_port = Port::new(self.base_port);
            data_port.write(data);
        }

        Ok(())
    }
}

/// Driver errors
#[derive(Debug, Clone, Copy)]
pub enum DriverError {
    DeviceNotFound,
    NotInitialized,
    InvalidData,
    Timeout,
}

impl fmt::Display for DriverError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DriverError::DeviceNotFound => write!(f, "Device not found"),
            DriverError::NotInitialized => write!(f, "Driver not initialized"),
            DriverError::InvalidData => write!(f, "Invalid data"),
            DriverError::Timeout => write!(f, "Operation timed out"),
        }
    }
}

// Tests
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_driver() {
        let driver = TemplateDriver::new(0x3F8);
        assert_eq!(driver.base_port, 0x3F8);
        assert!(!driver.initialized);
    }
}