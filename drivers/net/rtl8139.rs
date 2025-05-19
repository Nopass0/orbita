#![no_std]

//! RTL8139 Ethernet driver
//!
//! Provides basic initialization, packet send/receive and interrupt handling for
//! the Realtek RTL8139 network card.

extern crate alloc;

use alloc::boxed::Box;
use core::fmt;
use core::ptr;
use x86_64::instructions::port::Port;

/// Size of the receive buffer
const RX_BUFFER_SIZE: usize = 8192 + 16 + 1500;
/// Size of each transmit buffer
const TX_BUFFER_SIZE: usize = 1792;

/// RTL8139 device driver
pub struct RTL8139Driver {
    io_base: u16,
    irq: u8,
    rx_buffer: Box<[u8; RX_BUFFER_SIZE]>,
    tx_buffers: [Box<[u8; TX_BUFFER_SIZE]>; 4],
    cur_tx: usize,
    cur_rx: usize,
    initialized: bool,
}

impl RTL8139Driver {
    /// Create new driver instance
    pub fn new(io_base: u16, irq: u8) -> Self {
        Self {
            io_base,
            irq,
            rx_buffer: Box::new([0u8; RX_BUFFER_SIZE]),
            tx_buffers: [
                Box::new([0u8; TX_BUFFER_SIZE]),
                Box::new([0u8; TX_BUFFER_SIZE]),
                Box::new([0u8; TX_BUFFER_SIZE]),
                Box::new([0u8; TX_BUFFER_SIZE]),
            ],
            cur_tx: 0,
            cur_rx: 0,
            initialized: false,
        }
    }

    /// Initialize the network card
    pub fn init(&mut self) -> Result<(), NetError> {
        unsafe {
            // Issue software reset
            let mut cmd_port = Port::<u8>::new(self.io_base + 0x37);
            cmd_port.write(0x10);
            // Wait for reset completion
            for _ in 0..1000 {
                if cmd_port.read() & 0x10 == 0 {
                    break;
                }
                x86_64::instructions::nop();
            }

            // Set up receive buffer
            let rx_buf_addr = self.rx_buffer.as_ptr() as u32;
            let mut rbstart = Port::<u32>::new(self.io_base + 0x30);
            rbstart.write(rx_buf_addr);

            // Enable receiver and transmitter
            cmd_port.write(0x0C);

            // Enable interrupts (receive OK and transmit OK)
            let mut imr = Port::<u16>::new(self.io_base + 0x3C);
            imr.write(0x0005);
        }
        self.initialized = true;
        Ok(())
    }

    /// Send an Ethernet frame
    pub fn send_packet(&mut self, data: &[u8]) -> Result<(), NetError> {
        if !self.initialized {
            return Err(NetError::NotInitialized);
        }
        if data.len() > TX_BUFFER_SIZE {
            return Err(NetError::BufferTooSmall);
        }

        let tx_index = self.cur_tx % 4;
        let buf = &mut self.tx_buffers[tx_index];
        buf[..data.len()].copy_from_slice(data);

        unsafe {
            let tx_addr_port = Port::<u32>::new(self.io_base + 0x20 + (tx_index as u16 * 4));
            tx_addr_port.write(buf.as_ptr() as u32);
            let tx_status_port = Port::<u32>::new(self.io_base + 0x10 + (tx_index as u16 * 4));
            tx_status_port.write(data.len() as u32 & 0x1FFF);
        }

        self.cur_tx = (self.cur_tx + 1) % 4;
        Ok(())
    }

    /// Receive an Ethernet frame into the provided buffer
    pub fn receive_packet(&mut self, buffer: &mut [u8]) -> Result<usize, NetError> {
        if !self.initialized {
            return Err(NetError::NotInitialized);
        }

        // Simplified: In real driver we would check the RX buffer head and tail
        // pointers and handle ring wrapping. Here we just copy from the buffer.
        let length_port = Port::<u16>::new(self.io_base + 0x1E);
        let length: u16 = unsafe { length_port.read() };
        if length as usize > buffer.len() {
            return Err(NetError::BufferTooSmall);
        }
        unsafe {
            ptr::copy_nonoverlapping(
                self.rx_buffer.as_ptr().add(self.cur_rx),
                buffer.as_mut_ptr(),
                length as usize,
            );
        }
        self.cur_rx = (self.cur_rx + length as usize + 4) % RX_BUFFER_SIZE;
        Ok(length as usize)
    }

    /// Handle an interrupt from the network card
    pub fn handle_interrupt(&mut self) {
        if !self.initialized {
            return;
        }

        unsafe {
            let mut isr = Port::<u16>::new(self.io_base + 0x3E);
            let status = isr.read();
            // Acknowledge handled interrupts
            isr.write(status);
        }
    }
}

/// Network driver errors
#[derive(Debug, Clone, Copy)]
pub enum NetError {
    NotInitialized,
    BufferTooSmall,
}

impl fmt::Display for NetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NetError::NotInitialized => write!(f, "Driver not initialized"),
            NetError::BufferTooSmall => write!(f, "Buffer too small"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_driver() {
        let driver = RTL8139Driver::new(0xC000, 10);
        assert_eq!(driver.io_base, 0xC000);
        assert_eq!(driver.irq, 10);
        assert!(!driver.initialized);
    }
}
