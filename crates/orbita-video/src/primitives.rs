/// A point in framebuffer coordinates.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Point {
    pub x: usize,
    pub y: usize,
}

impl Point {
    pub const fn new(x: usize, y: usize) -> Self {
        Self { x, y }
    }
}

/// A width/height pair.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Size {
    pub width: usize,
    pub height: usize,
}

impl Size {
    pub const fn new(width: usize, height: usize) -> Self {
        Self { width, height }
    }
}

/// Insets are used for padding and margins around UI elements.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Insets {
    pub left: usize,
    pub top: usize,
    pub right: usize,
    pub bottom: usize,
}

impl Insets {
    pub const fn new(left: usize, top: usize, right: usize, bottom: usize) -> Self {
        Self {
            left,
            top,
            right,
            bottom,
        }
    }
}

/// A rectangle in framebuffer coordinates.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rect {
    pub x: usize,
    pub y: usize,
    pub width: usize,
    pub height: usize,
}

impl Rect {
    pub const fn new(x: usize, y: usize, width: usize, height: usize) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    pub const fn from_point_size(origin: Point, size: Size) -> Self {
        Self::new(origin.x, origin.y, size.width, size.height)
    }

    pub const fn right(self) -> usize {
        self.x.saturating_add(self.width)
    }

    pub const fn bottom(self) -> usize {
        self.y.saturating_add(self.height)
    }

    pub const fn contains(self, point: Point) -> bool {
        point.x >= self.x
            && point.y >= self.y
            && point.x < self.right()
            && point.y < self.bottom()
    }

    pub fn intersect(self, other: Self) -> Option<Self> {
        let x = core::cmp::max(self.x, other.x);
        let y = core::cmp::max(self.y, other.y);
        let right = core::cmp::min(self.right(), other.right());
        let bottom = core::cmp::min(self.bottom(), other.bottom());
        if right <= x || bottom <= y {
            None
        } else {
            Some(Self::new(x, y, right - x, bottom - y))
        }
    }

    pub fn inset(self, insets: Insets) -> Option<Self> {
        let x = self.x.saturating_add(insets.left);
        let y = self.y.saturating_add(insets.top);
        let right = self.right().saturating_sub(insets.right);
        let bottom = self.bottom().saturating_sub(insets.bottom);
        if right <= x || bottom <= y {
            None
        } else {
            Some(Self::new(x, y, right - x, bottom - y))
        }
    }
}
