//! Логотип Orbita OS: планета с орбитой и спутником.
//!
//! Рисуется векторно (а не иконкой), поэтому остаётся чётким на любом
//! размере и умеет анимироваться: спутник летит по орбите — это и есть
//! индикатор загрузки. Геометрия повторяет символ `logo` дизайн-системы:
//! орбита — эллипс 13.5×5.2 (в единицах viewBox 32), наклон −28°,
//! планета радиусом 6.4.

extern crate alloc;

use alloc::vec::Vec;

use crate::color::Color;
use crate::geom::{Rect, Vec2};
use crate::gfx::{Paint, Painter};
use crate::math;
use crate::tokens::Theme;

/// Наклон орбиты (радианы), как в символе `logo`: −28°.
const TILT: f32 = -28.0 * core::f32::consts::PI / 180.0;

/// Рисует логотип.
///
/// Принимает: рисовальщик, центр, размер (сторона квадрата, px), тему,
/// угол спутника на орбите (радианы; для статичного логотипа — `-0.45`, как
/// в макете) и силу свечения (0 — без свечения).
///
/// ```
/// use orbita_ui::{gfx::Painter, Color, Vec2, tokens::DARK};
/// let mut p = Painter::new();
/// p.begin_frame(200, 200, 1.0, Color::BLACK);
/// orbita_ui::shell::logo::draw(&mut p, Vec2::new(100.0, 100.0), 96.0, &DARK, 0.0, 1.0);
/// assert!(p.end_frame().instances.len() >= 5);
/// ```
pub fn draw(p: &mut Painter, c: Vec2, size: f32, t: &Theme, sat_angle: f32, glow: f32) {
    let k = size / 32.0;
    let (st, ct) = (math::sin(TILT), math::cos(TILT));
    // Точка орбиты в координатах экрана.
    let orbit = |a: f32| -> Vec2 {
        let x = 13.5 * math::cos(a);
        let y = 5.2 * math::sin(a);
        Vec2::new(c.x + (x * ct - y * st) * k, c.y + (x * st + y * ct) * k)
    };
    let arc = |from: f32, to: f32| -> Vec<Vec2> {
        (0..=40).map(|i| orbit(from + (to - from) * i as f32 / 40.0)).collect()
    };
    let bounds = Rect::new(c.x - 13.5 * k, c.y - 13.5 * k, 27.0 * k, 27.0 * k);
    let ring = Paint::linear(135.0, t.amb_cyan, t.amb_blue);
    let ring_back = Paint::linear(135.0, t.amb_cyan.percent(0.5), t.amb_blue.percent(0.5));

    // Свечение планеты.
    if glow > 0.0 {
        let g = 30.0 * k;
        p.rect(
            Rect::new(c.x - g, c.y - g, g * 2.0, g * 2.0),
            0.0,
            Paint::blob(0.5, 0.5, 0.5, 0.5, t.amb_blue.percent(0.45 * glow), 1.0),
        );
    }
    // Спутник за планетой рисуется до неё.
    let sat = orbit(sat_angle);
    let behind = math::sin(sat_angle) < 0.0;
    let sat_r = 2.3 * k;
    let draw_sat = |p: &mut Painter| {
        if glow > 0.0 {
            let g = sat_r * 3.5;
            p.rect(Rect::new(sat.x - g, sat.y - g, g * 2.0, g * 2.0), 0.0, Paint::blob(0.5, 0.5, 0.5, 0.5, t.amb_cyan.percent(0.55 * glow), 1.0));
        }
        p.rect(Rect::new(sat.x - sat_r, sat.y - sat_r, sat_r * 2.0, sat_r * 2.0), sat_r, Paint::Radial {
            cx: 0.35,
            cy: 0.3,
            rx: 0.9,
            ry: 0.9,
            c0: Color::WHITE,
            c1: Color::rgb_hex(0xe8f6ff),
            c2: t.amb_cyan.mix(Color::WHITE, 0.4),
            mid: 0.4,
            end: 1.0,
        });
    };
    // Задняя половина орбиты (верхняя дуга символа, 50%).
    p.polyline(&arc(core::f32::consts::PI, 2.0 * core::f32::consts::PI), false, 2.2 * k, 0.0, ring_back, bounds);
    if behind {
        draw_sat(p);
    }
    // Планета: радиальный градиент белый → голубой → акцент.
    let pr = 6.4 * k;
    p.rect(Rect::new(c.x - pr, c.y - pr, pr * 2.0, pr * 2.0), pr, Paint::Radial {
        cx: 0.35,
        cy: 0.3,
        rx: 0.8,
        ry: 0.8,
        c0: Color::WHITE,
        c1: t.amb_cyan,
        c2: t.accent,
        mid: 0.35,
        end: 1.0,
    });
    // Передняя половина орбиты.
    p.polyline(&arc(0.0, core::f32::consts::PI), false, 2.2 * k, 0.0, ring, bounds);
    if !behind {
        draw_sat(p);
    }
}
