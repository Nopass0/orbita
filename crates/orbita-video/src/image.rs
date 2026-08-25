use alloc::vec;
use alloc::vec::Vec;

use crate::Color;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImageView<'a> {
    pub width: usize,
    pub height: usize,
    pub pixels: &'a [Color],
}

impl<'a> ImageView<'a> {
    pub const fn new(width: usize, height: usize, pixels: &'a [Color]) -> Self {
        Self {
            width,
            height,
            pixels,
        }
    }

    pub fn pixel(&self, x: usize, y: usize) -> Option<Color> {
        if x >= self.width || y >= self.height {
            return None;
        }
        self.pixels.get(y * self.width + x).copied()
    }
}

#[derive(Clone, Debug)]
pub struct OwnedImage {
    width: usize,
    height: usize,
    pixels: Vec<Color>,
}

impl OwnedImage {
    pub fn new(width: usize, height: usize, fill: Color) -> Self {
        Self {
            width,
            height,
            pixels: vec![fill; width.saturating_mul(height)],
        }
    }

    pub fn set_pixel(&mut self, x: usize, y: usize, color: Color) {
        if x >= self.width || y >= self.height {
            return;
        }
        self.pixels[y * self.width + x] = color;
    }

    pub fn as_view(&self) -> ImageView<'_> {
        ImageView::new(self.width, self.height, &self.pixels)
    }
}
