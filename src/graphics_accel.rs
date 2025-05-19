#![no_std]

use crate::graphics::{Color, Framebuffer};

/// Trait for hardware accelerated operations
pub trait GraphicsAccelerator {
    fn fill_rect_accel(
        &mut self,
        fb: &mut Framebuffer,
        x: usize,
        y: usize,
        width: usize,
        height: usize,
        color: Color,
    );
}

/// Dummy GOP accelerator implementation
pub struct GopAccelerator;

impl GraphicsAccelerator for GopAccelerator {
    fn fill_rect_accel(
        &mut self,
        fb: &mut Framebuffer,
        x: usize,
        y: usize,
        width: usize,
        height: usize,
        color: Color,
    ) {
        // Fallback to software drawing for now
        fb.fill_rect(x, y, width, height, color);
    }
}

