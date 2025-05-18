#![no_std]
#![no_main]
#![feature(custom_test_frameworks)]
#![feature(alloc_error_handler)]
#![feature(abi_x86_interrupt)]
#![test_runner(orbita::test_runner)]
#![reexport_test_harness_main = "test_main"]

extern crate alloc;

extern crate orbita;

use bootloader::{entry_point, BootInfo};
use core::panic::PanicInfo;
use x86_64::VirtAddr;
use orbita::{
    allocator, gdt, graphics, interrupts, kernel, memory, serial, hlt_loop,
    serial_println,
};


entry_point!(kernel_main);

fn kernel_main(boot_info: &'static BootInfo) -> ! {
    // Инициализация базовых компонентов
    gdt::init();
    interrupts::init_idt();
    unsafe { interrupts::PICS.lock().initialize() };
    x86_64::instructions::interrupts::enable();

    // Инициализация последовательного порта
    serial::init();

    // Инициализация памяти
    let phys_mem_offset = VirtAddr::new(boot_info.physical_memory_offset);
    let mut mapper = unsafe { memory::init(phys_mem_offset) };
    let mut frame_allocator =
        unsafe { memory::BootInfoFrameAllocator::init(&boot_info.memory_map) };

    // Инициализация heap
    allocator::init_heap(&mut mapper, &mut frame_allocator).expect("heap initialization failed");

    // Переход в графический режим
    graphics::init(boot_info);

    // Запуск основного ядра
    kernel::start();

    hlt_loop();
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    serial_println!("Kernel panic: {}", info);
    hlt_loop();
}
