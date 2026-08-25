use alloc::vec::Vec;
use font8x8::{UnicodeFonts, BASIC_FONTS};

/// A monochrome glyph bitmap up to 16x16 pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GlyphBitmap {
    pub width: usize,
    pub height: usize,
    pub rows: [u16; 16],
}

impl GlyphBitmap {
    pub const fn new(width: usize, height: usize, rows: [u16; 16]) -> Self {
        Self {
            width,
            height,
            rows,
        }
    }

    pub const fn from_8x8(rows: [u8; 8]) -> Self {
        let mut expanded = [0u16; 16];
        let mut index = 0usize;
        while index < 8 {
            expanded[index] = rows[index] as u16;
            index += 1;
        }
        Self::new(8, 8, expanded)
    }

    pub const fn width(&self) -> usize {
        self.width
    }

    pub const fn height(&self) -> usize {
        self.height
    }
}

/// Common font API for framebuffer text rendering.
pub trait BitmapFont {
    fn glyph(&self, ch: char) -> Option<GlyphBitmap>;

    fn glyph_width(&self) -> usize {
        8
    }

    fn glyph_height(&self) -> usize {
        8
    }

    fn advance(&self) -> usize {
        self.glyph_width() + 1
    }

    fn line_height(&self) -> usize {
        self.glyph_height() + 2
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MemoryGlyph {
    pub ch: char,
    pub bitmap: GlyphBitmap,
}

/// Runtime-supplied bitmap font that can be assembled from memory tables.
#[derive(Clone, Debug)]
pub struct MemoryFont {
    glyph_width: usize,
    glyph_height: usize,
    glyphs: Vec<MemoryGlyph>,
}

impl MemoryFont {
    pub fn new(glyph_width: usize, glyph_height: usize) -> Self {
        Self {
            glyph_width,
            glyph_height,
            glyphs: Vec::new(),
        }
    }

    pub fn with_glyphs(glyph_width: usize, glyph_height: usize, glyphs: &[MemoryGlyph]) -> Self {
        Self {
            glyph_width,
            glyph_height,
            glyphs: glyphs.to_vec(),
        }
    }

    pub fn push_glyph(&mut self, glyph: MemoryGlyph) {
        self.glyphs.push(glyph);
    }
}

/// Built-in 8x8 bitmap font sourced from the `font8x8` crate.
#[derive(Clone, Copy, Debug, Default)]
pub struct BuiltinFont;

impl BuiltinFont {
    pub const fn new() -> Self {
        Self
    }
}

impl BitmapFont for BuiltinFont {
    fn glyph(&self, ch: char) -> Option<GlyphBitmap> {
        // Basic Latin is enough for kernel logs, command prompts, and labels.
        BASIC_FONTS.get(ch).map(GlyphBitmap::from_8x8)
    }
}

impl BitmapFont for MemoryFont {
    fn glyph(&self, ch: char) -> Option<GlyphBitmap> {
        self.glyphs
            .iter()
            .find(|glyph| glyph.ch == ch)
            .map(|glyph| glyph.bitmap)
    }

    fn glyph_width(&self) -> usize {
        self.glyph_width
    }

    fn glyph_height(&self) -> usize {
        self.glyph_height
    }
}
