#![no_std]
#![feature(custom_test_frameworks)]
#![feature(alloc_error_handler)]
#![feature(abi_x86_interrupt)]
#![test_runner(crate::test_runner)]
#![reexport_test_harness_main = "test_main"]

extern crate alloc;

pub mod allocator;
pub mod gdt;
pub mod graphics;
pub mod interrupts;
pub mod kernel;
pub mod memory;
pub mod serial;
pub mod vga_buffer;

use core::panic::PanicInfo;

pub fn hlt_loop() -> ! {
    loop {
        x86_64::instructions::hlt();
    }
}

pub fn test_runner(tests: &[&dyn Fn()]) {
    serial_println!("Running {} tests", tests.len());
    for test in tests {
        test();
    }
    exit_qemu(QemuExitCode::Success);
}

pub fn test_panic_handler(info: &PanicInfo) -> ! {
    serial_println!("[failed]\nError: {}", info);
    exit_qemu(QemuExitCode::Failed);
    hlt_loop();
}

pub fn exit_qemu(exit_code: QemuExitCode) {
    use x86_64::instructions::port::Port;
    unsafe {
        let mut port = Port::new(0xf4);
        port.write(exit_code as u32);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum QemuExitCode {
    Success = 0x10,
    Failed = 0x11,
}
