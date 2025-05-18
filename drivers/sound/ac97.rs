#![no_std]

//! AC97 Sound Driver for Orbita OS
//! 
//! Implements basic AC97 audio codec support

use core::fmt;
use x86_64::instructions::port::Port;
use alloc::vec::Vec;

const AC97_RESET: u16 = 0x00;
const AC97_MASTER_VOLUME: u16 = 0x02;
const AC97_PCM_OUT_VOLUME: u16 = 0x18;
const AC97_POWERDOWN: u16 = 0x26;
const AC97_EXTENDED_ID: u16 = 0x28;
const AC97_EXTENDED_STATUS: u16 = 0x2A;
const AC97_PCM_FRONT_DAC_RATE: u16 = 0x2C;

const NAM_BASE: u16 = 0x0;  // Native Audio Mixer
const NABM_BASE: u16 = 0x10; // Native Audio Bus Master

/// AC97 Buffer Descriptor
#[repr(C, packed)]
struct BufferDescriptor {
    addr: u32,
    samples: u16,
    flags: u16,
}

/// AC97 Sound Driver
pub struct AC97Driver {
    nam_base: u16,
    nabm_base: u16,
    initialized: bool,
    buffer_descriptors: Vec<BufferDescriptor>,
}

impl AC97Driver {
    /// Create a new AC97 driver instance
    pub fn new(nam_base: u16, nabm_base: u16) -> Self {
        Self {
            nam_base,
            nabm_base,
            initialized: false,
            buffer_descriptors: Vec::new(),
        }
    }

    /// Initialize the AC97 codec
    pub fn init(&mut self) -> Result<(), SoundError> {
        // Reset codec
        self.reset_codec()?;
        
        // Set volumes
        self.set_master_volume(0x0000)?; // Max volume
        self.set_pcm_volume(0x0808)?;     // 50% volume
        
        // Enable variable rate audio
        let ext_audio = self.read_register(AC97_EXTENDED_ID)?;
        if ext_audio & 0x0001 != 0 {
            // Variable rate PCM supported
            self.write_register(AC97_EXTENDED_STATUS, ext_audio | 0x0001)?;
            
            // Set sample rate to 48kHz
            self.set_sample_rate(48000)?;
        }
        
        self.initialized = true;
        Ok(())
    }

    /// Reset the codec
    fn reset_codec(&mut self) -> Result<(), SoundError> {
        unsafe {
            let mut reset_port = Port::<u16>::new(self.nam_base + AC97_RESET);
            reset_port.write(0);
            
            // Wait for codec ready
            for _ in 0..1000 {
                let status = reset_port.read();
                if status != 0xFFFF {
                    return Ok(());
                }
                x86_64::instructions::nop();
            }
        }
        Err(SoundError::CodecTimeout)
    }

    /// Read a codec register
    fn read_register(&self, reg: u16) -> Result<u16, SoundError> {
        unsafe {
            let mut port = Port::<u16>::new(self.nam_base + reg);
            Ok(port.read())
        }
    }

    /// Write to a codec register
    fn write_register(&mut self, reg: u16, value: u16) -> Result<(), SoundError> {
        unsafe {
            let mut port = Port::<u16>::new(self.nam_base + reg);
            port.write(value);
        }
        Ok(())
    }

    /// Set master volume (0x0000 = max, 0x8080 = mute)
    pub fn set_master_volume(&mut self, volume: u16) -> Result<(), SoundError> {
        self.write_register(AC97_MASTER_VOLUME, volume)
    }

    /// Set PCM output volume
    pub fn set_pcm_volume(&mut self, volume: u16) -> Result<(), SoundError> {
        self.write_register(AC97_PCM_OUT_VOLUME, volume)
    }

    /// Set sample rate
    pub fn set_sample_rate(&mut self, rate: u32) -> Result<(), SoundError> {
        if rate < 8000 || rate > 48000 {
            return Err(SoundError::InvalidSampleRate);
        }
        
        self.write_register(AC97_PCM_FRONT_DAC_RATE, rate as u16)?;
        Ok(())
    }

    /// Play PCM audio data
    pub fn play_audio(&mut self, data: &[u8]) -> Result<(), SoundError> {
        if !self.initialized {
            return Err(SoundError::NotInitialized);
        }

        // Setup buffer descriptors
        // This is simplified - real implementation would use DMA
        
        // Start playback
        unsafe {
            let mut control_port = Port::<u8>::new(self.nabm_base + 0x1B);
            control_port.write(0x01); // Start playback
        }
        
        Ok(())
    }

    /// Stop audio playback
    pub fn stop(&mut self) -> Result<(), SoundError> {
        unsafe {
            let mut control_port = Port::<u8>::new(self.nabm_base + 0x1B);
            control_port.write(0x00); // Stop playback
        }
        Ok(())
    }
}

/// Sound driver errors
#[derive(Debug, Clone, Copy)]
pub enum SoundError {
    CodecTimeout,
    NotInitialized,
    InvalidSampleRate,
    BufferOverflow,
    DMAError,
}

impl fmt::Display for SoundError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SoundError::CodecTimeout => write!(f, "Codec timeout"),
            SoundError::NotInitialized => write!(f, "Driver not initialized"),
            SoundError::InvalidSampleRate => write!(f, "Invalid sample rate"),
            SoundError::BufferOverflow => write!(f, "Buffer overflow"),
            SoundError::DMAError => write!(f, "DMA error"),
        }
    }
}

// Tests
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_driver() {
        let driver = AC97Driver::new(0xE000, 0xE100);
        assert_eq!(driver.nam_base, 0xE000);
        assert_eq!(driver.nabm_base, 0xE100);
        assert!(!driver.initialized);
    }
}