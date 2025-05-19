use crate::graphics::{Color, GraphicsWriter, BLACK, FRAMEBUFFER, WHITE};
use core::fmt::Write;
use crate::serial_println;

pub fn start() {
    serial_println!("Orbita OS Starting...");

    // Очистка экрана
    if let Some(ref mut fb) = *FRAMEBUFFER.lock() {
        fb.clear(BLACK);

        // Рисуем заголовок
        fb.draw_string("Orbita OS v0.1", 10, 10, WHITE);
        fb.draw_string("==================", 10, 20, WHITE);

        // Рисуем простой интерфейс
        draw_ui(fb);
    }

    serial_println!("Graphics initialized");

    // Detect audio devices via PCI
    let audio = crate::drivers::pci::find_audio_devices();
    serial_println!("Found {} audio device(s)", audio.len());
    serial_println!("System ready");

    // Основной цикл ядра
    loop {
        // Здесь будет обработка системных событий
        x86_64::instructions::hlt();
    }
}

fn draw_ui(fb: &mut crate::graphics::Framebuffer) {
    // Рисуем панель задач
    fb.fill_rect(
        0,
        fb.info.height - 40,
        fb.info.width,
        40,
        Color::new(64, 64, 64),
    );
    fb.draw_string("Orbita OS", 10, fb.info.height - 30, WHITE);

    // Рисуем окно терминала
    let terminal_x = 50;
    let terminal_y = 50;
    let terminal_width = 600;
    let terminal_height = 400;

    // Заголовок окна
    fb.fill_rect(
        terminal_x,
        terminal_y,
        terminal_width,
        30,
        Color::new(128, 128, 128),
    );
    fb.draw_string("Terminal", terminal_x + 10, terminal_y + 10, WHITE);

    // Тело окна
    fb.fill_rect(
        terminal_x,
        terminal_y + 30,
        terminal_width,
        terminal_height - 30,
        BLACK,
    );
    fb.draw_string(
        "> Welcome to Orbita OS",
        terminal_x + 10,
        terminal_y + 40,
        Color::new(0, 255, 0),
    );
    fb.draw_string(
        "> System initialized",
        terminal_x + 10,
        terminal_y + 55,
        Color::new(0, 255, 0),
    );
    fb.draw_string(
        "> _",
        terminal_x + 10,
        terminal_y + 70,
        Color::new(0, 255, 0),
    );
}
