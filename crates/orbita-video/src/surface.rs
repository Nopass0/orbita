use crate::{Color, Framebuffer, Point, Rect, Size};

/// A drawing surface with an optional clip rectangle.
///
/// This is the UI-facing layer over the raw framebuffer.
pub struct Surface<'a> {
    framebuffer: &'a mut Framebuffer,
    clip: Option<Rect>,
}

impl<'a> Surface<'a> {
    pub fn new(framebuffer: &'a mut Framebuffer) -> Self {
        Self {
            framebuffer,
            clip: None,
        }
    }

    pub fn size(&self) -> Size {
        self.framebuffer.size()
    }

    pub fn clip(&self) -> Option<Rect> {
        self.clip
    }

    pub fn set_clip(&mut self, clip: Option<Rect>) {
        self.clip = clip;
    }

    pub fn clear(&mut self, color: Color) {
        self.framebuffer.fill(color);
    }

    pub fn fill_rect(&mut self, rect: Rect, color: Color) {
        self.with_clipped(rect, |fb, clipped| fb.fill_rect(clipped, color));
    }

    pub fn stroke_rect(&mut self, rect: Rect, color: Color) {
        self.with_clipped(rect, |fb, clipped| fb.draw_rect(clipped, color));
    }

    pub fn panel(&mut self, rect: Rect, background: Color, border: Color) {
        self.fill_rect(rect, background);
        self.stroke_rect(rect, border);
    }

    pub fn separator_h(&mut self, x: usize, y: usize, width: usize, color: Color) {
        self.framebuffer.draw_hline(x, y, width, color);
    }

    pub fn separator_v(&mut self, x: usize, y: usize, height: usize, color: Color) {
        self.framebuffer.draw_vline(x, y, height, color);
    }

    pub fn point(&mut self, point: Point, color: Color) {
        self.write_pixel(point.x, point.y, color);
    }

    pub fn write_pixel(&mut self, x: usize, y: usize, color: Color) {
        if self.is_visible(x, y) {
            self.framebuffer.write_pixel(x, y, color);
        }
    }

    pub fn line(&mut self, from: Point, to: Point, color: Color) {
        self.framebuffer.draw_line(from, to, color);
    }

    pub fn circle(&mut self, center: Point, radius: usize, color: Color) {
        self.framebuffer.draw_circle(center, radius, color);
    }

    fn with_clipped<F>(&mut self, rect: Rect, draw: F)
    where
        F: FnOnce(&mut Framebuffer, Rect),
    {
        let viewport = Rect::new(0, 0, self.size().width, self.size().height);
        let clip = self.clip.unwrap_or(viewport);
        if let Some(clipped) = rect.intersect(clip).and_then(|r| r.intersect(viewport)) {
            draw(self.framebuffer, clipped);
        }
    }

    fn is_visible(&self, x: usize, y: usize) -> bool {
        let viewport = Rect::new(0, 0, self.size().width, self.size().height);
        let point = Point::new(x, y);
        if !viewport.contains(point) {
            return false;
        }
        match self.clip {
            Some(clip) => clip.contains(point),
            None => true,
        }
    }
}
