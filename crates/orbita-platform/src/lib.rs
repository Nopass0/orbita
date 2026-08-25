#![no_std]

//! Platform backend: the early serial console and halt.
//!
//! Every subsystem logs through this crate instead of touching the UART
//! directly, so the output channel can change (serial → framebuffer
//! overlay → log disk) without touching call sites.

use core::fmt::Write;
use orbita_arch_x86_64::{cpu, serial::SerialPort};
use spin::Mutex;

static SERIAL: Mutex<SerialPort> = Mutex::new(SerialPort::com1());

/// Initialize the COM1 serial console (first thing the kernel does).
pub fn init_early_console() {
    SERIAL.lock().init();
}

/// Write `message` without a line break.
pub fn log(message: &str) {
    let _ = SERIAL.lock().write_str(message);
}

/// Write `message` followed by CRLF.
pub fn log_line(message: &str) {
    SERIAL.lock().write_line(message);
}

/// Write formatted output without a line break.
pub fn log_fmt(args: core::fmt::Arguments<'_>) {
    let _ = SERIAL.lock().write_fmt(args);
}

/// Write formatted output followed by CRLF.
pub fn log_line_fmt(args: core::fmt::Arguments<'_>) {
    let mut serial = SERIAL.lock();
    let _ = serial.write_fmt(args);
    serial.write_byte(b'\r');
    serial.write_byte(b'\n');
}

/// Halt the CPU forever (terminal failure path).
pub fn halt_forever() -> ! {
    cpu::halt_forever()
}
