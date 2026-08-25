extern crate alloc;

use alloc::vec;
use alloc::vec::Vec;

use crate::{BitmapFont, BuiltinFont, Color, Framebuffer, GlyphBitmap, ImageView, Insets, Point, Rect, Size, TextStyle};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CursorKind {
    Arrow,
    Beam,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CursorStyle {
    pub kind: CursorKind,
    pub fill: Color,
    pub outline: Color,
    pub shadow: Color,
}

impl CursorStyle {
    pub const fn mac_like() -> Self {
        Self {
            kind: CursorKind::Arrow,
            fill: Color::rgba(252, 253, 255, 255),
            outline: Color::rgba(8, 12, 20, 255),
            shadow: Color::rgba(30, 90, 200, 56),
        }
    }
}

pub struct BackBuffer {
    width: usize,
    height: usize,
    pixels: Vec<Color>,
}

impl BackBuffer {
    pub fn new(size: Size) -> Self {
        Self {
            width: size.width,
            height: size.height,
            pixels: vec![Color::TRANSPARENT; size.width.saturating_mul(size.height)],
        }
    }

    pub const fn size(&self) -> Size {
        Size::new(self.width, self.height)
    }

    pub fn clear(&mut self, color: Color) {
        for pixel in &mut self.pixels {
            *pixel = color;
        }
    }

    pub fn canvas(&mut self) -> GuiCanvas<'_> {
        GuiCanvas { buffer: self }
    }

    pub fn present(&self, framebuffer: &mut Framebuffer) {
        self.present_region(framebuffer, Rect::new(0, 0, self.width, self.height));
    }

    /// Presents only `region` through a swappable [`PresentBackend`](crate::PresentBackend)
    /// (dirty-rect contract identical to [`BackBuffer::present_region`](BackBuffer::present_region)).
    pub fn present_region_to(&self, backend: &mut dyn crate::PresentBackend, region: Rect) {
        let full = Rect::new(0, 0, self.width, self.height);
        if let Some(region) = region.intersect(full) {
            backend.present_region(&self.pixels, self.width, region);
        }
    }

    /// Presents only `region` (clipped to both surfaces). Cursor blinks
    /// and prompt edits present a few rows instead of the whole screen.
    pub fn present_region(&self, framebuffer: &mut Framebuffer, region: Rect) {
        let width = self.width.min(framebuffer.width());
        let height = self.height.min(framebuffer.height());
        let full = Rect::new(0, 0, width, height);
        let Some(region) = region.intersect(full) else {
            return;
        };
        for y in region.y..region.bottom() {
            let row = &self.pixels[y * self.width + region.x..y * self.width + region.right()];
            framebuffer.write_row(region.x, y, row);
        }
    }

    fn blend_pixel(&mut self, x: usize, y: usize, color: Color) {
        if x >= self.width || y >= self.height {
            return;
        }
        let index = y * self.width + x;
        // Opaque colors skip the blend arithmetic entirely — the common
        // case for window backgrounds and text pills.
        self.pixels[index] = if color.a == 255 {
            color
        } else {
            color.blend_over(self.pixels[index])
        };
    }

    /// Fills a horizontal span without per-pixel coverage checks.
    fn fill_span(&mut self, y: usize, x_start: usize, x_end: usize, color: Color) {
        if y >= self.height {
            return;
        }
        let start = x_start.min(self.width);
        let end = x_end.min(self.width);
        if color.a == 255 {
            for x in start..end {
                self.pixels[y * self.width + x] = color;
            }
        } else {
            for x in start..end {
                self.pixels[y * self.width + x] = color.blend_over(self.pixels[y * self.width + x]);
            }
        }
    }

}

pub struct GuiCanvas<'a> {
    buffer: &'a mut BackBuffer,
}

impl<'a> GuiCanvas<'a> {
    pub fn clear(&mut self, color: Color) {
        self.buffer.clear(color);
    }

    pub fn fill_rect(&mut self, rect: Rect, color: Color) {
        for y in rect.y..rect.bottom().min(self.buffer.height) {
            for x in rect.x..rect.right().min(self.buffer.width) {
                self.buffer.blend_pixel(x, y, color);
            }
        }
    }

    pub fn fill_gradient(&mut self, rect: Rect, top: Color, bottom: Color) {
        let height = rect.height.max(1);
        for y in 0..rect.height {
            let mix = ((y * 255) / height) as u8;
            let color = top.lerp(bottom, mix);
            for x in rect.x..rect.right().min(self.buffer.width) {
                self.buffer.blend_pixel(x, rect.y + y, color);
            }
        }
    }

    pub fn fill_radial_glow(&mut self, center: Point, radius: usize, color: Color) {
        let left = center.x.saturating_sub(radius);
        let top = center.y.saturating_sub(radius);
        let right = (center.x + radius).min(self.buffer.width.saturating_sub(1));
        let bottom = (center.y + radius).min(self.buffer.height.saturating_sub(1));

        for y in top..=bottom {
            for x in left..=right {
                let dx = x as isize - center.x as isize;
                let dy = y as isize - center.y as isize;
                let distance_sq = (dx * dx + dy * dy) as usize;
                if distance_sq > radius * radius {
                    continue;
                }
                let distance = integer_sqrt(distance_sq);
                let alpha = (((radius.saturating_sub(distance)) * color.a as usize) / radius.max(1)) as u8;
                self.buffer.blend_pixel(x, y, color.with_alpha(alpha));
            }
        }
    }

    pub fn fill_rounded_rect(&mut self, rect: Rect, radius: usize, color: Color) {
        if radius == 0 {
            for y in rect.y..rect.bottom().min(self.buffer.height) {
                self.buffer.fill_span(y, rect.x, rect.right(), color);
            }
            return;
        }
        let bottom = rect.bottom().min(self.buffer.height);
        let right = rect.right();
        let corner_end_y = (rect.y + radius).min(bottom);
        let corner_start_y = rect.bottom().saturating_sub(radius);
        for y in rect.y..bottom {
            if y >= corner_end_y && y < corner_start_y.max(corner_end_y) {
                // Interior band: the whole row is covered.
                self.buffer.fill_span(y, rect.x, right, color);
            } else {
                // Corner bands: straight middle section fast, arc ends
                // checked per pixel (a radius-sized square at each side).
                self.buffer.fill_span(y, rect.x + radius, right.saturating_sub(radius), color);
                for x in rect.x..(rect.x + radius).min(right) {
                    if inside_rounded_rect(x, y, rect, radius) {
                        self.buffer.blend_pixel(x, y, color);
                    }
                }
                for x in right.saturating_sub(radius)..right {
                    if self.row_x_in_bounds(y, x) && inside_rounded_rect(x, y, rect, radius) {
                        self.buffer.blend_pixel(x, y, color);
                    }
                }
            }
        }
    }

    fn row_x_in_bounds(&self, y: usize, x: usize) -> bool {
        y < self.buffer.height && x < self.buffer.width
    }

    pub fn stroke_rounded_rect(&mut self, rect: Rect, radius: usize, thickness: usize, color: Color) {
        let inner = rect.inset(Insets::new(thickness, thickness, thickness, thickness));
        for y in rect.y..rect.bottom().min(self.buffer.height) {
            for x in rect.x..rect.right().min(self.buffer.width) {
                let outer_hit = inside_rounded_rect(x, y, rect, radius);
                let inner_hit = inner
                    .map(|inner_rect| inside_rounded_rect(x, y, inner_rect, radius.saturating_sub(thickness)))
                    .unwrap_or(false);
                if outer_hit && !inner_hit {
                    self.buffer.blend_pixel(x, y, color);
                }
            }
        }
    }

    pub fn glass_panel(
        &mut self,
        rect: Rect,
        radius: usize,
        tint_top: Color,
        tint_bottom: Color,
        border: Color,
    ) {
        self.fill_rounded_rect(rect, radius, tint_top.with_alpha(48));
        self.fill_gradient(rect, tint_top.with_alpha(64), tint_bottom.with_alpha(92));
        self.stroke_rounded_rect(rect, radius, 1, border);
    }

    pub fn shadow(&mut self, rect: Rect, radius: usize, spread: usize, color: Color) {
        let shadow_rect = Rect::new(
            rect.x.saturating_sub(spread),
            rect.y.saturating_sub(spread),
            rect.width.saturating_add(spread * 2),
            rect.height.saturating_add(spread * 2),
        );
        for y in shadow_rect.y..shadow_rect.bottom().min(self.buffer.height) {
            for x in shadow_rect.x..shadow_rect.right().min(self.buffer.width) {
                if inside_rounded_rect(x, y, shadow_rect, radius + spread)
                    && !inside_rounded_rect(x, y, rect, radius)
                {
                    self.buffer.blend_pixel(x, y, color);
                }
            }
        }
    }

    pub fn blit_image(&mut self, origin: Point, image: ImageView<'_>) {
        for y in 0..image.height {
            for x in 0..image.width {
                if let Some(color) = image.pixel(x, y) {
                    self.buffer.blend_pixel(origin.x + x, origin.y + y, color);
                }
            }
        }
    }

    pub fn draw_text(
        &mut self,
        origin: Point,
        text: &str,
        style: TextStyle,
    ) {
        self.draw_text_with_font(origin, text, style, BuiltinFont::new());
    }

    pub fn draw_text_with_font<F: BitmapFont + Clone>(
        &mut self,
        origin: Point,
        text: &str,
        style: TextStyle,
        font: F,
    ) {
        let mut renderer = SoftwareTextRenderer::new(self, font, style);
        renderer.draw_text(origin, text);
    }

    pub fn draw_cursor(&mut self, origin: Point, style: CursorStyle, phase: u8) {
        match style.kind {
            CursorKind::Arrow => draw_arrow_cursor(self, origin, style, phase),
            CursorKind::Beam => draw_beam_cursor(self, origin, style, phase),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DesktopTheme {
    pub wallpaper_top: Color,
    pub wallpaper_bottom: Color,
    pub wallpaper_glow: Color,
    pub window_fill: Color,
    pub window_fill_2: Color,
    pub window_border: Color,
    pub title_text: Color,
    pub body_text: Color,
    pub dock_tint_top: Color,
    pub dock_tint_bottom: Color,
    pub dock_border: Color,
}

impl DesktopTheme {
    pub const fn aurora() -> Self {
        Self {
            wallpaper_top: Color::rgb(30, 152, 164),
            wallpaper_bottom: Color::rgb(6, 34, 48),
            wallpaper_glow: Color::rgba(144, 255, 232, 78),
            window_fill: Color::rgba(14, 28, 40, 198),
            window_fill_2: Color::rgba(7, 17, 28, 186),
            window_border: Color::rgba(228, 248, 255, 58),
            title_text: Color::rgb(248, 250, 255),
            body_text: Color::rgb(224, 240, 244),
            dock_tint_top: Color::rgba(242, 252, 255, 34),
            dock_tint_bottom: Color::rgba(24, 120, 134, 134),
            dock_border: Color::rgba(228, 248, 255, 72),
        }
    }
}

pub fn inside_rounded_rect(x: usize, y: usize, rect: Rect, radius: usize) -> bool {
    if radius == 0 {
        return rect.contains(Point::new(x, y));
    }
    if !rect.contains(Point::new(x, y)) {
        return false;
    }

    let left = rect.x + radius;
    let right = rect.right().saturating_sub(radius + 1);
    let top = rect.y + radius;
    let bottom = rect.bottom().saturating_sub(radius + 1);

    if x >= left && x <= right {
        return true;
    }
    if y >= top && y <= bottom {
        return true;
    }

    let corner_x = if x < left { left } else { right };
    let corner_y = if y < top { top } else { bottom };
    let dx = x as isize - corner_x as isize;
    let dy = y as isize - corner_y as isize;
    (dx * dx + dy * dy) as usize <= radius * radius
}

fn integer_sqrt(value: usize) -> usize {
    let mut x = value;
    let mut y = x.div_ceil(2);
    while y < x {
        x = y;
        y = (x + value / x) / 2;
    }
    x
}

fn draw_arrow_cursor(canvas: &mut GuiCanvas<'_>, origin: Point, style: CursorStyle, phase: u8) {
    let pulse = 12 + ((phase as usize * 8) / 255);
    canvas.shadow(
        Rect::new(origin.x + 2, origin.y + 3, 18, 24),
        6,
        3,
        style.shadow.with_alpha((30 + pulse) as u8),
    );

    let rows = [
        (0usize, 0usize, 1usize),
        (1, 0, 2),
        (2, 0, 3),
        (3, 0, 4),
        (4, 0, 5),
        (5, 0, 6),
        (6, 0, 7),
        (7, 0, 8),
        (8, 0, 9),
        (9, 1, 7),
        (10, 2, 4),
        (11, 3, 4),
        (12, 4, 4),
        (13, 5, 4),
        (14, 6, 4),
        (15, 7, 4),
        (16, 8, 4),
        (17, 9, 3),
        (18, 10, 2),
    ];

    for (row, start, len) in rows {
        for col in start..start + len {
            canvas.buffer.blend_pixel(origin.x + col, origin.y + row, style.fill);
        }
    }

    for row in 0..19 {
        canvas.buffer.blend_pixel(origin.x, origin.y + row, style.outline);
    }
    for col in 0..11 {
        canvas.buffer.blend_pixel(origin.x + col, origin.y + col.min(8), style.outline);
    }
    for row in 9..19 {
        canvas.buffer.blend_pixel(origin.x + 8, origin.y + row, style.outline);
    }
}

fn draw_beam_cursor(canvas: &mut GuiCanvas<'_>, origin: Point, style: CursorStyle, phase: u8) {
    let alpha = 190u8.saturating_add(phase / 4);
    canvas.fill_rounded_rect(
        Rect::new(origin.x, origin.y, 6, 18),
        3,
        style.fill.with_alpha(alpha),
    );
    canvas.stroke_rounded_rect(Rect::new(origin.x, origin.y, 6, 18), 3, 1, style.outline);
}

struct SoftwareTextRenderer<'a, 'b, F: BitmapFont> {
    canvas: &'a mut GuiCanvas<'b>,
    font: F,
    style: TextStyle,
}

impl<'a, 'b, F: BitmapFont> SoftwareTextRenderer<'a, 'b, F> {
    fn new(canvas: &'a mut GuiCanvas<'b>, font: F, style: TextStyle) -> Self {
        Self { canvas, font, style }
    }

    fn draw_text(&mut self, origin: Point, text: &str) {
        let mut x = origin.x;
        let mut y = origin.y;
        let line_height = self.font.glyph_height() * self.style.scale + self.style.line_spacing;
        for ch in text.chars() {
            if ch == '\n' {
                x = origin.x;
                y = y.saturating_add(line_height);
                continue;
            }
            if let Some(glyph) = self.font.glyph(ch) {
                self.draw_glyph(Point::new(x, y), glyph);
            }
            x = x.saturating_add(self.font.glyph_width() * self.style.scale + self.style.letter_spacing);
        }
    }

    fn draw_glyph(&mut self, origin: Point, glyph: GlyphBitmap) {
        if let Some(bg) = self.style.bg {
            self.canvas.fill_rect(
                Rect::new(
                    origin.x,
                    origin.y,
                    glyph.width() * self.style.scale,
                    glyph.height() * self.style.scale,
                ),
                bg,
            );
        }

        for row in 0..glyph.height() {
            let bits = glyph.rows[row];
            for col in 0..glyph.width() {
                if bits & (1 << col) == 0 {
                    continue;
                }
                for sy in 0..self.style.scale {
                    for sx in 0..self.style.scale {
                        self.canvas.buffer.blend_pixel(
                            origin.x + col * self.style.scale + sx,
                            origin.y + row * self.style.scale + sy,
                            self.style.fg,
                        );
                    }
                }
            }
        }
    }
}
