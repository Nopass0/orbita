# Drivers Implementation Guide

## Overview
The drivers module provides hardware abstraction for various devices including PCI, timers, keyboard, and sound devices.

## Module Structure

### 1. PCI Driver (pci.rs)

```rust
//! PCI bus driver
//! Enumerates and manages PCI devices

use alloc::vec::Vec;
use spin::Mutex;
use x86_64::instructions::port::{Port, PortReadOnly, PortWriteOnly};

/// PCI configuration space ports
const CONFIG_ADDRESS: u16 = 0xCF8;
const CONFIG_DATA: u16 = 0xCFC;

/// PCI device identification
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PciDeviceId {
    pub vendor: u16,
    pub device: u16,
}

/// PCI device location
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PciLocation {
    pub bus: u8,
    pub device: u8,
    pub function: u8,
}

impl PciLocation {
    /// Create a configuration address
    fn config_address(&self, offset: u8) -> u32 {
        let bus = self.bus as u32;
        let device = (self.device as u32) << 11;
        let function = (self.function as u32) << 8;
        let offset = (offset as u32) & 0xFC;
        
        0x80000000 | (bus << 16) | device | function | offset
    }
}

/// PCI device class codes
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PciClass {
    pub class: u8,
    pub subclass: u8,
    pub prog_if: u8,
}

impl PciClass {
    /// Check if this is a known device type
    pub fn device_type(&self) -> Option<DeviceType> {
        match (self.class, self.subclass) {
            (0x01, 0x01) => Some(DeviceType::IdeController),
            (0x01, 0x06) => Some(DeviceType::SataController),
            (0x02, 0x00) => Some(DeviceType::EthernetController),
            (0x03, 0x00) => Some(DeviceType::VgaController),
            (0x04, 0x01) => Some(DeviceType::AudioDevice),
            (0x06, 0x04) => Some(DeviceType::PciBridge),
            (0x0C, 0x03) => Some(DeviceType::UsbController),
            _ => None,
        }
    }
}

/// Known device types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceType {
    IdeController,
    SataController,
    EthernetController,
    VgaController,
    AudioDevice,
    PciBridge,
    UsbController,
}

/// PCI Base Address Register
#[derive(Debug, Clone, Copy)]
pub enum Bar {
    Memory32 { address: u32, size: u32, prefetchable: bool },
    Memory64 { address: u64, size: u64, prefetchable: bool },
    Io { port: u16, size: u16 },
}

/// PCI device information
#[derive(Debug, Clone)]
pub struct PciDevice {
    pub location: PciLocation,
    pub id: PciDeviceId,
    pub class: PciClass,
    pub bars: [Option<Bar>; 6],
    pub interrupt_line: u8,
    pub interrupt_pin: u8,
}

impl PciDevice {
    /// Read a configuration register
    pub fn read_config(&self, offset: u8) -> u32 {
        unsafe {
            let mut addr_port = Port::new(CONFIG_ADDRESS);
            let mut data_port = Port::new(CONFIG_DATA);
            
            addr_port.write(self.location.config_address(offset));
            data_port.read()
        }
    }
    
    /// Write a configuration register
    pub fn write_config(&self, offset: u8, value: u32) {
        unsafe {
            let mut addr_port = Port::new(CONFIG_ADDRESS);
            let mut data_port = Port::new(CONFIG_DATA);
            
            addr_port.write(self.location.config_address(offset));
            data_port.write(value);
        }
    }
    
    /// Read a BAR (Base Address Register)
    fn read_bar(&self, bar_index: usize) -> Option<Bar> {
        if bar_index >= 6 {
            return None;
        }
        
        let offset = 0x10 + (bar_index as u8) * 4;
        let bar_value = self.read_config(offset);
        
        if bar_value == 0 {
            return None;
        }
        
        // Memory or I/O?
        if bar_value & 1 == 0 {
            // Memory BAR
            let prefetchable = (bar_value & 0x08) != 0;
            let is_64bit = (bar_value & 0x06) == 0x04;
            
            // Determine size by writing all 1s and reading back
            self.write_config(offset, 0xFFFFFFFF);
            let size_mask = self.read_config(offset);
            self.write_config(offset, bar_value); // Restore original value
            
            if is_64bit {
                let bar_high = self.read_config(offset + 4);
                let address = ((bar_high as u64) << 32) | ((bar_value & 0xFFFFFFF0) as u64);
                
                // Get high part of size
                self.write_config(offset + 4, 0xFFFFFFFF);
                let size_mask_high = self.read_config(offset + 4);
                self.write_config(offset + 4, bar_high);
                
                let size = (((size_mask_high as u64) << 32) | (size_mask & 0xFFFFFFF0) as u64)
                    .wrapping_neg() & !0xF;
                
                Some(Bar::Memory64 { address, size, prefetchable })
            } else {
                let address = bar_value & 0xFFFFFFF0;
                let size = (size_mask & 0xFFFFFFF0).wrapping_neg() & !0xF;
                
                Some(Bar::Memory32 { address, size, prefetchable })
            }
        } else {
            // I/O BAR
            let port = (bar_value & 0xFFFC) as u16;
            
            // Determine size
            self.write_config(offset, 0xFFFFFFFF);
            let size_mask = self.read_config(offset);
            self.write_config(offset, bar_value);
            
            let size = ((size_mask & 0xFFFC) as u16).wrapping_neg() & !0x3;
            
            Some(Bar::Io { port, size })
        }
    }
    
    /// Enable bus mastering for DMA
    pub fn enable_bus_mastering(&self) {
        let command = self.read_config(0x04);
        self.write_config(0x04, command | 0x04);
    }
    
    /// Enable memory space access
    pub fn enable_memory_space(&self) {
        let command = self.read_config(0x04);
        self.write_config(0x04, command | 0x02);
    }
    
    /// Enable I/O space access
    pub fn enable_io_space(&self) {
        let command = self.read_config(0x04);
        self.write_config(0x04, command | 0x01);
    }
}

/// PCI bus driver
pub struct PciBus {
    devices: Vec<PciDevice>,
}

impl PciBus {
    /// Create a new PCI bus instance
    pub fn new() -> Self {
        Self {
            devices: Vec::new(),
        }
    }
    
    /// Scan the PCI bus for devices
    pub fn scan(&mut self) {
        self.devices.clear();
        
        for bus in 0..=255 {
            for device in 0..32 {
                for function in 0..8 {
                    let location = PciLocation { bus, device, function };
                    
                    if let Some(device) = self.probe_device(location) {
                        self.devices.push(device);
                        
                        // If this is function 0 and not multifunction, skip other functions
                        if function == 0 {
                            let header_type = (device.read_config(0x0C) >> 16) as u8;
                            if header_type & 0x80 == 0 {
                                break;
                            }
                        }
                    } else if function == 0 {
                        // No device at function 0, skip other functions
                        break;
                    }
                }
            }
        }
    }
    
    /// Probe a specific device location
    fn probe_device(&self, location: PciLocation) -> Option<PciDevice> {
        // Read vendor ID
        let vendor_id = unsafe {
            let mut addr_port = Port::new(CONFIG_ADDRESS);
            let mut data_port: Port<u32> = Port::new(CONFIG_DATA);
            
            addr_port.write(location.config_address(0));
            data_port.read() as u16
        };
        
        if vendor_id == 0xFFFF || vendor_id == 0x0000 {
            return None;
        }
        
        // Read device ID
        let device_id = unsafe {
            let mut addr_port = Port::new(CONFIG_ADDRESS);
            let mut data_port: Port<u32> = Port::new(CONFIG_DATA);
            
            addr_port.write(location.config_address(0));
            (data_port.read() >> 16) as u16
        };
        
        // Read class codes
        let class_code = unsafe {
            let mut addr_port = Port::new(CONFIG_ADDRESS);
            let mut data_port: Port<u32> = Port::new(CONFIG_DATA);
            
            addr_port.write(location.config_address(0x08));
            data_port.read()
        };
        
        let class = PciClass {
            class: (class_code >> 24) as u8,
            subclass: (class_code >> 16) as u8,
            prog_if: (class_code >> 8) as u8,
        };
        
        // Read interrupt information
        let interrupt_info = unsafe {
            let mut addr_port = Port::new(CONFIG_ADDRESS);
            let mut data_port: Port<u32> = Port::new(CONFIG_DATA);
            
            addr_port.write(location.config_address(0x3C));
            data_port.read()
        };
        
        let interrupt_line = interrupt_info as u8;
        let interrupt_pin = (interrupt_info >> 8) as u8;
        
        let mut device = PciDevice {
            location,
            id: PciDeviceId {
                vendor: vendor_id,
                device: device_id,
            },
            class,
            bars: [None; 6],
            interrupt_line,
            interrupt_pin,
        };
        
        // Read BARs
        for i in 0..6 {
            device.bars[i] = device.read_bar(i);
            
            // Skip next BAR for 64-bit BARs
            if let Some(Bar::Memory64 { .. }) = device.bars[i] {
                device.bars[i + 1] = None;
            }
        }
        
        Some(device)
    }
    
    /// Find devices by vendor and device ID
    pub fn find_device(&self, vendor: u16, device: u16) -> Vec<&PciDevice> {
        self.devices
            .iter()
            .filter(|d| d.id.vendor == vendor && d.id.device == device)
            .collect()
    }
    
    /// Find devices by class
    pub fn find_by_class(&self, class: u8, subclass: u8) -> Vec<&PciDevice> {
        self.devices
            .iter()
            .filter(|d| d.class.class == class && d.class.subclass == subclass)
            .collect()
    }
    
    /// Find devices by type
    pub fn find_by_type(&self, device_type: DeviceType) -> Vec<&PciDevice> {
        self.devices
            .iter()
            .filter(|d| d.class.device_type() == Some(device_type))
            .collect()
    }
}

/// Global PCI bus instance
pub static PCI_BUS: Mutex<PciBus> = Mutex::new(PciBus::new());

/// Initialize PCI subsystem
pub fn init() {
    PCI_BUS.lock().scan();
    
    // Print found devices
    let bus = PCI_BUS.lock();
    for device in &bus.devices {
        println!("PCI Device: {:04x}:{:04x} at {:02x}:{:02x}.{:x}",
            device.id.vendor,
            device.id.device,
            device.location.bus,
            device.location.device,
            device.location.function
        );
        
        if let Some(device_type) = device.class.device_type() {
            println!("  Type: {:?}", device_type);
        }
        
        for (i, bar) in device.bars.iter().enumerate() {
            if let Some(bar) = bar {
                match bar {
                    Bar::Memory32 { address, size, prefetchable } => {
                        println!("  BAR{}: Memory32 at 0x{:08x}, size: 0x{:x}, prefetchable: {}",
                            i, address, size, prefetchable);
                    }
                    Bar::Memory64 { address, size, prefetchable } => {
                        println!("  BAR{}: Memory64 at 0x{:016x}, size: 0x{:x}, prefetchable: {}",
                            i, address, size, prefetchable);
                    }
                    Bar::Io { port, size } => {
                        println!("  BAR{}: I/O at 0x{:04x}, size: 0x{:x}", i, port, size);
                    }
                }
            }
        }
    }
}
```

### 2. Timer Driver (timer.rs)

```rust
//! System timer driver
//! Provides timing and scheduling services

use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;
use x86_64::instructions::port::Port;

/// Timer frequency in Hz
const TIMER_FREQUENCY: u32 = 100; // 100 Hz = 10ms per tick

/// PIT (Programmable Interval Timer) ports
const PIT_CHANNEL0: u16 = 0x40;
const PIT_COMMAND: u16 = 0x43;

/// PIT frequency
const PIT_FREQUENCY: u32 = 1193182;

/// Global tick counter
static TICKS: AtomicU64 = AtomicU64::new(0);

/// Timer callbacks
static CALLBACKS: Mutex<Vec<TimerCallback>> = Mutex::new(Vec::new());

/// Timer callback structure
struct TimerCallback {
    id: u64,
    interval: u64,
    next_trigger: u64,
    callback: fn(),
    repeating: bool,
}

/// Initialize the timer
pub fn init() {
    // Configure PIT
    let divisor = PIT_FREQUENCY / TIMER_FREQUENCY;
    
    unsafe {
        // Send command byte
        Port::new(PIT_COMMAND).write(0x36_u8);
        
        // Send frequency divisor
        let low = divisor as u8;
        let high = (divisor >> 8) as u8;
        
        Port::new(PIT_CHANNEL0).write(low);
        Port::new(PIT_CHANNEL0).write(high);
    }
    
    println!("Timer initialized at {} Hz", TIMER_FREQUENCY);
}

/// Handle timer interrupt (called from interrupt handler)
pub fn tick() {
    let current_tick = TICKS.fetch_add(1, Ordering::Relaxed);
    
    // Process timer callbacks
    let mut callbacks = CALLBACKS.lock();
    let mut i = 0;
    
    while i < callbacks.len() {
        let callback = &mut callbacks[i];
        
        if current_tick >= callback.next_trigger {
            // Execute callback
            (callback.callback)();
            
            if callback.repeating {
                // Schedule next execution
                callback.next_trigger = current_tick + callback.interval;
                i += 1;
            } else {
                // Remove one-shot timer
                callbacks.remove(i);
            }
        } else {
            i += 1;
        }
    }
}

/// Get current system ticks
pub fn get_ticks() -> u64 {
    TICKS.load(Ordering::Relaxed)
}

/// Get system uptime in milliseconds
pub fn get_uptime_ms() -> u64 {
    get_ticks() * 1000 / TIMER_FREQUENCY as u64
}

/// Get system uptime in seconds
pub fn get_uptime_secs() -> u64 {
    get_ticks() / TIMER_FREQUENCY as u64
}

/// Sleep for a specified number of milliseconds
pub fn sleep_ms(ms: u64) {
    let start = get_uptime_ms();
    let target = start + ms;
    
    while get_uptime_ms() < target {
        // Busy wait - could be improved with proper scheduling
        core::hint::spin_loop();
    }
}

/// Sleep for a specified number of seconds
pub fn sleep_secs(secs: u64) {
    sleep_ms(secs * 1000);
}

/// Add a timer callback
pub fn add_timer(interval_ms: u64, callback: fn(), repeating: bool) -> u64 {
    let mut callbacks = CALLBACKS.lock();
    let id = callbacks.len() as u64;
    
    let interval_ticks = interval_ms * TIMER_FREQUENCY as u64 / 1000;
    let next_trigger = get_ticks() + interval_ticks;
    
    callbacks.push(TimerCallback {
        id,
        interval: interval_ticks,
        next_trigger,
        callback,
        repeating,
    });
    
    id
}

/// Remove a timer callback
pub fn remove_timer(id: u64) {
    let mut callbacks = CALLBACKS.lock();
    callbacks.retain(|cb| cb.id != id);
}

/// High precision timer using TSC (Time Stamp Counter)
pub struct HighPrecisionTimer {
    frequency: u64,
}

impl HighPrecisionTimer {
    /// Create a new high precision timer
    pub fn new() -> Self {
        // Calibrate TSC frequency
        let frequency = Self::calibrate_tsc();
        Self { frequency }
    }
    
    /// Calibrate TSC frequency
    fn calibrate_tsc() -> u64 {
        // Use PIT to calibrate TSC
        let start_tsc = Self::read_tsc();
        
        // Wait for 100ms
        sleep_ms(100);
        
        let end_tsc = Self::read_tsc();
        
        // Calculate frequency
        (end_tsc - start_tsc) * 10
    }
    
    /// Read TSC value
    fn read_tsc() -> u64 {
        unsafe {
            core::arch::x86_64::_rdtsc()
        }
    }
    
    /// Get current time in nanoseconds
    pub fn get_nanos(&self) -> u64 {
        let tsc = Self::read_tsc();
        tsc * 1_000_000_000 / self.frequency
    }
    
    /// Get current time in microseconds
    pub fn get_micros(&self) -> u64 {
        self.get_nanos() / 1000
    }
    
    /// Sleep for nanoseconds
    pub fn sleep_nanos(&self, nanos: u64) {
        let start = self.get_nanos();
        let target = start + nanos;
        
        while self.get_nanos() < target {
            core::hint::spin_loop();
        }
    }
}

/// HPET (High Precision Event Timer) support
pub struct Hpet {
    base_address: *mut u64,
    frequency: u64,
}

impl Hpet {
    /// Initialize HPET from ACPI table
    pub unsafe fn new(base_address: usize) -> Option<Self> {
        let base = base_address as *mut u64;
        
        // Read capabilities
        let capabilities = base.read_volatile();
        let vendor_id = (capabilities >> 16) & 0xFFFF;
        let period = capabilities >> 32; // Period in femtoseconds
        
        if vendor_id == 0 || period == 0 {
            return None;
        }
        
        // Calculate frequency
        let frequency = 1_000_000_000_000_000 / period;
        
        // Enable HPET
        let config = base.offset(2);
        config.write_volatile(config.read_volatile() | 1);
        
        Some(Self {
            base_address: base,
            frequency,
        })
    }
    
    /// Read counter value
    pub fn read_counter(&self) -> u64 {
        unsafe {
            self.base_address.offset(0x1E).read_volatile()
        }
    }
    
    /// Get current time in nanoseconds
    pub fn get_nanos(&self) -> u64 {
        self.read_counter() * 1_000_000_000 / self.frequency
    }
}

/// Real-time clock (RTC) support
pub struct Rtc;

impl Rtc {
    /// CMOS ports
    const CMOS_ADDRESS: u16 = 0x70;
    const CMOS_DATA: u16 = 0x71;
    
    /// Read from CMOS
    unsafe fn read_cmos(register: u8) -> u8 {
        Port::new(Self::CMOS_ADDRESS).write(register);
        Port::new(Self::CMOS_DATA).read()
    }
    
    /// Get current date and time
    pub fn get_datetime() -> DateTime {
        unsafe {
            // Disable interrupts to ensure consistent read
            let _guard = crate::interrupts::without_interrupts(|| {
                // Wait for update to complete
                while Self::read_cmos(0x0A) & 0x80 != 0 {}
                
                let second = Self::read_cmos(0x00);
                let minute = Self::read_cmos(0x02);
                let hour = Self::read_cmos(0x04);
                let day = Self::read_cmos(0x07);
                let month = Self::read_cmos(0x08);
                let year = Self::read_cmos(0x09);
                
                // Convert BCD to binary if necessary
                let register_b = Self::read_cmos(0x0B);
                if register_b & 0x04 == 0 {
                    // BCD mode
                    DateTime {
                        year: bcd_to_binary(year) as u16 + 2000,
                        month: bcd_to_binary(month),
                        day: bcd_to_binary(day),
                        hour: bcd_to_binary(hour),
                        minute: bcd_to_binary(minute),
                        second: bcd_to_binary(second),
                    }
                } else {
                    // Binary mode
                    DateTime {
                        year: year as u16 + 2000,
                        month,
                        day,
                        hour,
                        minute,
                        second,
                    }
                }
            });
            
            _guard
        }
    }
}

/// Date and time structure
#[derive(Debug, Clone, Copy)]
pub struct DateTime {
    pub year: u16,
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
}

/// Convert BCD to binary
fn bcd_to_binary(bcd: u8) -> u8 {
    (bcd & 0x0F) + ((bcd >> 4) * 10)
}

/// Timer statistics
pub struct TimerStats {
    pub uptime_ms: u64,
    pub tick_count: u64,
    pub timer_frequency: u32,
    pub active_timers: usize,
}

/// Get timer statistics
pub fn get_stats() -> TimerStats {
    TimerStats {
        uptime_ms: get_uptime_ms(),
        tick_count: get_ticks(),
        timer_frequency: TIMER_FREQUENCY,
        active_timers: CALLBACKS.lock().len(),
    }
}
```

### 3. Keyboard Driver (keyboard.rs)

```rust
//! PS/2 keyboard driver
//! Handles keyboard input and scancode translation

use alloc::collections::VecDeque;
use spin::Mutex;
use x86_64::instructions::port::Port;

/// PS/2 keyboard ports
const DATA_PORT: u16 = 0x60;
const STATUS_PORT: u16 = 0x64;
const COMMAND_PORT: u16 = 0x64;

/// Keyboard commands
const CMD_SET_LEDS: u8 = 0xED;
const CMD_ECHO: u8 = 0xEE;
const CMD_SCAN_CODE_SET: u8 = 0xF0;
const CMD_IDENTIFY: u8 = 0xF2;
const CMD_SET_RATE: u8 = 0xF3;
const CMD_ENABLE: u8 = 0xF4;
const CMD_DISABLE: u8 = 0xF5;
const CMD_RESET: u8 = 0xFF;

/// Key states
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyState {
    Pressed,
    Released,
}

/// Key event
#[derive(Debug, Clone, Copy)]
pub struct KeyEvent {
    pub key: Key,
    pub state: KeyState,
    pub modifiers: Modifiers,
}

/// Keyboard modifiers
#[derive(Debug, Clone, Copy)]
pub struct Modifiers {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub caps_lock: bool,
    pub num_lock: bool,
    pub scroll_lock: bool,
}

impl Modifiers {
    fn new() -> Self {
        Self {
            shift: false,
            ctrl: false,
            alt: false,
            caps_lock: false,
            num_lock: false,
            scroll_lock: false,
        }
    }
}

/// Keyboard keys
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    // Letters
    A, B, C, D, E, F, G, H, I, J, K, L, M,
    N, O, P, Q, R, S, T, U, V, W, X, Y, Z,
    
    // Numbers
    Key0, Key1, Key2, Key3, Key4, Key5, Key6, Key7, Key8, Key9,
    
    // Function keys
    F1, F2, F3, F4, F5, F6, F7, F8, F9, F10, F11, F12,
    
    // Special keys
    Escape, Tab, CapsLock, Shift, Ctrl, Alt,
    Space, Enter, Backspace, Delete,
    
    // Arrow keys
    Up, Down, Left, Right,
    
    // Punctuation
    Period, Comma, Semicolon, Quote, Slash, Backslash,
    LeftBracket, RightBracket, Equals, Minus, Grave,
    
    // Numpad
    NumPad0, NumPad1, NumPad2, NumPad3, NumPad4,
    NumPad5, NumPad6, NumPad7, NumPad8, NumPad9,
    NumPadPlus, NumPadMinus, NumPadMultiply, NumPadDivide,
    NumPadEnter, NumPadPeriod,
    
    // Other
    Home, End, PageUp, PageDown, Insert,
    PrintScreen, ScrollLock, Pause, NumLock,
    
    // Unknown key
    Unknown(u8),
}

/// Keyboard driver state
struct KeyboardState {
    modifiers: Modifiers,
    extended: bool,
    event_queue: VecDeque<KeyEvent>,
}

/// Global keyboard state
static KEYBOARD_STATE: Mutex<KeyboardState> = Mutex::new(KeyboardState {
    modifiers: Modifiers {
        shift: false,
        ctrl: false,
        alt: false,
        caps_lock: false,
        num_lock: false,
        scroll_lock: false,
    },
    extended: false,
    event_queue: VecDeque::new(),
});

/// Scancode set 1 to key mapping
fn scancode_to_key(scancode: u8, extended: bool) -> Option<(Key, KeyState)> {
    let pressed = scancode & 0x80 == 0;
    let code = scancode & 0x7F;
    let state = if pressed { KeyState::Pressed } else { KeyState::Released };
    
    let key = if extended {
        match code {
            0x48 => Key::Up,
            0x50 => Key::Down,
            0x4B => Key::Left,
            0x4D => Key::Right,
            0x47 => Key::Home,
            0x4F => Key::End,
            0x49 => Key::PageUp,
            0x51 => Key::PageDown,
            0x52 => Key::Insert,
            0x53 => Key::Delete,
            _ => Key::Unknown(scancode),
        }
    } else {
        match code {
            0x01 => Key::Escape,
            0x02 => Key::Key1,
            0x03 => Key::Key2,
            0x04 => Key::Key3,
            0x05 => Key::Key4,
            0x06 => Key::Key5,
            0x07 => Key::Key6,
            0x08 => Key::Key7,
            0x09 => Key::Key8,
            0x0A => Key::Key9,
            0x0B => Key::Key0,
            0x0C => Key::Minus,
            0x0D => Key::Equals,
            0x0E => Key::Backspace,
            0x0F => Key::Tab,
            
            0x10 => Key::Q,
            0x11 => Key::W,
            0x12 => Key::E,
            0x13 => Key::R,
            0x14 => Key::T,
            0x15 => Key::Y,
            0x16 => Key::U,
            0x17 => Key::I,
            0x18 => Key::O,
            0x19 => Key::P,
            0x1A => Key::LeftBracket,
            0x1B => Key::RightBracket,
            0x1C => Key::Enter,
            0x1D => Key::Ctrl,
            
            0x1E => Key::A,
            0x1F => Key::S,
            0x20 => Key::D,
            0x21 => Key::F,
            0x22 => Key::G,
            0x23 => Key::H,
            0x24 => Key::J,
            0x25 => Key::K,
            0x26 => Key::L,
            0x27 => Key::Semicolon,
            0x28 => Key::Quote,
            0x29 => Key::Grave,
            0x2A => Key::Shift, // Left shift
            0x2B => Key::Backslash,
            
            0x2C => Key::Z,
            0x2D => Key::X,
            0x2E => Key::C,
            0x2F => Key::V,
            0x30 => Key::B,
            0x31 => Key::N,
            0x32 => Key::M,
            0x33 => Key::Comma,
            0x34 => Key::Period,
            0x35 => Key::Slash,
            0x36 => Key::Shift, // Right shift
            
            0x38 => Key::Alt,
            0x39 => Key::Space,
            0x3A => Key::CapsLock,
            
            0x3B => Key::F1,
            0x3C => Key::F2,
            0x3D => Key::F3,
            0x3E => Key::F4,
            0x3F => Key::F5,
            0x40 => Key::F6,
            0x41 => Key::F7,
            0x42 => Key::F8,
            0x43 => Key::F9,
            0x44 => Key::F10,
            
            0x45 => Key::NumLock,
            0x46 => Key::ScrollLock,
            
            // Numpad
            0x47 => Key::NumPad7,
            0x48 => Key::NumPad8,
            0x49 => Key::NumPad9,
            0x4A => Key::NumPadMinus,
            0x4B => Key::NumPad4,
            0x4C => Key::NumPad5,
            0x4D => Key::NumPad6,
            0x4E => Key::NumPadPlus,
            0x4F => Key::NumPad1,
            0x50 => Key::NumPad2,
            0x51 => Key::NumPad3,
            0x52 => Key::NumPad0,
            0x53 => Key::NumPadPeriod,
            
            0x57 => Key::F11,
            0x58 => Key::F12,
            
            _ => Key::Unknown(scancode),
        }
    };
    
    Some((key, state))
}

/// Process a scancode from the keyboard
pub fn process_scancode(scancode: u8) {
    let mut state = KEYBOARD_STATE.lock();
    
    // Handle extended scancodes
    if scancode == 0xE0 {
        state.extended = true;
        return;
    }
    
    // Process the scancode
    if let Some((key, key_state)) = scancode_to_key(scancode, state.extended) {
        // Update modifiers
        match key {
            Key::Shift => state.modifiers.shift = key_state == KeyState::Pressed,
            Key::Ctrl => state.modifiers.ctrl = key_state == KeyState::Pressed,
            Key::Alt => state.modifiers.alt = key_state == KeyState::Pressed,
            Key::CapsLock if key_state == KeyState::Pressed => {
                state.modifiers.caps_lock = !state.modifiers.caps_lock;
                update_leds(&state.modifiers);
            }
            Key::NumLock if key_state == KeyState::Pressed => {
                state.modifiers.num_lock = !state.modifiers.num_lock;
                update_leds(&state.modifiers);
            }
            Key::ScrollLock if key_state == KeyState::Pressed => {
                state.modifiers.scroll_lock = !state.modifiers.scroll_lock;
                update_leds(&state.modifiers);
            }
            _ => {}
        }
        
        // Create key event
        let event = KeyEvent {
            key,
            state: key_state,
            modifiers: state.modifiers,
        };
        
        // Add to event queue
        state.event_queue.push_back(event);
    }
    
    // Reset extended flag
    state.extended = false;
}

/// Get the next key event
pub fn get_key_event() -> Option<KeyEvent> {
    KEYBOARD_STATE.lock().event_queue.pop_front()
}

/// Convert key to ASCII character
pub fn key_to_char(key: Key, modifiers: &Modifiers) -> Option<char> {
    let shift = modifiers.shift ^ modifiers.caps_lock;
    
    match key {
        Key::A => Some(if shift { 'A' } else { 'a' }),
        Key::B => Some(if shift { 'B' } else { 'b' }),
        Key::C => Some(if shift { 'C' } else { 'c' }),
        Key::D => Some(if shift { 'D' } else { 'd' }),
        Key::E => Some(if shift { 'E' } else { 'e' }),
        Key::F => Some(if shift { 'F' } else { 'f' }),
        Key::G => Some(if shift { 'G' } else { 'g' }),
        Key::H => Some(if shift { 'H' } else { 'h' }),
        Key::I => Some(if shift { 'I' } else { 'i' }),
        Key::J => Some(if shift { 'J' } else { 'j' }),
        Key::K => Some(if shift { 'K' } else { 'k' }),
        Key::L => Some(if shift { 'L' } else { 'l' }),
        Key::M => Some(if shift { 'M' } else { 'm' }),
        Key::N => Some(if shift { 'N' } else { 'n' }),
        Key::O => Some(if shift { 'O' } else { 'o' }),
        Key::P => Some(if shift { 'P' } else { 'p' }),
        Key::Q => Some(if shift { 'Q' } else { 'q' }),
        Key::R => Some(if shift { 'R' } else { 'r' }),
        Key::S => Some(if shift { 'S' } else { 's' }),
        Key::T => Some(if shift { 'T' } else { 't' }),
        Key::U => Some(if shift { 'U' } else { 'u' }),
        Key::V => Some(if shift { 'V' } else { 'v' }),
        Key::W => Some(if shift { 'W' } else { 'w' }),
        Key::X => Some(if shift { 'X' } else { 'x' }),
        Key::Y => Some(if shift { 'Y' } else { 'y' }),
        Key::Z => Some(if shift { 'Z' } else { 'z' }),
        
        Key::Key0 => Some(if modifiers.shift { ')' } else { '0' }),
        Key::Key1 => Some(if modifiers.shift { '!' } else { '1' }),
        Key::Key2 => Some(if modifiers.shift { '@' } else { '2' }),
        Key::Key3 => Some(if modifiers.shift { '#' } else { '3' }),
        Key::Key4 => Some(if modifiers.shift { '$' } else { '4' }),
        Key::Key5 => Some(if modifiers.shift { '%' } else { '5' }),
        Key::Key6 => Some(if modifiers.shift { '^' } else { '6' }),
        Key::Key7 => Some(if modifiers.shift { '&' } else { '7' }),
        Key::Key8 => Some(if modifiers.shift { '*' } else { '8' }),
        Key::Key9 => Some(if modifiers.shift { '(' } else { '9' }),
        
        Key::Space => Some(' '),
        Key::Enter => Some('\n'),
        Key::Tab => Some('\t'),
        
        Key::Period => Some(if modifiers.shift { '>' } else { '.' }),
        Key::Comma => Some(if modifiers.shift { '<' } else { ',' }),
        Key::Semicolon => Some(if modifiers.shift { ':' } else { ';' }),
        Key::Quote => Some(if modifiers.shift { '"' } else { '\'' }),
        Key::Slash => Some(if modifiers.shift { '?' } else { '/' }),
        Key::Backslash => Some(if modifiers.shift { '|' } else { '\\' }),
        Key::LeftBracket => Some(if modifiers.shift { '{' } else { '[' }),
        Key::RightBracket => Some(if modifiers.shift { '}' } else { ']' }),
        Key::Equals => Some(if modifiers.shift { '+' } else { '=' }),
        Key::Minus => Some(if modifiers.shift { '_' } else { '-' }),
        Key::Grave => Some(if modifiers.shift { '~' } else { '`' }),
        
        _ => None,
    }
}

/// Update keyboard LEDs
fn update_leds(modifiers: &Modifiers) {
    unsafe {
        // Wait for keyboard controller
        wait_for_controller();
        
        // Send LED command
        Port::new(DATA_PORT).write(CMD_SET_LEDS);
        wait_for_controller();
        
        // Send LED state
        let led_state = (modifiers.scroll_lock as u8) |
                       ((modifiers.num_lock as u8) << 1) |
                       ((modifiers.caps_lock as u8) << 2);
        
        Port::new(DATA_PORT).write(led_state);
    }
}

/// Wait for keyboard controller to be ready
unsafe fn wait_for_controller() {
    let mut status_port = Port::<u8>::new(STATUS_PORT);
    
    // Wait for input buffer to be empty
    while status_port.read() & 0x02 != 0 {
        core::hint::spin_loop();
    }
}

/// Initialize keyboard driver
pub fn init() {
    unsafe {
        // Reset keyboard
        wait_for_controller();
        Port::new(DATA_PORT).write(CMD_RESET);
        
        // Wait for self-test result
        while Port::<u8>::new(DATA_PORT).read() != 0xAA {
            core::hint::spin_loop();
        }
        
        // Enable keyboard
        wait_for_controller();
        Port::new(DATA_PORT).write(CMD_ENABLE);
        
        // Set initial LED state
        let state = KEYBOARD_STATE.lock();
        update_leds(&state.modifiers);
    }
    
    println!("Keyboard initialized");
}
```

### 4. Sound Driver (sound.rs)

```rust
//! Sound driver implementation
//! Supports PC speaker and basic audio devices

use spin::Mutex;
use x86_64::instructions::port::Port;

/// PC speaker ports
const PIT_CHANNEL_2: u16 = 0x42;
const PIT_COMMAND: u16 = 0x43;
const PC_SPEAKER_PORT: u16 = 0x61;

/// Sound frequency range
const MIN_FREQUENCY: u32 = 20;
const MAX_FREQUENCY: u32 = 20000;

/// PIT frequency for sound generation
const PIT_FREQUENCY: u32 = 1193180;

/// PC speaker driver
pub struct PcSpeaker;

impl PcSpeaker {
    /// Play a tone at the specified frequency
    pub fn play_tone(frequency: u32) {
        if frequency < MIN_FREQUENCY || frequency > MAX_FREQUENCY {
            return;
        }
        
        let divisor = PIT_FREQUENCY / frequency;
        
        unsafe {
            // Configure PIT channel 2
            Port::new(PIT_COMMAND).write(0xB6_u8);
            
            // Set frequency divisor
            Port::new(PIT_CHANNEL_2).write((divisor & 0xFF) as u8);
            Port::new(PIT_CHANNEL_2).write((divisor >> 8) as u8);
            
            // Enable speaker
            let mut speaker_port = Port::<u8>::new(PC_SPEAKER_PORT);
            let current = speaker_port.read();
            speaker_port.write(current | 0x03);
        }
    }
    
    /// Stop playing tone
    pub fn stop() {
        unsafe {
            let mut speaker_port = Port::<u8>::new(PC_SPEAKER_PORT);
            let current = speaker_port.read();
            speaker_port.write(current & !0x03);
        }
    }
}

/// Musical notes
#[derive(Debug, Clone, Copy)]
pub enum Note {
    C,
    CSharp,
    D,
    DSharp,
    E,
    F,
    FSharp,
    G,
    GSharp,
    A,
    ASharp,
    B,
}

impl Note {
    /// Get frequency for note at given octave
    pub fn frequency(self, octave: u8) -> u32 {
        let base_freq = match self {
            Note::C => 261.63,
            Note::CSharp => 277.18,
            Note::D => 293.66,
            Note::DSharp => 311.13,
            Note::E => 329.63,
            Note::F => 349.23,
            Note::FSharp => 369.99,
            Note::G => 392.00,
            Note::GSharp => 415.30,
            Note::A => 440.00,
            Note::ASharp => 466.16,
            Note::B => 493.88,
        };
        
        // Adjust for octave (base frequencies are for octave 4)
        let multiplier = 2.0_f32.powi(octave as i32 - 4);
        (base_freq * multiplier) as u32
    }
}

/// Simple music player
pub struct MusicPlayer {
    tempo: u32, // Beats per minute
}

impl MusicPlayer {
    /// Create a new music player
    pub fn new(tempo: u32) -> Self {
        Self { tempo }
    }
    
    /// Play a note
    pub fn play_note(&self, note: Note, octave: u8, duration_ms: u32) {
        let frequency = note.frequency(octave);
        PcSpeaker::play_tone(frequency);
        crate::timer::sleep_ms(duration_ms as u64);
        PcSpeaker::stop();
    }
    
    /// Play a rest (silence)
    pub fn play_rest(&self, duration_ms: u32) {
        PcSpeaker::stop();
        crate::timer::sleep_ms(duration_ms as u64);
    }
    
    /// Play a melody
    pub fn play_melody(&self, notes: &[(Option<(Note, u8)>, u32)]) {
        for (note_data, duration_beats) in notes.iter() {
            let duration_ms = (60_000 * duration_beats) / self.tempo;
            
            match note_data {
                Some((note, octave)) => self.play_note(*note, *octave, duration_ms),
                None => self.play_rest(duration_ms),
            }
        }
    }
}

/// Sound effects
pub struct SoundEffects;

impl SoundEffects {
    /// Play a beep sound
    pub fn beep() {
        PcSpeaker::play_tone(800);
        crate::timer::sleep_ms(200);
        PcSpeaker::stop();
    }
    
    /// Play an error sound
    pub fn error() {
        PcSpeaker::play_tone(250);
        crate::timer::sleep_ms(400);
        PcSpeaker::stop();
    }
    
    /// Play a success sound
    pub fn success() {
        PcSpeaker::play_tone(600);
        crate::timer::sleep_ms(100);
        PcSpeaker::play_tone(800);
        crate::timer::sleep_ms(100);
        PcSpeaker::play_tone(1000);
        crate::timer::sleep_ms(200);
        PcSpeaker::stop();
    }
    
    /// Play a notification sound
    pub fn notification() {
        PcSpeaker::play_tone(1000);
        crate::timer::sleep_ms(100);
        PcSpeaker::stop();
        crate::timer::sleep_ms(50);
        PcSpeaker::play_tone(1000);
        crate::timer::sleep_ms(100);
        PcSpeaker::stop();
    }
}

/// AC97 audio controller driver (basic structure)
pub struct Ac97Audio {
    base_address: u16,
}

impl Ac97Audio {
    /// Create new AC97 driver from PCI device
    pub fn new(pci_device: &crate::pci::PciDevice) -> Option<Self> {
        // Find I/O BAR
        for bar in &pci_device.bars {
            if let Some(crate::pci::Bar::Io { port, .. }) = bar {
                return Some(Self {
                    base_address: *port,
                });
            }
        }
        None
    }
    
    /// Initialize AC97 codec
    pub fn init(&self) {
        // Reset codec
        self.write_register(AC97_RESET, 0);
        
        // Wait for codec ready
        while self.read_register(AC97_POWERDOWN) & 0x0F != 0x0F {
            crate::timer::sleep_ms(10);
        }
        
        // Set master volume
        self.write_register(AC97_MASTER_VOLUME, 0x0000); // Maximum volume
        
        // Set PCM output volume
        self.write_register(AC97_PCM_VOLUME, 0x0808);
    }
    
    /// Read AC97 register
    fn read_register(&self, register: u16) -> u16 {
        unsafe {
            let addr_port = self.base_address + register;
            Port::new(addr_port).read()
        }
    }
    
    /// Write AC97 register
    fn write_register(&self, register: u16, value: u16) {
        unsafe {
            let addr_port = self.base_address + register;
            Port::new(addr_port).write(value);
        }
    }
}

// AC97 register offsets
const AC97_RESET: u16 = 0x00;
const AC97_MASTER_VOLUME: u16 = 0x02;
const AC97_POWERDOWN: u16 = 0x26;
const AC97_PCM_VOLUME: u16 = 0x18;

/// Sound system state
pub struct SoundSystem {
    pc_speaker_enabled: bool,
    ac97_device: Option<Ac97Audio>,
}

static SOUND_SYSTEM: Mutex<SoundSystem> = Mutex::new(SoundSystem {
    pc_speaker_enabled: true,
    ac97_device: None,
});

/// Initialize sound system
pub fn init() {
    // Enable PC speaker by default
    let mut system = SOUND_SYSTEM.lock();
    system.pc_speaker_enabled = true;
    
    // Look for AC97 audio device
    let pci_bus = crate::pci::PCI_BUS.lock();
    for device in pci_bus.find_by_class(0x04, 0x01) { // Audio device
        if let Some(ac97) = Ac97Audio::new(device) {
            ac97.init();
            system.ac97_device = Some(ac97);
            println!("AC97 audio initialized");
            break;
        }
    }
    
    println!("Sound system initialized");
}

/// Play startup sound
pub fn play_startup_sound() {
    let player = MusicPlayer::new(120); // 120 BPM
    
    // Simple startup melody
    let melody = vec![
        (Some((Note::C, 4)), 1),
        (Some((Note::E, 4)), 1),
        (Some((Note::G, 4)), 1),
        (Some((Note::C, 5)), 2),
    ];
    
    player.play_melody(&melody);
}

/// Play shutdown sound
pub fn play_shutdown_sound() {
    let player = MusicPlayer::new(120);
    
    let melody = vec![
        (Some((Note::C, 5)), 1),
        (Some((Note::G, 4)), 1),
        (Some((Note::E, 4)), 1),
        (Some((Note::C, 4)), 2),
    ];
    
    player.play_melody(&melody);
}
```

## Usage Examples

### PCI Device Enumeration

```rust
use orbita_os::drivers::pci;

// Initialize PCI subsystem
pci::init();

// Find all network devices
let bus = pci::PCI_BUS.lock();
for device in bus.find_by_class(0x02, 0x00) {
    println!("Found network device: {:04x}:{:04x}", 
        device.id.vendor, device.id.device);
}

// Find specific device
let devices = bus.find_device(0x8086, 0x100E); // Intel e1000
if let Some(device) = devices.first() {
    device.enable_bus_mastering();
    device.enable_memory_space();
}
```

### Timer Usage

```rust
use orbita_os::drivers::timer;

// Basic timing
let start = timer::get_uptime_ms();
perform_operation();
let elapsed = timer::get_uptime_ms() - start;
println!("Operation took {} ms", elapsed);

// Periodic callbacks
timer::add_timer(1000, || {
    println!("One second elapsed");
}, true);

// High precision timing
let hp_timer = timer::HighPrecisionTimer::new();
let start = hp_timer.get_nanos();
// ... operation ...
let elapsed_ns = hp_timer.get_nanos() - start;
```

### Keyboard Input

```rust
use orbita_os::drivers::keyboard::{self, KeyState};

// Poll for key events
while let Some(event) = keyboard::get_key_event() {
    if event.state == KeyState::Pressed {
        if let Some(ch) = keyboard::key_to_char(event.key, &event.modifiers) {
            print!("{}", ch);
        }
        
        // Handle special keys
        match event.key {
            keyboard::Key::Enter => println!(),
            keyboard::Key::Escape => break,
            _ => {}
        }
    }
}
```

### Sound Output

```rust
use orbita_os::drivers::sound::{PcSpeaker, Note, MusicPlayer, SoundEffects};

// Simple beep
SoundEffects::beep();

// Play a tone
PcSpeaker::play_tone(440); // A4
timer::sleep_ms(500);
PcSpeaker::stop();

// Play a melody
let player = MusicPlayer::new(120); // 120 BPM
player.play_note(Note::C, 4, 500);
player.play_note(Note::D, 4, 500);
player.play_note(Note::E, 4, 1000);
```

## Common Errors and Solutions

### 1. PCI Device Not Found

**Error**: Expected PCI device not detected
**Solution**: 
- Check device is properly connected
- Verify bus/device/function numbers
- Enable PCI bridge if device is behind bridge
- Check for legacy vs native mode

### 2. Timer Drift

**Error**: Timer becomes inaccurate over time
**Solution**: 
- Use high precision timer for accurate measurements
- Calibrate TSC frequency properly
- Consider using HPET if available
- Synchronize with RTC periodically

### 3. Keyboard Ghost Keys

**Error**: Extra or missing key events
**Solution**: 
- Implement proper key repeat handling
- Check for proper scancode set
- Handle extended scancodes correctly
- Implement debouncing if needed

### 4. No Sound Output

**Error**: PC speaker produces no sound
**Solution**: 
- Verify speaker is connected
- Check PIT channel 2 configuration
- Ensure frequency is in audible range
- Check speaker control port settings

## Module Dependencies

1. **Hardware Dependencies**:
   - PCI configuration space
   - I/O ports
   - PIT (8254) timer
   - PS/2 controller
   - PC speaker hardware

2. **Internal Dependencies**:
   - `interrupts`: Interrupt handling
   - `memory`: DMA buffer allocation
   - `io`: Port I/O operations

3. **Used By**:
   - Device drivers (network, storage)
   - Input system
   - Audio subsystem
   - System monitoring

## Performance Considerations

### 1. PCI Access Optimization

```rust
// Cache configuration space reads
pub struct PciDeviceCache {
    device: PciDevice,
    cache: HashMap<u8, u32>,
}

impl PciDeviceCache {
    pub fn read_config(&mut self, offset: u8) -> u32 {
        if let Some(&value) = self.cache.get(&offset) {
            value
        } else {
            let value = self.device.read_config(offset);
            self.cache.insert(offset, value);
            value
        }
    }
}
```

### 2. Timer Precision

```rust
// Use appropriate timer for the task
pub fn measure_operation<F, R>(f: F) -> (R, u64)
where
    F: FnOnce() -> R,
{
    if precision_needed() {
        let timer = HighPrecisionTimer::new();
        let start = timer.get_nanos();
        let result = f();
        let elapsed = timer.get_nanos() - start;
        (result, elapsed)
    } else {
        let start = get_uptime_ms();
        let result = f();
        let elapsed = get_uptime_ms() - start;
        (result, elapsed * 1_000_000) // Convert to nanos
    }
}
```

### 3. Keyboard Buffer Management

```rust
// Efficient key event buffer
const EVENT_BUFFER_SIZE: usize = 256;

pub struct KeyEventBuffer {
    buffer: [KeyEvent; EVENT_BUFFER_SIZE],
    read_idx: usize,
    write_idx: usize,
}

impl KeyEventBuffer {
    pub fn push(&mut self, event: KeyEvent) -> bool {
        let next_write = (self.write_idx + 1) % EVENT_BUFFER_SIZE;
        if next_write == self.read_idx {
            false // Buffer full
        } else {
            self.buffer[self.write_idx] = event;
            self.write_idx = next_write;
            true
        }
    }
}
```

## Future Improvements

1. **USB Support**:
   - UHCI/OHCI/EHCI/XHCI drivers
   - USB device enumeration
   - HID class driver
   - Mass storage class

2. **Audio Enhancement**:
   - HD Audio support
   - MIDI playback
   - Audio mixing
   - 3D positional audio

3. **Input Devices**:
   - USB keyboard/mouse
   - Touchpad support
   - Gamepad/joystick
   - Touchscreen

4. **Power Management**:
   - ACPI support
   - CPU frequency scaling
   - Device power states
   - Suspend/resume