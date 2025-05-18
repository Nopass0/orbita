#![no_std]
#![no_main]
#![feature(custom_test_frameworks)]
#![test_runner(orbita::test_runner)]
#![reexport_test_harness_main = "test_main"]

use core::panic::PanicInfo;
use orbita::{serial_print, serial_println};

#[no_mangle]
pub extern "C" fn _start() -> ! {
    test_main();
    loop {}
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    orbita::test_panic_handler(info)
}

#[test_case]
fn test_serial_output() {
    serial_print!("Testing serial output... ");
    serial_println!("[ok]");
}

#[test_case]
fn test_boot() {
    serial_println!("Orbita OS booted successfully!");
}
