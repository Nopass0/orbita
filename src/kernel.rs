use crate::graphics::{Color, GraphicsWriter, BLACK, FRAMEBUFFER, WHITE};
use crate::mouse::MouseCursor;
use crate::window_manager::{Window, WindowManager};
use core::fmt::Write;
use crate::serial_println;

pub fn start() {
    serial_println!("Orbita OS Starting...");

    // Initialize simple window manager and cursor
    let mut wm = WindowManager::new();
    wm.add_window(Window {
        x: 50,
        y: 50,
        width: 300,
        height: 200,
        title: "Demo",
    });
    let cursor = MouseCursor::new();

    // Очистка экрана
    if let Some(ref mut fb) = *FRAMEBUFFER.lock() {
        fb.clear(BLACK);
        wm.draw();
        cursor.draw();
        fb.swap_buffers();
    }

    serial_println!("Graphics initialized");
    serial_println!("System ready");

    // Основной цикл ядра
    loop {
        // Здесь будет обработка системных событий
        x86_64::instructions::hlt();
    }
}

