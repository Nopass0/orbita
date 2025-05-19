#![no_std]

//! Intel High Definition Audio driver skeleton

use core::fmt;
use x86_64::instructions::port::Port;

/// HDA driver structure
pub struct HdaDriver {
    pub base: u32,
    codecs: [bool; 15],
}

impl HdaDriver {
    /// Create a new HDA driver
    pub fn new(base: u32) -> Self {
        Self {
            base,
            codecs: [false; 15],
        }
    }

    /// Initialize the controller and discover codecs
    pub fn init(&mut self) -> Result<(), HdaError> {
        self.reset_controller()?;
        self.discover_codecs();
        Ok(())
    }

    /// Perform controller reset
    fn reset_controller(&mut self) -> Result<(), HdaError> {
        unsafe {
            let mut gctl = Port::<u32>::new((self.base + 0x08) as u16); // Global Control
            gctl.write(0);
            for _ in 0..1000 {
                if gctl.read() & 0x1 == 0 {
                    break;
                }
                x86_64::instructions::nop();
            }
            gctl.write(1);
            for _ in 0..1000 {
                if gctl.read() & 0x1 == 1 {
                    return Ok(());
                }
                x86_64::instructions::nop();
            }
        }
        Err(HdaError::Timeout)
    }

    /// Detect available codecs
    fn discover_codecs(&mut self) {
        for i in 0..15 {
            let offset = 0x60 + i as u32 * 4;
            let presence = unsafe { Port::<u32>::new((self.base + offset) as u16).read() } & 0x1;
            self.codecs[i] = presence != 0;
        }
    }

    /// Get codec presence information
    pub fn codecs(&self) -> &[bool; 15] {
        &self.codecs
    }

    /// Setup an output stream buffer
    pub fn setup_stream(&self, stream: usize, buffer_addr: u32, length: u32) -> Result<(), HdaError> {
        if stream >= 15 {
            return Err(HdaError::InvalidStream);
        }
        let offset = 0x80 + stream as u32 * 0x20;
        unsafe {
            Port::<u32>::new((self.base + offset + 0x18) as u16).write(buffer_addr);
            Port::<u32>::new((self.base + offset + 0x1C) as u16).write(length);
        }
        Ok(())
    }
}

/// HDA driver errors
#[derive(Debug, Clone, Copy)]
pub enum HdaError {
    Timeout,
    InvalidStream,
}

impl fmt::Display for HdaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HdaError::Timeout => write!(f, "HDA reset timeout"),
            HdaError::InvalidStream => write!(f, "Invalid stream index"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new() {
        let hda = HdaDriver::new(0xF0000000);
        assert_eq!(hda.base, 0xF0000000);
        assert!(!hda.codecs.iter().any(|c| *c));
    }
}
