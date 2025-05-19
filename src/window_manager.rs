#![no_std]

use alloc::vec::Vec;
use crate::graphics::{Color, FRAMEBUFFER};

/// Simple window representation
pub struct Window {
    pub x: usize,
    pub y: usize,
    pub width: usize,
    pub height: usize,
    pub title: &'static str,
}

/// Basic window manager
pub struct WindowManager {
    windows: Vec<Window>,
}

impl WindowManager {
    /// Create empty window manager
    pub fn new() -> Self {
        Self { windows: Vec::new() }
    }

    /// Add a window to manager
    pub fn add_window(&mut self, window: Window) {
        self.windows.push(window);
    }

    /// Draw all windows
    pub fn draw(&self) {
        if let Some(ref mut fb) = *FRAMEBUFFER.lock() {
            for w in &self.windows {
                fb.fill_rect(w.x, w.y, w.width, w.height, Color::new(0, 0, 128));
                fb.draw_string(w.title, w.x + 4, w.y + 4, Color::new(255, 255, 255));
            }
        }
    }
}

