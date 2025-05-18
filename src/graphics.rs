use bootloader::{BootInfo, bootinfo::{FrameBuffer, PixelFormat}};
use core::fmt::Write;
use font8x8::{UnicodeFonts, BASIC_FONTS};
use lazy_static::lazy_static;
use spin::Mutex;

pub struct Framebuffer {
    buffer: &'static mut [u8],
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
        let info = FrameBufferInfo {
            width: framebuffer.info().horizontal_resolution,
            height: framebuffer.info().vertical_resolution,
            stride: framebuffer.info().stride,
            bytes_per_pixel: framebuffer.info().bytes_per_pixel,
            pixel_format: framebuffer.info().pixel_format,
        };
        
        Self {
            buffer: framebuffer.buffer_mut(),
            info,
        }
    }

    pub fn draw_pixel(&mut self, x: usize, y: usize, color: Color) {
        if x < self.info.width && y < self.info.height {
            let pixel_offset = y * self.info.stride + x;
            let byte_offset = pixel_offset * self.info.bytes_per_pixel;
            
            let (r, g, b) = color.as_rgb();
            
            match self.info.pixel_format {
                PixelFormat::RGB => {
                    self.buffer[byte_offset] = r;
                    self.buffer[byte_offset + 1] = g;
                    self.buffer[byte_offset + 2] = b;
                },
                PixelFormat::BGR => {
                    self.buffer[byte_offset] = b;
                    self.buffer[byte_offset + 1] = g;
                    self.buffer[byte_offset + 2] = r;
                },
                _ => {}
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
    if let Some(framebuffer) = boot_info.framebuffer.as_mut() {
        let fb = Framebuffer::new(framebuffer);
        *FRAMEBUFFER.lock() = Some(fb);
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