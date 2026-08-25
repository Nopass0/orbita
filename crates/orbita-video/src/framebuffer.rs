use crate::{Color, PixelFormat, Point, Rect, Size};

/// Basic framebuffer description used by early boot and kernel rendering.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FramebufferInfo {
    pub base: *mut u8,
    pub size_bytes: usize,
    pub width: usize,
    pub height: usize,
    pub stride: usize,
    pub bytes_per_pixel: usize,
    pub format: PixelFormat,
}

impl FramebufferInfo {
    pub const fn size(self) -> Size {
        Size::new(self.width, self.height)
    }
}

/// A linear framebuffer backed by raw memory.
pub struct Framebuffer {
    pub info: FramebufferInfo,
}

impl Framebuffer {
    pub const fn new(info: FramebufferInfo) -> Self {
        Self { info }
    }

    pub const fn width(&self) -> usize {
        self.info.width
    }

    pub const fn height(&self) -> usize {
        self.info.height
    }

    pub const fn size(&self) -> Size {
        self.info.size()
    }

    pub fn fill(&mut self, color: Color) {
        self.fill_rect(Rect::new(0, 0, self.width(), self.height()), color);
    }

    pub fn clear(&mut self) {
        self.fill(Color::BLACK);
    }

    pub fn fill_rect(&mut self, rect: Rect, color: Color) {
        let clipped = match rect.intersect(Rect::new(0, 0, self.width(), self.height())) {
            Some(rect) => rect,
            None => return,
        };

        for y in clipped.y..clipped.bottom() {
            for x in clipped.x..clipped.right() {
                self.write_pixel(x, y, color);
            }
        }
    }

    pub fn draw_rect(&mut self, rect: Rect, color: Color) {
        if rect.width == 0 || rect.height == 0 {
            return;
        }

        let x2 = rect.right().saturating_sub(1);
        let y2 = rect.bottom().saturating_sub(1);
        self.draw_hline(rect.x, rect.y, rect.width, color);
        self.draw_hline(rect.x, y2, rect.width, color);
        self.draw_vline(rect.x, rect.y, rect.height, color);
        self.draw_vline(x2, rect.y, rect.height, color);
    }

    pub fn draw_hline(&mut self, x: usize, y: usize, width: usize, color: Color) {
        if y >= self.height() || width == 0 {
            return;
        }
        let end = core::cmp::min(x.saturating_add(width), self.width());
        for px in x..end {
            self.write_pixel(px, y, color);
        }
    }

    pub fn draw_vline(&mut self, x: usize, y: usize, height: usize, color: Color) {
        if x >= self.width() || height == 0 {
            return;
        }
        let end = core::cmp::min(y.saturating_add(height), self.height());
        for py in y..end {
            self.write_pixel(x, py, color);
        }
    }

    pub fn draw_line(&mut self, from: Point, to: Point, color: Color) {
        let mut x0 = from.x as isize;
        let mut y0 = from.y as isize;
        let x1 = to.x as isize;
        let y1 = to.y as isize;
        let dx = (x1 - x0).abs();
        let sx = if x0 < x1 { 1 } else { -1 };
        let dy = -(y1 - y0).abs();
        let sy = if y0 < y1 { 1 } else { -1 };
        let mut err = dx + dy;

        loop {
            if x0 >= 0 && y0 >= 0 {
                self.write_pixel(x0 as usize, y0 as usize, color);
            }
            if x0 == x1 && y0 == y1 {
                break;
            }
            let e2 = err * 2;
            if e2 >= dy {
                err += dy;
                x0 += sx;
            }
            if e2 <= dx {
                err += dx;
                y0 += sy;
            }
        }
    }

    pub fn draw_circle(&mut self, center: Point, radius: usize, color: Color) {
        let mut x = radius as isize;
        let mut y = 0isize;
        let mut err = 1isize - x;

        while x >= y {
            self.plot_circle_points(center, x, y, color);
            y += 1;
            if err < 0 {
                err += 2 * y + 1;
            } else {
                x -= 1;
                err += 2 * (y - x) + 1;
            }
        }
    }

    pub fn draw_checkerboard(&mut self, tile: usize, a: Color, b: Color) {
        if tile == 0 {
            return;
        }
        for y in 0..self.height() {
            for x in 0..self.width() {
                let choose_a = ((x / tile) + (y / tile)) % 2 == 0;
                self.write_pixel(x, y, if choose_a { a } else { b });
            }
        }
    }

    pub fn draw_gradient(&mut self, top_left: Color, bottom_right: Color) {
        let width = self.width().max(1);
        let height = self.height().max(1);

        for y in 0..self.height() {
            for x in 0..self.width() {
                let xr = x as u32 * 255 / width as u32;
                let yr = y as u32 * 255 / height as u32;
                let mix = ((xr + yr) / 2) as u8;
                let color = Color::rgba(
                    lerp(top_left.r, bottom_right.r, mix),
                    lerp(top_left.g, bottom_right.g, mix),
                    lerp(top_left.b, bottom_right.b, mix),
                    0xff,
                );
                self.write_pixel(x, y, color);
            }
        }
    }

    /// Bulk row copy: packs and writes `pixels` starting at `(x, y)`.
    /// One bounds check and one format decision per row instead of per
    /// pixel — this is the hot path for presenting back buffers.
    pub fn write_row(&mut self, x: usize, y: usize, pixels: &[Color]) {
        if y >= self.height() || x >= self.width() || pixels.is_empty() {
            return;
        }
        let count = pixels.len().min(self.width() - x);
        let row_offset = y
            .saturating_mul(self.info.stride)
            .saturating_add(x)
            .saturating_mul(self.info.bytes_per_pixel);
        let base = self.info.base as *mut u32;
        // Early boot framebuffer writes are raw and intentionally unbuffered.
        unsafe {
            for (i, color) in pixels.iter().take(count).enumerate() {
                base.add(row_offset / 4 + i).write_volatile(self.info.format.pack(*color));
            }
        }
    }

    pub fn write_pixel(&mut self, x: usize, y: usize, color: Color) {
        if x >= self.width() || y >= self.height() {
            return;
        }
        let offset = y
            .saturating_mul(self.info.stride)
            .saturating_add(x)
            .saturating_mul(self.info.bytes_per_pixel);
        let value = self.info.format.pack(color);
        // Early boot framebuffer writes are raw and intentionally unbuffered.
        unsafe {
            self.info
                .base
                .add(offset)
                .cast::<u32>()
                .write_volatile(value);
        }
    }

    fn plot_circle_points(&mut self, center: Point, x: isize, y: isize, color: Color) {
        let cx = center.x as isize;
        let cy = center.y as isize;
        let points = [
            (cx + x, cy + y),
            (cx + y, cy + x),
            (cx - y, cy + x),
            (cx - x, cy + y),
            (cx - x, cy - y),
            (cx - y, cy - x),
            (cx + y, cy - x),
            (cx + x, cy - y),
        ];

        for (px, py) in points {
            if px >= 0 && py >= 0 {
                self.write_pixel(px as usize, py as usize, color);
            }
        }
    }
}

fn lerp(a: u8, b: u8, t: u8) -> u8 {
    let a = a as u16;
    let b = b as u16;
    let t = t as u16;
    (((a * (255 - t)) + (b * t)) / 255) as u8
}
