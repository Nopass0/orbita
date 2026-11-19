//! Обои рабочего стола: `ob-wall--a/b` и ambient-сцены `ob-stage--*`.
//!
//! Обои — статичный пакет кадра с ключом кэша: GPU и программный рендерер
//! рисуют их один раз и переиспользуют, пока не сменится тема, размер
//! экрана или выбор обоев. Поэтому даже тяжёлые размытые волны ничего не
//! стоят в установившемся режиме.

extern crate alloc;

use alloc::vec::Vec;

use crate::color::Color;
use crate::geom::{Rect, Vec2};
use crate::gfx::{Paint, Painter};
use crate::math;
use crate::tokens::{Theme, noise};

/// Вариант обоев.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Wallpaper {
    /// `ob-wall--a`: синий слева сверху, волны, кольца справа.
    WallA,
    /// `ob-wall--b`: зеркальная композиция с голубым справа.
    WallB,
    /// `ob-stage--aurora`: переплетение blue/cyan/indigo/teal/rose.
    Aurora,
    /// `ob-stage--mesh`: сетка пересекающихся эллипсов.
    Mesh,
    /// `ob-stage--spot`: один источник света сверху.
    Spot,
    /// `ob-stage--linear`: диагональный глубокий синий.
    Linear,
    /// Чистый `surface-base` без градиента.
    Flat,
}

impl Wallpaper {
    /// Все варианты (порядок в настройках).
    pub const ALL: [Wallpaper; 7] = [
        Wallpaper::WallA,
        Wallpaper::WallB,
        Wallpaper::Aurora,
        Wallpaper::Mesh,
        Wallpaper::Spot,
        Wallpaper::Linear,
        Wallpaper::Flat,
    ];

    /// Название для списка выбора.
    pub fn title(self) -> &'static str {
        match self {
            Wallpaper::WallA => "Орбита",
            Wallpaper::WallB => "Прибой",
            Wallpaper::Aurora => "Aurora",
            Wallpaper::Mesh => "Mesh",
            Wallpaper::Spot => "Spot",
            Wallpaper::Linear => "Linear",
            Wallpaper::Flat => "Без градиента",
        }
    }

    /// Индекс варианта.
    pub fn index(self) -> usize {
        Wallpaper::ALL.iter().position(|w| *w == self).unwrap_or(0)
    }

    /// Вариант по индексу.
    pub fn from_index(i: usize) -> Wallpaper {
        *Wallpaper::ALL.get(i).unwrap_or(&Wallpaper::WallA)
    }
}

/// Аппроксимирует кубическую кривую Безье ломаной.
fn cubic(out: &mut Vec<Vec2>, p0: Vec2, p1: Vec2, p2: Vec2, p3: Vec2, steps: usize) {
    for i in 1..=steps {
        let t = i as f32 / steps as f32;
        let mt = 1.0 - t;
        let x = mt * mt * mt * p0.x + 3.0 * mt * mt * t * p1.x + 3.0 * mt * t * t * p2.x + t * t * t * p3.x;
        let y = mt * mt * mt * p0.y + 3.0 * mt * mt * t * p1.y + 3.0 * mt * t * t * p2.y + t * t * t * p3.y;
        out.push(Vec2::new(x, y));
    }
}

/// Волна из макета: `M x0 y0 C c1 c2 p S c3 p2` (две кубические кривые,
/// вторая — со зеркальной контрольной точкой).
fn wave(m: Vec2, c1: Vec2, c2: Vec2, p: Vec2, c3: Vec2, p2: Vec2) -> Vec<Vec2> {
    let mut pts = alloc::vec![m];
    cubic(&mut pts, m, c1, c2, p, 24);
    let r = Vec2::new(2.0 * p.x - c2.x, 2.0 * p.y - c2.y);
    cubic(&mut pts, p, r, c3, p2, 24);
    pts
}

/// Рисует обои на весь экран.
///
/// Принимает: рисовальщик, тему, вариант, экран (логические px) и признак
/// включённого шума. Вызывать в начале кадра — внутри `begin_cached`.
pub fn draw(p: &mut Painter, t: &Theme, wp: Wallpaper, screen: Rect, noise_on: bool) {
    p.rect(screen, 0.0, Paint::Solid(t.surface_base));
    let s = t.amb_strength;
    let blob = |p: &mut Painter, area: Rect, rx: f32, ry: f32, cx: f32, cy: f32, c: Color, k: f32, end: f32| {
        p.rect(area, 0.0, Paint::blob(cx, cy, rx, ry, c.percent(k), end));
    };
    match wp {
        Wallpaper::WallA | Wallpaper::WallB => {
            let a = wp == Wallpaper::WallA;
            if a {
                blob(p, screen, 0.60, 0.80, 0.06, 0.00, t.amb_blue, s, 0.70);
                blob(p, screen, 0.50, 0.70, 1.00, 0.18, t.amb_indigo, s, 0.70);
                blob(p, screen, 0.60, 0.70, 0.68, 1.12, t.amb_cyan, s * 0.7, 0.70);
                blob(p, screen, 0.40, 0.50, 0.00, 1.00, t.amb_teal, s * 0.6, 0.70);
                blob(p, screen, 0.30, 0.40, 0.55, 0.40, t.amb_rose, s * 0.25, 0.70);
            } else {
                blob(p, screen, 0.60, 0.80, 1.00, 0.00, t.amb_cyan, s, 0.70);
                blob(p, screen, 0.55, 0.75, 0.00, 0.30, t.amb_blue, s, 0.70);
                blob(p, screen, 0.60, 0.70, 0.30, 1.15, t.amb_indigo, s, 0.70);
                blob(p, screen, 0.40, 0.50, 1.00, 1.00, t.amb_teal, s * 0.7, 0.70);
                blob(p, screen, 0.30, 0.40, 0.45, 0.55, t.amb_amber, s * 0.25, 0.70);
            }
            draw_waves(p, t, screen, !a);
        }
        Wallpaper::Aurora => {
            blob(p, screen, 0.52, 0.62, 0.12, 0.16, t.amb_blue, s, 0.72);
            blob(p, screen, 0.46, 0.56, 0.88, 0.10, t.amb_cyan, s * 0.8, 0.70);
            blob(p, screen, 0.54, 0.64, 0.80, 0.92, t.amb_indigo, s, 0.72);
            blob(p, screen, 0.40, 0.50, 0.14, 0.92, t.amb_teal, s * 0.65, 0.72);
            blob(p, screen, 0.28, 0.36, 0.52, 0.48, t.amb_rose, s * 0.3, 0.72);
        }
        Wallpaper::Mesh => {
            blob(p, screen, 0.38, 0.46, 0.20, 0.30, t.amb_blue, s, 0.70);
            blob(p, screen, 0.34, 0.44, 0.46, 0.14, t.amb_teal, s * 0.7, 0.70);
            blob(p, screen, 0.36, 0.46, 0.74, 0.34, t.amb_cyan, s * 0.75, 0.70);
            blob(p, screen, 0.40, 0.50, 0.38, 0.78, t.amb_indigo, s, 0.70);
            blob(p, screen, 0.34, 0.44, 0.84, 0.84, t.amb_blue, s * 0.9, 0.70);
            blob(p, screen, 0.22, 0.30, 0.60, 0.60, t.amb_amber, s * 0.3, 0.70);
        }
        Wallpaper::Spot => {
            blob(p, screen, 0.60, 0.70, 0.50, -0.10, t.amb_blue, s, 0.72);
            blob(p, screen, 0.40, 0.50, 1.00, 1.00, t.amb_indigo, s * 0.55, 0.72);
        }
        Wallpaper::Linear => {
            let k = if t.mode == crate::tokens::ThemeMode::Dark { 0.9 } else { 0.5 };
            p.rect(
                screen,
                0.0,
                Paint::Linear {
                    angle: 135.0,
                    c0: t.accent_deep.fade(k),
                    c1: t.amb_blue.mix(t.accent_deep, 0.15).mix(t.amb_cyan, 0.45).fade(k),
                    c2: t.amb_teal.mix(t.amb_cyan, 0.3).fade(k),
                    mid: 0.55,
                },
            );
        }
        Wallpaper::Flat => {}
    }
    if noise_on && wp != Wallpaper::Flat {
        p.noise(screen, 0.0, noise::SUBTLE * 1.6);
    }
}

/// Волны и кольца из SVG-обоев макета (viewBox 1280×720, `slice`).
fn draw_waves(p: &mut Painter, t: &Theme, screen: Rect, mirror: bool) {
    // preserveAspectRatio="xMidYMid slice": масштаб по большей стороне.
    let k = math::max(screen.w / 1280.0, screen.h / 720.0);
    let ox = screen.x + (screen.w - 1280.0 * k) * 0.5;
    let oy = screen.y + (screen.h - 720.0 * k) * 0.5;
    let map = |x: f32, y: f32| -> Vec2 {
        let x = if mirror { 1280.0 - x } else { x };
        Vec2::new(ox + x * k, oy + y * k)
    };
    let v = |x: f32, y: f32| map(x, y);
    let bounds = Rect::new(ox, oy, 1280.0 * k, 720.0 * k);
    let grad = |c0: Color, c1: Color, c2: Color, a: f32| {
        let (c0, c2) = if mirror { (c2, c0) } else { (c0, c2) };
        Paint::Linear { angle: 90.0, c0: c0.fade(a), c1: c1.fade(a), c2: c2.fade(a), mid: 0.55 }
    };
    // Толстые размытые полосы (`.bl`: blur 34px).
    let band1 = wave(v(-80.0, 580.0), v(220.0, 400.0), v(430.0, 720.0), v(720.0, 540.0), v(1120.0, 300.0), v(1380.0, 420.0));
    p.polyline(&band1, false, 150.0 * k, 34.0 * k, grad(t.amb_blue, t.amb_cyan, t.amb_indigo, 0.5), bounds);
    let band2 = wave(v(-80.0, 300.0), v(260.0, 150.0), v(520.0, 330.0), v(820.0, 170.0), v(1180.0, 40.0), v(1380.0, 130.0));
    p.polyline(&band2, false, 90.0 * k, 34.0 * k, grad(t.amb_blue, t.amb_cyan, t.amb_indigo, 0.38), bounds);
    // Тонкие линии (`.bs`: blur 1px), шаг 14px.
    for i in 0..8 {
        let dy = i as f32 * 14.0;
        let line = wave(
            v(-80.0, 540.0 + dy),
            v(220.0, 360.0 + dy),
            v(430.0, 680.0 + dy),
            v(720.0, 500.0 + dy),
            v(1120.0, 260.0 + dy),
            v(1380.0, 380.0 + dy),
        );
        p.polyline(&line, false, 1.2 * k, 0.8 * k, grad(t.amb_cyan, t.amb_cyan.mix(t.amb_blue, 0.5), t.amb_blue, 0.42), bounds);
    }
    // Концентрические кольца цвета `ink` с прозрачностью 10%.
    let c = map(1010.0, 250.0);
    for r in [130.0, 210.0, 300.0, 400.0] {
        let mut pts = Vec::with_capacity(97);
        for i in 0..=96 {
            let a = i as f32 / 96.0 * core::f32::consts::TAU;
            pts.push(Vec2::new(c.x + math::cos(a) * r * k, c.y + math::sin(a) * r * k));
        }
        p.polyline(&pts, false, 1.0 * k, 0.6 * k, Paint::Solid(t.ink.with_alpha(0.1)), bounds);
    }
}

/// Ключ кэша обоев: меняется при смене всего, что влияет на картинку.
pub fn cache_key(t: &Theme, wp: Wallpaper, w: u32, h: u32, noise_on: bool) -> u32 {
    let mut k: u32 = 0x9e37_79b9;
    for v in [wp.index() as u32, t.mode as u32, w, h, noise_on as u32, t.accent.to_bgra_u32()] {
        k ^= v.wrapping_add(0x7f4a_7c15).wrapping_add(k << 6).wrapping_add(k >> 2);
    }
    k | 1
}
