#![no_std]

//! Intel E1000 Ethernet driver
//!
//! Implements initialization with ring buffers and basic DMA operations.

extern crate alloc;

use alloc::boxed::Box;
use alloc::vec::Vec;
use core::fmt;
use core::ptr::{read_volatile, write_volatile};

/// Number of descriptors in the transmit and receive rings
const TX_RING_SIZE: usize = 16;
const RX_RING_SIZE: usize = 16;
/// Buffer size for each packet
const BUFFER_SIZE: usize = 2048;

/// Transmit descriptor
#[repr(C, packed)]
struct TxDesc {
    addr: u64,
    length: u16,
    cso: u8,
    cmd: u8,
    status: u8,
    css: u8,
    special: u16,
}

/// Receive descriptor
#[repr(C, packed)]
struct RxDesc {
    addr: u64,
    length: u16,
    csum: u16,
    status: u8,
    errors: u8,
    special: u16,
}

/// Intel E1000 network driver
pub struct E1000Driver {
    mmio_base: *mut u32,
    tx_descs: Vec<TxDesc>,
    rx_descs: Vec<RxDesc>,
    tx_buffers: Vec<Box<[u8; BUFFER_SIZE]>>,
    rx_buffers: Vec<Box<[u8; BUFFER_SIZE]>>,
    tx_cur: usize,
    rx_cur: usize,
    initialized: bool,
}

impl E1000Driver {
    /// Create a new driver instance
    pub fn new(mmio_base: usize) -> Self {
        let mut tx_descs = Vec::with_capacity(TX_RING_SIZE);
        let mut rx_descs = Vec::with_capacity(RX_RING_SIZE);
        let mut tx_buffers = Vec::with_capacity(TX_RING_SIZE);
        let mut rx_buffers = Vec::with_capacity(RX_RING_SIZE);

        for _ in 0..TX_RING_SIZE {
            tx_descs.push(TxDesc {
                addr: 0,
                length: 0,
                cso: 0,
                cmd: 0,
                status: 0,
                css: 0,
                special: 0,
            });
            tx_buffers.push(Box::new([0u8; BUFFER_SIZE]));
        }

        for _ in 0..RX_RING_SIZE {
            rx_descs.push(RxDesc {
                addr: 0,
                length: 0,
                csum: 0,
                status: 0,
                errors: 0,
                special: 0,
            });
            rx_buffers.push(Box::new([0u8; BUFFER_SIZE]));
        }

        Self {
            mmio_base: mmio_base as *mut u32,
            tx_descs,
            rx_descs,
            tx_buffers,
            rx_buffers,
            tx_cur: 0,
            rx_cur: 0,
            initialized: false,
        }
    }

    /// Initialize the device and DMA rings
    pub fn init(&mut self) {
        unsafe {
            // Reset device
            self.write_reg(0x0000, 0x04000000);
            self.write_reg(0x0000, 0x00000000);

            // Initialize transmit ring
            for (i, buf) in self.tx_buffers.iter_mut().enumerate() {
                self.tx_descs[i].addr = buf.as_ptr() as u64;
                self.tx_descs[i].status = 0x1;
            }
            let tx_base = self.tx_descs.as_ptr() as u64;
            self.write_reg(0x03800, (tx_base & 0xFFFF_FFFF) as u32);
            self.write_reg(0x03804, (tx_base >> 32) as u32);
            self.write_reg(0x03808, (TX_RING_SIZE * core::mem::size_of::<TxDesc>()) as u32);
            self.write_reg(0x03810, 0);
            self.write_reg(0x03818, 0);

            // Initialize receive ring
            for (i, buf) in self.rx_buffers.iter_mut().enumerate() {
                self.rx_descs[i].addr = buf.as_ptr() as u64;
            }
            let rx_base = self.rx_descs.as_ptr() as u64;
            self.write_reg(0x02800, (rx_base & 0xFFFF_FFFF) as u32);
            self.write_reg(0x02804, (rx_base >> 32) as u32);
            self.write_reg(0x02808, (RX_RING_SIZE * core::mem::size_of::<RxDesc>()) as u32);
            self.write_reg(0x02810, 0);
            self.write_reg(0x02818, (RX_RING_SIZE as u32) - 1);

            // Enable transmitter and receiver
            self.write_reg(0x00400, 0x0000000C);
            self.write_reg(0x0100, 0x00000002);
        }
        self.initialized = true;
    }

    /// Send an Ethernet frame
    pub fn send_packet(&mut self, data: &[u8]) -> Result<(), NetError> {
        if !self.initialized {
            return Err(NetError::NotInitialized);
        }
        if data.len() > BUFFER_SIZE {
            return Err(NetError::BufferTooSmall);
        }

        let idx = self.tx_cur % TX_RING_SIZE;
        let buf = &mut self.tx_buffers[idx];
        buf[..data.len()].copy_from_slice(data);

        self.tx_descs[idx].length = data.len() as u16;
        self.tx_descs[idx].cmd = 0b0000_1011; // EOP + IFCS + RS
        self.tx_descs[idx].status = 0;

        self.tx_cur = (self.tx_cur + 1) % TX_RING_SIZE;
        unsafe {
            self.write_reg(0x03818, self.tx_cur as u32);
        }
        Ok(())
    }

    /// Receive an Ethernet frame
    pub fn receive_packet(&mut self, buffer: &mut [u8]) -> Result<usize, NetError> {
        if !self.initialized {
            return Err(NetError::NotInitialized);
        }

        let idx = self.rx_cur % RX_RING_SIZE;
        if self.rx_descs[idx].status & 0x01 == 0 {
            return Err(NetError::NoPacket);
        }

        let length = self.rx_descs[idx].length as usize;
        if length > buffer.len() {
            return Err(NetError::BufferTooSmall);
        }

        buffer[..length].copy_from_slice(&self.rx_buffers[idx][..length]);
        self.rx_descs[idx].status = 0;
        self.rx_cur = (self.rx_cur + 1) % RX_RING_SIZE;
        unsafe {
            self.write_reg(0x02818, ((self.rx_cur + RX_RING_SIZE - 1) % RX_RING_SIZE) as u32);
        }
        Ok(length)
    }

    /// Handle an interrupt from the device
    pub fn handle_interrupt(&mut self) {
        if !self.initialized {
            return;
        }

        unsafe {
            let icr = self.read_reg(0x000C);
            // Acknowledge interrupts by writing back the value
            self.write_reg(0x000C, icr);
        }
    }

    #[inline]
    unsafe fn read_reg(&self, offset: u32) -> u32 {
        let ptr = self.mmio_base.add((offset / 4) as usize);
        read_volatile(ptr)
    }

    #[inline]
    unsafe fn write_reg(&self, offset: u32, value: u32) {
        let ptr = self.mmio_base.add((offset / 4) as usize);
        write_volatile(ptr, value);
    }
}

/// Network driver errors
#[derive(Debug, Clone, Copy)]
pub enum NetError {
    NotInitialized,
    BufferTooSmall,
    NoPacket,
}

impl fmt::Display for NetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NetError::NotInitialized => write!(f, "Driver not initialized"),
            NetError::BufferTooSmall => write!(f, "Buffer too small"),
            NetError::NoPacket => write!(f, "No packet available"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_driver() {
        let driver = E1000Driver::new(0xFEC00000);
        assert_eq!(driver.mmio_base as usize, 0xFEC00000);
        assert!(!driver.initialized);
    }
}
