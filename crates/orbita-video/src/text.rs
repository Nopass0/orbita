use crate::{
    font::{BitmapFont, BuiltinFont},
    Color, Point, Rect, Surface,
};

/// Horizontal alignment for text placement.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextAlign {
    Left,
    Center,
    Right,
}

/// Wrapping mode for the text renderer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextWrap {
    None,
    Character,
}

/// Layout information returned after text measurement.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TextMetrics {
    pub width: usize,
    pub height: usize,
    pub lines: usize,
}

/// Styling for text output.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TextStyle {
    pub fg: Color,
    pub bg: Option<Color>,
    pub scale: usize,
    pub letter_spacing: usize,
    pub line_spacing: usize,
    pub align: TextAlign,
    pub wrap: TextWrap,
}

impl TextStyle {
    pub const fn monospace(fg: Color) -> Self {
        Self {
            fg,
            bg: None,
            scale: 1,
            letter_spacing: 1,
            line_spacing: 2,
            align: TextAlign::Left,
            wrap: TextWrap::None,
        }
    }
}

impl Default for TextStyle {
    fn default() -> Self {
        Self::monospace(Color::WHITE)
    }
}

/// Text rendering helper for framebuffer UI and debug output.
pub struct TextRenderer<'a, F: BitmapFont = BuiltinFont> {
    surface: Surface<'a>,
    font: F,
    style: TextStyle,
}

impl<'a> TextRenderer<'a, BuiltinFont> {
    pub fn new(surface: Surface<'a>) -> Self {
        Self::with_font(surface, BuiltinFont::new())
    }
}

impl<'a, F: BitmapFont> TextRenderer<'a, F> {
    pub fn with_font(surface: Surface<'a>, font: F) -> Self {
        Self {
            surface,
            font,
            style: TextStyle::default(),
        }
    }

    pub fn surface_mut(&mut self) -> &mut Surface<'a> {
        &mut self.surface
    }

    pub fn style(&self) -> TextStyle {
        self.style
    }

    pub fn set_style(&mut self, style: TextStyle) {
        self.style = style;
    }

    pub fn measure(&self, text: &str) -> TextMetrics {
        self.measure_in_width(text, usize::MAX)
    }

    pub fn measure_in_width(&self, text: &str, max_width: usize) -> TextMetrics {
        let glyph_h = self.font.glyph_height() * self.style.scale;
        let line_height = glyph_h + self.style.line_spacing;
        let mut width = 0usize;
        let mut current = 0usize;
        let mut lines = 1usize;

        for ch in text.chars() {
            if ch == '\n' {
                width = core::cmp::max(width, current);
                current = 0;
                lines += 1;
                continue;
            }

            let glyph_w = self.glyph_advance(ch);
            let next = if current == 0 { glyph_w } else { current + glyph_w };
            if self.style.wrap == TextWrap::Character && next > max_width && current != 0 {
                width = core::cmp::max(width, current);
                current = glyph_w;
                lines += 1;
            } else {
                current = next;
            }
        }

        width = core::cmp::max(width, current);
        TextMetrics {
            width,
            height: lines * line_height,
            lines,
        }
    }

    pub fn draw_text(&mut self, origin: Point, text: &str) -> TextMetrics {
        self.draw_text_in_rect(Rect::new(origin.x, origin.y, usize::MAX, usize::MAX), text)
    }

    pub fn draw_text_in_rect(&mut self, rect: Rect, text: &str) -> TextMetrics {
        let metrics = self.measure_in_width(text, rect.width);
        let lines = split_lines(text);
        let line_height = self.font.glyph_height() * self.style.scale + self.style.line_spacing;

        for (line_index, line) in lines.iter().enumerate() {
            let line_width = self.measure_line(line);
            let start_x = match self.style.align {
                TextAlign::Left => rect.x,
                TextAlign::Center => rect.x + rect.width.saturating_sub(line_width) / 2,
                TextAlign::Right => rect.x + rect.width.saturating_sub(line_width),
            };
            let y = rect.y + line_index * line_height;
            let mut x = start_x;

            for ch in line.chars() {
                self.draw_glyph(Point::new(x, y), ch);
                x = x.saturating_add(self.glyph_advance(ch));
            }
        }

        metrics
    }

    fn glyph_advance(&self, _ch: char) -> usize {
        self.font.glyph_width() * self.style.scale + self.style.letter_spacing
    }

    fn measure_line(&self, line: &str) -> usize {
        let mut width = 0usize;
        for (idx, ch) in line.chars().enumerate() {
            if idx == 0 {
                width = width.saturating_add(self.font.glyph_width() * self.style.scale);
            } else {
                width = width.saturating_add(self.glyph_advance(ch));
            }
        }
        width
    }

    fn draw_glyph(&mut self, origin: Point, ch: char) {
        let glyph = match self.font.glyph(ch) {
            Some(glyph) => glyph,
            None => return,
        };

        if let Some(bg) = self.style.bg {
            let bounds = Rect::new(
                origin.x,
                origin.y,
                glyph.width() * self.style.scale,
                glyph.height() * self.style.scale,
            );
            self.surface.fill_rect(bounds, bg);
        }

        // The font is 8x8, so each glyph row can be expanded directly into pixels.
        for (row_idx, row) in glyph.rows.iter().enumerate() {
            if row_idx >= glyph.height() {
                break;
            }
            for col in 0..glyph.width() {
                if row & (1 << col) == 0 {
                    continue;
                }
                self.draw_scaled_pixel(
                    origin.x + col * self.style.scale,
                    origin.y + row_idx * self.style.scale,
                    self.style.fg,
                );
            }
        }
    }

    fn draw_scaled_pixel(&mut self, x: usize, y: usize, color: Color) {
        for sy in 0..self.style.scale {
            for sx in 0..self.style.scale {
                self.surface.write_pixel(x + sx, y + sy, color);
            }
        }
    }
}

/// Small zero-allocation line splitter used by the text renderer.
struct Lines<'a> {
    text: &'a str,
}

impl<'a> Lines<'a> {
    fn iter(self) -> LinesIter<'a> {
        LinesIter {
            text: self.text,
            index: 0,
        }
    }
}

struct LinesIter<'a> {
    text: &'a str,
    index: usize,
}

impl<'a> Iterator for LinesIter<'a> {
    type Item = &'a str;

    fn next(&mut self) -> Option<Self::Item> {
        if self.index >= self.text.len() {
            return None;
        }

        let rest = &self.text[self.index..];
        let end = rest.find('\n').unwrap_or(rest.len());
        let line = &rest[..end];
        self.index += end + usize::from(end < rest.len());
        Some(line)
    }
}

fn split_lines(text: &str) -> Lines<'_> {
    Lines { text }
}
