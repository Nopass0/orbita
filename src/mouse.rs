#![no_std]

use crate::graphics::{Color, FRAMEBUFFER};

/// Simple software mouse cursor
pub struct MouseCursor {
    pub x: isize,
    pub y: isize,
}

impl MouseCursor {
    /// Create a new mouse cursor at (0,0)
    pub const fn new() -> Self {
        Self { x: 0, y: 0 }
    }

    /// Move the cursor by delta values
    pub fn move_by(&mut self, dx: isize, dy: isize) {
        self.x += dx;
        self.y += dy;
    }

    /// Render the cursor as a small square
    pub fn draw(&self) {
        if let Some(ref mut fb) = *FRAMEBUFFER.lock() {
            let x = self.x.max(0) as usize;
            let y = self.y.max(0) as usize;
            for dy in 0..10 {
                for dx in 0..10 {
                    fb.draw_pixel(x + dx, y + dy, Color::new(255, 255, 255));
                }
            }
        }
    }
}
