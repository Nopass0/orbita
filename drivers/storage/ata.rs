#![no_std]

//! ATA/ATAPI Driver for Orbita OS
//!
//! Provides disk detection, sector read/write and DMA skeleton.

use core::fmt;
use x86_64::instructions::port::Port;

/// Represents an ATA controller on a legacy IDE bus.
pub struct AtaController {
    pub io_base: u16,
    pub control_base: u16,
    bus_master_base: Option<u16>,
}

impl AtaController {
    /// Create a new controller instance with the given I/O ports.
    pub const fn new(io_base: u16, control_base: u16) -> Self {
        Self { io_base, control_base, bus_master_base: None }
    }

    /// Detect drive presence using the IDENTIFY command.
    pub fn detect(&mut self) -> Result<bool, AtaError> {
        unsafe {
            let mut drive_head = Port::<u8>::new(self.io_base + 6);
            drive_head.write(0xA0); // Master drive

            let mut status = Port::<u8>::new(self.io_base + 7);
            status.write(0xEC); // IDENTIFY

            // Wait for BSY to clear
            for _ in 0..100000 {
                let s = status.read();
                if s & 0x80 == 0 {
                    return Ok(s != 0);
                }
            }
        }
        Err(AtaError::Timeout)
    }

    /// Read a single 512-byte sector using PIO.
    pub fn read_sector(&mut self, lba: u32, buffer: &mut [u8]) -> Result<(), AtaError> {
        if buffer.len() < 512 {
            return Err(AtaError::BufferTooSmall);
        }
        unsafe {
            let mut sector_count = Port::<u8>::new(self.io_base + 2);
            let mut lba_low = Port::<u8>::new(self.io_base + 3);
            let mut lba_mid = Port::<u8>::new(self.io_base + 4);
            let mut lba_high = Port::<u8>::new(self.io_base + 5);
            let mut drive_head = Port::<u8>::new(self.io_base + 6);
            let mut command = Port::<u8>::new(self.io_base + 7);

            sector_count.write(1);
            lba_low.write((lba & 0xFF) as u8);
            lba_mid.write(((lba >> 8) & 0xFF) as u8);
            lba_high.write(((lba >> 16) & 0xFF) as u8);
            drive_head.write(0xE0 | (((lba >> 24) & 0x0F) as u8));
            command.write(0x20); // READ SECTOR

            let mut status = Port::<u8>::new(self.io_base + 7);
            for _ in 0..100000 {
                let s = status.read();
                if s & 0x08 != 0 {
                    break;
                }
            }

            let mut data = Port::<u16>::new(self.io_base);
            for i in 0..256 {
                let value: u16 = data.read();
                let bytes = value.to_le_bytes();
                buffer[i * 2] = bytes[0];
                buffer[i * 2 + 1] = bytes[1];
            }
        }
        Ok(())
    }

    /// Write a single 512-byte sector using PIO.
    pub fn write_sector(&mut self, lba: u32, buffer: &[u8]) -> Result<(), AtaError> {
        if buffer.len() < 512 {
            return Err(AtaError::BufferTooSmall);
        }
        unsafe {
            let mut sector_count = Port::<u8>::new(self.io_base + 2);
            let mut lba_low = Port::<u8>::new(self.io_base + 3);
            let mut lba_mid = Port::<u8>::new(self.io_base + 4);
            let mut lba_high = Port::<u8>::new(self.io_base + 5);
            let mut drive_head = Port::<u8>::new(self.io_base + 6);
            let mut command = Port::<u8>::new(self.io_base + 7);

            sector_count.write(1);
            lba_low.write((lba & 0xFF) as u8);
            lba_mid.write(((lba >> 8) & 0xFF) as u8);
            lba_high.write(((lba >> 16) & 0xFF) as u8);
            drive_head.write(0xE0 | (((lba >> 24) & 0x0F) as u8));
            command.write(0x30); // WRITE SECTOR

            let mut data = Port::<u16>::new(self.io_base);
            for i in 0..256 {
                let lo = buffer[i * 2] as u16;
                let hi = buffer[i * 2 + 1] as u16;
                data.write((hi << 8) | lo);
            }
        }
        Ok(())
    }

    /// Setup Bus Master IDE registers for DMA transfers.
    pub fn setup_dma(&mut self, bus_master_base: u16) {
        self.bus_master_base = Some(bus_master_base);
    }

    /// Read multiple sectors via DMA (skeleton).
    pub fn read_dma(&mut self, _lba: u32, _sectors: u16, _buffer: &mut [u8]) -> Result<(), AtaError> {
        // TODO: Implement DMA transfer logic
        Ok(())
    }

    /// Write multiple sectors via DMA (skeleton).
    pub fn write_dma(&mut self, _lba: u32, _sectors: u16, _buffer: &[u8]) -> Result<(), AtaError> {
        // TODO: Implement DMA transfer logic
        Ok(())
    }
}

/// Errors returned by the ATA driver.
#[derive(Debug, Clone, Copy)]
pub enum AtaError {
    DeviceNotFound,
    InvalidLba,
    BufferTooSmall,
    Timeout,
}

impl fmt::Display for AtaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AtaError::DeviceNotFound => write!(f, "ATA device not found"),
            AtaError::InvalidLba => write!(f, "Invalid LBA"),
            AtaError::BufferTooSmall => write!(f, "Buffer too small"),
            AtaError::Timeout => write!(f, "Operation timed out"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_controller() {
        let ctrl = AtaController::new(0x1F0, 0x3F6);
        assert_eq!(ctrl.io_base, 0x1F0);
        assert_eq!(ctrl.control_base, 0x3F6);
    }
}
