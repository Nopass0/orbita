# Graphics Implementation Guide

## Overview
The graphics module provides display capabilities for Orbita OS, including VGA text mode, framebuffer graphics, and advanced rendering features.

## Module Structure

### 1. VGA Buffer (vga_buffer.rs)

```rust
//! VGA text mode buffer implementation
//! Provides text output capabilities for early boot and fallback display

use core::fmt;
use spin::Mutex;
use volatile::Volatile;
use x86_64::instructions::port::Port;

/// VGA buffer dimensions
const BUFFER_HEIGHT: usize = 25;
const BUFFER_WIDTH: usize = 80;

/// VGA color attribute byte
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Color {
    Black = 0,
    Blue = 1,
    Green = 2,
    Cyan = 3,
    Red = 4,
    Magenta = 5,
    Brown = 6,
    LightGray = 7,
    DarkGray = 8,
    LightBlue = 9,
    LightGreen = 10,
    LightCyan = 11,
    LightRed = 12,
    Pink = 13,
    Yellow = 14,
    White = 15,
}

/// Color code combining foreground and background colors
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
pub struct ColorCode(u8);

impl ColorCode {
    /// Create a new color code
    pub const fn new(foreground: Color, background: Color) -> Self {
        ColorCode((background as u8) << 4 | (foreground as u8))
    }
}

/// A screen character with color attributes
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
struct ScreenChar {
    ascii_character: u8,
    color_code: ColorCode,
}

/// The VGA text buffer
#[repr(transparent)]
struct Buffer {
    chars: [[Volatile<ScreenChar>; BUFFER_WIDTH]; BUFFER_HEIGHT],
}

/// Writer for the VGA buffer
pub struct Writer {
    column_position: usize,
    row_position: usize,
    color_code: ColorCode,
    buffer: &'static mut Buffer,
}

impl Writer {
    /// Write a single byte to the buffer
    pub fn write_byte(&mut self, byte: u8) {
        match byte {
            b'\n' => self.new_line(),
            b'\r' => self.column_position = 0,
            b'\t' => {
                // Implement tab as 4 spaces
                for _ in 0..4 {
                    self.write_byte(b' ');
                }
            }
            byte => {
                if self.column_position >= BUFFER_WIDTH {
                    self.new_line();
                }
                
                let row = self.row_position;
                let col = self.column_position;
                let color_code = self.color_code;
                
                self.buffer.chars[row][col].write(ScreenChar {
                    ascii_character: byte,
                    color_code,
                });
                
                self.column_position += 1;
            }
        }
    }
    
    /// Write a string to the buffer
    pub fn write_string(&mut self, s: &str) {
        for byte in s.bytes() {
            match byte {
                // Printable ASCII byte or newline
                0x20..=0x7e | b'\n' | b'\r' | b'\t' => self.write_byte(byte),
                // Not printable ASCII
                _ => self.write_byte(0xfe), // ■ character
            }
        }
    }
    
    /// Move to a new line
    fn new_line(&mut self) {
        self.row_position += 1;
        self.column_position = 0;
        
        if self.row_position >= BUFFER_HEIGHT {
            self.scroll();
            self.row_position = BUFFER_HEIGHT - 1;
        }
    }
    
    /// Scroll the buffer up by one line
    fn scroll(&mut self) {
        for row in 1..BUFFER_HEIGHT {
            for col in 0..BUFFER_WIDTH {
                let character = self.buffer.chars[row][col].read();
                self.buffer.chars[row - 1][col].write(character);
            }
        }
        
        self.clear_row(BUFFER_HEIGHT - 1);
    }
    
    /// Clear a row
    fn clear_row(&mut self, row: usize) {
        let blank = ScreenChar {
            ascii_character: b' ',
            color_code: self.color_code,
        };
        
        for col in 0..BUFFER_WIDTH {
            self.buffer.chars[row][col].write(blank);
        }
    }
    
    /// Clear the entire screen
    pub fn clear_screen(&mut self) {
        for row in 0..BUFFER_HEIGHT {
            self.clear_row(row);
        }
        self.row_position = 0;
        self.column_position = 0;
    }
    
    /// Set the color for future writes
    pub fn set_color(&mut self, foreground: Color, background: Color) {
        self.color_code = ColorCode::new(foreground, background);
    }
    
    /// Move cursor to a specific position
    pub fn set_cursor_position(&mut self, row: usize, col: usize) {
        if row < BUFFER_HEIGHT && col < BUFFER_WIDTH {
            self.row_position = row;
            self.column_position = col;
            self.update_cursor();
        }
    }
    
    /// Update the hardware cursor position
    fn update_cursor(&self) {
        let pos = self.row_position * BUFFER_WIDTH + self.column_position;
        
        unsafe {
            // Send the high byte
            Port::new(0x3D4).write(0x0E_u8);
            Port::new(0x3D5).write((pos >> 8) as u8);
            
            // Send the low byte
            Port::new(0x3D4).write(0x0F_u8);
            Port::new(0x3D5).write(pos as u8);
        }
    }
}

impl fmt::Write for Writer {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        self.write_string(s);
        Ok(())
    }
}

/// Global writer instance
pub static WRITER: Mutex<Writer> = Mutex::new(Writer {
    column_position: 0,
    row_position: 0,
    color_code: ColorCode::new(Color::White, Color::Black),
    buffer: unsafe { &mut *(0xb8000 as *mut Buffer) },
});

/// Print to the VGA buffer
#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => ($crate::vga_buffer::_print(format_args!($($arg)*)));
}

/// Print to the VGA buffer with a newline
#[macro_export]
macro_rules! println {
    () => ($crate::print!("\n"));
    ($($arg:tt)*) => ($crate::print!("{}\n", format_args!($($arg)*)));
}

/// Internal print function
#[doc(hidden)]
pub fn _print(args: fmt::Arguments) {
    use core::fmt::Write;
    use crate::interrupts;
    
    interrupts::without_interrupts(|| {
        WRITER.lock().write_fmt(args).unwrap();
    });
}

/// Initialize VGA text mode
pub fn init() {
    // Clear the screen on initialization
    WRITER.lock().clear_screen();
    
    // Set default colors
    WRITER.lock().set_color(Color::White, Color::Black);
    
    // Enable cursor
    enable_cursor();
}

/// Enable the text cursor
fn enable_cursor() {
    unsafe {
        // Set cursor shape
        Port::new(0x3D4).write(0x0A_u8);
        Port::new(0x3D5).write(0x00_u8); // Top scanline
        
        Port::new(0x3D4).write(0x0B_u8);
        Port::new(0x3D5).write(0x0F_u8); // Bottom scanline
    }
}

/// Disable the text cursor
pub fn disable_cursor() {
    unsafe {
        Port::new(0x3D4).write(0x0A_u8);
        Port::new(0x3D5).write(0x20_u8);
    }
}
```

### 2. Framebuffer Graphics (graphics.rs)

```rust
//! Framebuffer graphics implementation
//! Provides pixel-based graphics capabilities

use core::slice;
use bootloader::{BootInfo, framebuffer::Framebuffer};
use spin::Mutex;

/// Pixel color representation
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    /// Create a new color
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 255 }
    }
    
    /// Create a new color with alpha
    pub const fn new_with_alpha(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }
    
    /// Common colors
    pub const BLACK: Self = Self::new(0, 0, 0);
    pub const WHITE: Self = Self::new(255, 255, 255);
    pub const RED: Self = Self::new(255, 0, 0);
    pub const GREEN: Self = Self::new(0, 255, 0);
    pub const BLUE: Self = Self::new(0, 0, 255);
    pub const YELLOW: Self = Self::new(255, 255, 0);
    pub const CYAN: Self = Self::new(0, 255, 255);
    pub const MAGENTA: Self = Self::new(255, 0, 255);
    pub const GRAY: Self = Self::new(128, 128, 128);
}

/// Framebuffer information
pub struct FramebufferInfo {
    pub width: u32,
    pub height: u32,
    pub pitch: u32,
    pub bpp: u8,
    pub buffer: &'static mut [u8],
}

/// Graphics driver for framebuffer operations
pub struct Graphics {
    info: FramebufferInfo,
    font: &'static [u8],
}

impl Graphics {
    /// Create a new graphics instance from the bootloader framebuffer
    pub fn new(framebuffer: &'static mut Framebuffer) -> Self {
        let info = FramebufferInfo {
            width: framebuffer.info().width,
            height: framebuffer.info().height,
            pitch: framebuffer.info().stride,
            bpp: framebuffer.info().bytes_per_pixel as u8,
            buffer: unsafe {
                slice::from_raw_parts_mut(
                    framebuffer.buffer_mut().as_mut_ptr(),
                    framebuffer.buffer_mut().len(),
                )
            },
        };
        
        Self {
            info,
            font: include_bytes!("../assets/font8x8.raw"),
        }
    }
    
    /// Clear the screen with a color
    pub fn clear(&mut self, color: Color) {
        for y in 0..self.info.height {
            for x in 0..self.info.width {
                self.put_pixel(x, y, color);
            }
        }
    }
    
    /// Put a pixel at the specified coordinates
    pub fn put_pixel(&mut self, x: u32, y: u32, color: Color) {
        if x >= self.info.width || y >= self.info.height {
            return;
        }
        
        let offset = (y * self.info.pitch + x * (self.info.bpp as u32 / 8)) as usize;
        
        match self.info.bpp {
            32 => {
                self.info.buffer[offset] = color.b;
                self.info.buffer[offset + 1] = color.g;
                self.info.buffer[offset + 2] = color.r;
                self.info.buffer[offset + 3] = color.a;
            }
            24 => {
                self.info.buffer[offset] = color.b;
                self.info.buffer[offset + 1] = color.g;
                self.info.buffer[offset + 2] = color.r;
            }
            16 => {
                let pixel = ((color.r as u16 & 0xF8) << 8) |
                           ((color.g as u16 & 0xFC) << 3) |
                           ((color.b as u16 & 0xF8) >> 3);
                self.info.buffer[offset] = pixel as u8;
                self.info.buffer[offset + 1] = (pixel >> 8) as u8;
            }
            _ => {} // Unsupported BPP
        }
    }
    
    /// Draw a line using Bresenham's algorithm
    pub fn draw_line(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, color: Color) {
        let dx = (x1 - x0).abs();
        let dy = (y1 - y0).abs();
        let sx = if x0 < x1 { 1 } else { -1 };
        let sy = if y0 < y1 { 1 } else { -1 };
        let mut err = dx - dy;
        let mut x = x0;
        let mut y = y0;
        
        loop {
            self.put_pixel(x as u32, y as u32, color);
            
            if x == x1 && y == y1 {
                break;
            }
            
            let e2 = 2 * err;
            if e2 > -dy {
                err -= dy;
                x += sx;
            }
            if e2 < dx {
                err += dx;
                y += sy;
            }
        }
    }
    
    /// Draw a rectangle
    pub fn draw_rect(&mut self, x: u32, y: u32, width: u32, height: u32, color: Color) {
        for i in 0..width {
            self.put_pixel(x + i, y, color);
            self.put_pixel(x + i, y + height - 1, color);
        }
        
        for i in 0..height {
            self.put_pixel(x, y + i, color);
            self.put_pixel(x + width - 1, y + i, color);
        }
    }
    
    /// Fill a rectangle
    pub fn fill_rect(&mut self, x: u32, y: u32, width: u32, height: u32, color: Color) {
        for dy in 0..height {
            for dx in 0..width {
                self.put_pixel(x + dx, y + dy, color);
            }
        }
    }
    
    /// Draw a circle using midpoint algorithm
    pub fn draw_circle(&mut self, center_x: i32, center_y: i32, radius: i32, color: Color) {
        let mut x = radius;
        let mut y = 0;
        let mut err = 0;
        
        while x >= y {
            self.put_pixel((center_x + x) as u32, (center_y + y) as u32, color);
            self.put_pixel((center_x + y) as u32, (center_y + x) as u32, color);
            self.put_pixel((center_x - y) as u32, (center_y + x) as u32, color);
            self.put_pixel((center_x - x) as u32, (center_y + y) as u32, color);
            self.put_pixel((center_x - x) as u32, (center_y - y) as u32, color);
            self.put_pixel((center_x - y) as u32, (center_y - x) as u32, color);
            self.put_pixel((center_x + y) as u32, (center_y - x) as u32, color);
            self.put_pixel((center_x + x) as u32, (center_y - y) as u32, color);
            
            if err <= 0 {
                y += 1;
                err += 2 * y + 1;
            }
            
            if err > 0 {
                x -= 1;
                err -= 2 * x + 1;
            }
        }
    }
    
    /// Fill a circle
    pub fn fill_circle(&mut self, center_x: i32, center_y: i32, radius: i32, color: Color) {
        for y in -radius..=radius {
            for x in -radius..=radius {
                if x * x + y * y <= radius * radius {
                    self.put_pixel(
                        (center_x + x) as u32,
                        (center_y + y) as u32,
                        color
                    );
                }
            }
        }
    }
    
    /// Draw a character at the specified position
    pub fn draw_char(&mut self, ch: char, x: u32, y: u32, color: Color, scale: u32) {
        let char_index = ch as usize;
        if char_index >= 256 {
            return; // Only support ASCII
        }
        
        let char_data = &self.font[char_index * 8..(char_index + 1) * 8];
        
        for (row, &byte) in char_data.iter().enumerate() {
            for col in 0..8 {
                if byte & (1 << (7 - col)) != 0 {
                    for sy in 0..scale {
                        for sx in 0..scale {
                            self.put_pixel(
                                x + col * scale + sx,
                                y + row as u32 * scale + sy,
                                color
                            );
                        }
                    }
                }
            }
        }
    }
    
    /// Draw a string at the specified position
    pub fn draw_string(&mut self, text: &str, x: u32, y: u32, color: Color, scale: u32) {
        let char_width = 8 * scale;
        let mut current_x = x;
        
        for ch in text.chars() {
            if ch == '\n' {
                return; // Simplified: don't handle newlines
            }
            self.draw_char(ch, current_x, y, color, scale);
            current_x += char_width;
        }
    }
    
    /// Draw an image (raw RGB data)
    pub fn draw_image(&mut self, image_data: &[u8], x: u32, y: u32, width: u32, height: u32) {
        for row in 0..height {
            for col in 0..width {
                let offset = ((row * width + col) * 3) as usize;
                if offset + 2 < image_data.len() {
                    let color = Color::new(
                        image_data[offset],
                        image_data[offset + 1],
                        image_data[offset + 2],
                    );
                    self.put_pixel(x + col, y + row, color);
                }
            }
        }
    }
    
    /// Alpha blend two colors
    fn blend_colors(&self, foreground: Color, background: Color) -> Color {
        let alpha = foreground.a as u16;
        let inv_alpha = 255 - alpha;
        
        Color::new(
            ((foreground.r as u16 * alpha + background.r as u16 * inv_alpha) / 255) as u8,
            ((foreground.g as u16 * alpha + background.g as u16 * inv_alpha) / 255) as u8,
            ((foreground.b as u16 * alpha + background.b as u16 * inv_alpha) / 255) as u8,
        )
    }
    
    /// Put a pixel with alpha blending
    pub fn put_pixel_alpha(&mut self, x: u32, y: u32, color: Color) {
        if x >= self.info.width || y >= self.info.height {
            return;
        }
        
        if color.a == 255 {
            self.put_pixel(x, y, color);
            return;
        }
        
        if color.a == 0 {
            return;
        }
        
        // Get current pixel color
        let offset = (y * self.info.pitch + x * (self.info.bpp as u32 / 8)) as usize;
        let background = match self.info.bpp {
            32 => Color::new(
                self.info.buffer[offset + 2],
                self.info.buffer[offset + 1],
                self.info.buffer[offset],
            ),
            24 => Color::new(
                self.info.buffer[offset + 2],
                self.info.buffer[offset + 1],
                self.info.buffer[offset],
            ),
            _ => Color::BLACK, // Fallback
        };
        
        let blended = self.blend_colors(color, background);
        self.put_pixel(x, y, blended);
    }
}

/// Global graphics instance
pub static GRAPHICS: Mutex<Option<Graphics>> = Mutex::new(None);

/// Initialize graphics with the framebuffer provided by the bootloader
pub fn init(boot_info: &'static BootInfo) {
    if let Some(fb) = boot_info.framebuffer.as_mut() {
        let graphics = Graphics::new(fb);
        *GRAPHICS.lock() = Some(graphics);
    }
}

/// Draw a pixel
pub fn put_pixel(x: u32, y: u32, color: Color) {
    if let Some(graphics) = GRAPHICS.lock().as_mut() {
        graphics.put_pixel(x, y, color);
    }
}

/// Clear the screen
pub fn clear(color: Color) {
    if let Some(graphics) = GRAPHICS.lock().as_mut() {
        graphics.clear(color);
    }
}

/// Draw a line
pub fn draw_line(x0: i32, y0: i32, x1: i32, y1: i32, color: Color) {
    if let Some(graphics) = GRAPHICS.lock().as_mut() {
        graphics.draw_line(x0, y0, x1, y1, color);
    }
}

/// Draw a rectangle
pub fn draw_rect(x: u32, y: u32, width: u32, height: u32, color: Color) {
    if let Some(graphics) = GRAPHICS.lock().as_mut() {
        graphics.draw_rect(x, y, width, height, color);
    }
}

/// Fill a rectangle
pub fn fill_rect(x: u32, y: u32, width: u32, height: u32, color: Color) {
    if let Some(graphics) = GRAPHICS.lock().as_mut() {
        graphics.fill_rect(x, y, width, height, color);
    }
}

/// Draw a string
pub fn draw_string(text: &str, x: u32, y: u32, color: Color, scale: u32) {
    if let Some(graphics) = GRAPHICS.lock().as_mut() {
        graphics.draw_string(text, x, y, color, scale);
    }
}
```

## Usage Examples

### VGA Text Mode

```rust
use orbita_os::vga_buffer::{Color, WRITER};

// Basic text output
println!("Hello, Orbita OS!");

// Colored text
WRITER.lock().set_color(Color::Green, Color::Black);
println!("Success: System initialized");

// Clear screen
WRITER.lock().clear_screen();

// Position cursor
WRITER.lock().set_cursor_position(10, 40);
print!("Centered text");
```

### Framebuffer Graphics

```rust
use orbita_os::graphics::{Color, init, clear, draw_rect, fill_circle, draw_string};

// Initialize graphics with framebuffer
init(framebuffer_tag);

// Clear screen to black
clear(Color::BLACK);

// Draw shapes
draw_rect(10, 10, 100, 100, Color::RED);
fill_circle(200, 200, 50, Color::BLUE);

// Draw text
draw_string("Orbita OS", 300, 100, Color::WHITE, 2);

// Draw a gradient
for x in 0..256 {
    let color = Color::new(x as u8, 0, 255 - x as u8);
    draw_line(x, 0, x, 100, color);
}
```

## Common Errors and Solutions

### 1. Screen Corruption

**Error**: Display shows garbage or corrupted graphics
**Solution**: 
- Verify framebuffer address and format
- Check for buffer overflows
- Ensure proper synchronization

### 2. Wrong Colors

**Error**: Colors appear incorrect or swapped
**Solution**: 
- Check pixel format (RGB vs BGR)
- Verify bits per pixel setting
- Handle endianness correctly

### 3. Performance Issues

**Error**: Drawing operations are slow
**Solution**: 
- Implement double buffering
- Use hardware acceleration if available
- Batch drawing operations
- Implement dirty rectangle tracking

### 4. Text Not Visible

**Error**: Text doesn't appear on screen
**Solution**: 
- Check font data is loaded correctly
- Verify text color contrasts with background
- Ensure coordinates are within screen bounds

## Module Dependencies

1. **Hardware Dependencies**:
   - VGA hardware for text mode
   - Linear framebuffer for graphics mode
   - Video BIOS for mode switching

2. **Internal Dependencies**:
   - `memory`: Buffer allocation
   - `interrupts`: Atomic operations
   - `multiboot`: Framebuffer information

3. **Used By**:
   - `console`: Text output
   - `window_manager`: GUI rendering
   - `desktop`: User interface
   - `games`: Game graphics

## Performance Optimization

### 1. Double Buffering

```rust
pub struct DoubleBuffer {
    front_buffer: &'static mut [u8],
    back_buffer: Vec<u8>,
    width: u32,
    height: u32,
    pitch: u32,
}

impl DoubleBuffer {
    pub fn swap(&mut self) {
        self.front_buffer.copy_from_slice(&self.back_buffer);
    }
    
    pub fn draw_to_back<F>(&mut self, f: F)
    where
        F: FnOnce(&mut [u8]),
    {
        f(&mut self.back_buffer);
    }
}
```

### 2. Dirty Rectangle Tracking

```rust
pub struct DirtyRectManager {
    rects: Vec<Rectangle>,
    screen_width: u32,
    screen_height: u32,
}

impl DirtyRectManager {
    pub fn mark_dirty(&mut self, rect: Rectangle) {
        self.rects.push(rect);
    }
    
    pub fn merge_rects(&mut self) {
        // Merge overlapping rectangles
        // Implementation here
    }
    
    pub fn update_screen(&mut self, graphics: &mut Graphics) {
        for rect in &self.rects {
            graphics.update_region(rect);
        }
        self.rects.clear();
    }
}
```

### 3. Hardware Acceleration

```rust
pub trait GraphicsAccelerator {
    fn fill_rect_accel(&mut self, x: u32, y: u32, width: u32, height: u32, color: Color);
    fn copy_rect_accel(&mut self, src: Rectangle, dst: Point);
    fn draw_line_accel(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, color: Color);
}

// Implement for specific hardware
impl GraphicsAccelerator for VesaAccelerator {
    // Hardware-specific implementations
}
```

## Advanced Features

### 1. Mode Setting

```rust
pub fn set_video_mode(width: u32, height: u32, bpp: u8) -> Result<(), VideoError> {
    // Use VESA BIOS extensions or native driver
    unsafe {
        // Call video BIOS or configure hardware directly
    }
}
```

### 2. Hardware Cursor

```rust
pub struct HardwareCursor {
    x: i32,
    y: i32,
    visible: bool,
    image: CursorImage,
}

impl HardwareCursor {
    pub fn set_position(&mut self, x: i32, y: i32) {
        self.x = x;
        self.y = y;
        // Update hardware registers
    }
    
    pub fn set_image(&mut self, image: CursorImage) {
        self.image = image;
        // Upload to hardware
    }
}
```

### 3. Transparency and Compositing

```rust
pub struct Compositor {
    layers: Vec<Layer>,
    output_buffer: Vec<u8>,
}

impl Compositor {
    pub fn composite(&mut self) {
        // Clear output buffer
        self.output_buffer.fill(0);
        
        // Composite layers from bottom to top
        for layer in &self.layers {
            if layer.visible {
                self.blend_layer(layer);
            }
        }
    }
    
    fn blend_layer(&mut self, layer: &Layer) {
        // Alpha blending implementation
    }
}
```

## Future Improvements

1. **3D Graphics**:
   - Implement OpenGL-compatible API
   - Add GPU driver support
   - Hardware transform and lighting

2. **Advanced 2D Features**:
   - Anti-aliasing
   - Bezier curves
   - Gradient fills
   - Image transformations

3. **Display Management**:
   - Multiple monitor support
   - Display hotplug detection
   - Resolution switching
   - Color management

4. **Performance**:
   - SIMD optimizations
   - GPU offloading
   - Texture caching
   - Parallel rendering