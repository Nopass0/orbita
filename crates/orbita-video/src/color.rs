#![allow(clippy::exhaustive_enums)]

/// RGBA color used by the framebuffer renderer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    /// Creates an opaque RGB color.
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 0xff }
    }

    /// Creates an RGBA color.
    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    pub const WHITE: Self = Self::rgb(255, 255, 255);
    pub const BLACK: Self = Self::rgb(0, 0, 0);
    pub const RED: Self = Self::rgb(220, 60, 60);
    pub const GREEN: Self = Self::rgb(60, 220, 120);
    pub const BLUE: Self = Self::rgb(80, 120, 240);
    pub const GRAY: Self = Self::rgb(120, 120, 120);
    pub const TRANSPARENT: Self = Self::rgba(0, 0, 0, 0);

    pub const fn with_alpha(self, a: u8) -> Self {
        Self { a, ..self }
    }

    pub fn blend_over(self, background: Self) -> Self {
        let alpha = self.a as u16;
        let inv = 255u16.saturating_sub(alpha);

        let r = ((self.r as u16 * alpha) + (background.r as u16 * inv)) / 255;
        let g = ((self.g as u16 * alpha) + (background.g as u16 * inv)) / 255;
        let b = ((self.b as u16 * alpha) + (background.b as u16 * inv)) / 255;
        let out_a = alpha + ((background.a as u16 * inv) / 255);

        Self::rgba(r as u8, g as u8, b as u8, out_a.min(255) as u8)
    }

    pub fn lerp(self, other: Self, t: u8) -> Self {
        Self::rgba(
            lerp_channel(self.r, other.r, t),
            lerp_channel(self.g, other.g, t),
            lerp_channel(self.b, other.b, t),
            lerp_channel(self.a, other.a, t),
        )
    }
}

/// How a 32-bit pixel is written to the framebuffer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PixelFormat {
    Rgb,
    Bgr,
    Argb,
    Abgr,
}

impl PixelFormat {
    /// Packs a color into a 32-bit pixel value.
    pub const fn pack(self, color: Color) -> u32 {
        match self {
            // UEFI GOP exposes byte-oriented channel order while framebuffer writes use little-endian u32 stores.
            // For an RGB byte layout in memory, the u32 must therefore be packed as 0x00BBGGRR.
            Self::Rgb => ((color.b as u32) << 16) | ((color.g as u32) << 8) | color.r as u32,
            Self::Bgr => ((color.r as u32) << 16) | ((color.g as u32) << 8) | color.b as u32,
            Self::Argb => {
                ((color.a as u32) << 24)
                    | ((color.b as u32) << 16)
                    | ((color.g as u32) << 8)
                    | color.r as u32
            }
            Self::Abgr => {
                ((color.a as u32) << 24)
                    | ((color.r as u32) << 16)
                    | ((color.g as u32) << 8)
                    | color.b as u32
            }
        }
    }
}

const fn lerp_channel(a: u8, b: u8, t: u8) -> u8 {
    let a = a as u16;
    let b = b as u16;
    let t = t as u16;
    (((a * (255 - t)) + (b * t)) / 255) as u8
}
