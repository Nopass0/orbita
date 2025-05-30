use bootloader::framebuffer::{Framebuffer as BootFramebuffer, PixelFormat as BootPixelFormat};
use bootloader::BootInfo;
use core::fmt::Write;
use alloc::vec::Vec;
use font8x8::{UnicodeFonts, BASIC_FONTS};
use lazy_static::lazy_static;
use spin::Mutex;

// Actual framebuffer type provided by the bootloader
type FrameBuffer = BootFramebuffer;
#[derive(Debug, Clone, Copy)]
enum PixelFormat {
    RGB,
    BGR,
}

pub struct Framebuffer {
    buffer: &'static mut [u8],
    back_buffer: Vec<u8>,
    info: FrameBufferInfo,
}

#[derive(Debug, Clone, Copy)]
struct FrameBufferInfo {
    width: usize,
    height: usize,
    stride: usize,
    bytes_per_pixel: usize,
    pixel_format: PixelFormat,
}

impl Framebuffer {
    pub fn new(framebuffer: &'static mut FrameBuffer) -> Self {
        let fb_info = framebuffer.info();
        let info = FrameBufferInfo {
            width: fb_info.width as usize,
            height: fb_info.height as usize,
            stride: fb_info.stride,
            bytes_per_pixel: fb_info.bytes_per_pixel,
            pixel_format: match fb_info.pixel_format {
                BootPixelFormat::Rgb => PixelFormat::RGB,
                BootPixelFormat::Bgr => PixelFormat::BGR,
                _ => PixelFormat::RGB,
            },
        };

        let buffer = framebuffer.buffer_mut();
        let back_buffer = vec![0; buffer.len()];

        Self {
            buffer,
            back_buffer,
            info,
        }
    }

    /// Copy the back buffer to the screen buffer
    pub fn swap_buffers(&mut self) {
        self.buffer.copy_from_slice(&self.back_buffer);
    }

    pub fn draw_pixel(&mut self, x: usize, y: usize, color: Color) {
        if x < self.info.width && y < self.info.height {
            let pixel_offset = y * self.info.stride + x;
            let byte_offset = pixel_offset * self.info.bytes_per_pixel;

            let (r, g, b) = color.as_rgb();

            match self.info.pixel_format {
                PixelFormat::RGB => {
                    self.back_buffer[byte_offset] = r;
                    self.back_buffer[byte_offset + 1] = g;
                    self.back_buffer[byte_offset + 2] = b;
                }
                PixelFormat::BGR => {
                    self.back_buffer[byte_offset] = b;
                    self.back_buffer[byte_offset + 1] = g;
                    self.back_buffer[byte_offset + 2] = r;
                }
            }
        }
    }

    pub fn fill_rect(&mut self, x: usize, y: usize, width: usize, height: usize, color: Color) {
        for dy in 0..height {
            for dx in 0..width {
                self.draw_pixel(x + dx, y + dy, color);
            }
        }
    }

    pub fn draw_char(&mut self, c: char, x: usize, y: usize, color: Color) {
        if let Some(glyph) = BASIC_FONTS.get(c) {
            for (dy, row) in glyph.iter().enumerate() {
                for dx in 0..8 {
                    if row & (1 << (7 - dx)) != 0 {
                        self.draw_pixel(x + dx, y + dy, color);
                    }
                }
            }
        }
    }

    pub fn draw_string(&mut self, s: &str, x: usize, y: usize, color: Color) {
        let mut current_x = x;
        for c in s.chars() {
            self.draw_char(c, current_x, y, color);
            current_x += 8;
        }
    }

    pub fn clear(&mut self, color: Color) {
        self.fill_rect(0, 0, self.info.width, self.info.height, color);
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Color {
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Color { r, g, b }
    }

    pub fn as_rgb(self) -> (u8, u8, u8) {
        (self.r, self.g, self.b)
    }
}

pub const BLACK: Color = Color::new(0, 0, 0);
pub const WHITE: Color = Color::new(255, 255, 255);
pub const RED: Color = Color::new(255, 0, 0);
pub const GREEN: Color = Color::new(0, 255, 0);
pub const BLUE: Color = Color::new(0, 0, 255);

lazy_static! {
    pub static ref FRAMEBUFFER: Mutex<Option<Framebuffer>> = Mutex::new(None);
}

pub fn init(boot_info: &'static BootInfo) {
    if let Some(fb) = boot_info.framebuffer.as_mut() {
        let framebuffer = Framebuffer::new(fb);
        *FRAMEBUFFER.lock() = Some(framebuffer);
    }
}

pub struct GraphicsWriter {
    x: usize,
    y: usize,
    color: Color,
}

impl GraphicsWriter {
    pub fn new() -> Self {
        GraphicsWriter {
            x: 10,
            y: 10,
            color: WHITE,
        }
    }

    pub fn write_string(&mut self, s: &str) {
        if let Some(ref mut fb) = *FRAMEBUFFER.lock() {
            for c in s.chars() {
                if c == '\n' {
                    self.x = 10;
                    self.y += 10;
                } else {
                    fb.draw_char(c, self.x, self.y, self.color);
                    self.x += 8;
                    if self.x > fb.info.width - 10 {
                        self.x = 10;
                        self.y += 10;
                    }
                }
            }
        }
    }
}

impl Write for GraphicsWriter {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        self.write_string(s);
        Ok(())
    }
}
