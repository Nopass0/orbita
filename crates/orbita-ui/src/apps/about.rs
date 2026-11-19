//! «Orbita» — сведения о системе (окно и раздел настроек).

use super::AppCx;
use super::settings::group;
use crate::geom::{Rect, Vec2};
use crate::gfx::Paint;
use crate::icons::Icon;
use crate::tokens::text;

/// Окно «Orbita».
pub struct AboutApp;

impl AboutApp {
    /// Новое окно.
    pub fn new() -> AboutApp {
        AboutApp
    }

    /// Рисует окно.
    pub fn ui(&mut self, cx: &mut AppCx, body: Rect) {
        about_page(cx, body.inset_xy(24.0, 8.0));
    }
}

impl Default for AboutApp {
    fn default() -> Self {
        AboutApp::new()
    }
}

/// Страница «О системе»: логотип, версия, сведения.
pub fn about_page(cx: &mut AppCx, a: Rect) {
    let t = cx.ui.theme;
    let theme = t;
    let logo = Rect::new(a.x, a.y + 4.0, 72.0, 72.0);
    cx.ui.p.icon(Icon::Logo, logo, t.ink, &theme, 1.0);
    cx.ui.p.text(Vec2::new(logo.right() + 16.0, a.y + 10.0), &text::TITLE_LG, "Orbita OS", t.ink);
    cx.ui.p.text(Vec2::new(logo.right() + 16.0, a.y + 44.0), &text::MONO_SM, "0.1.0 · x86_64 · no_std Rust", t.ink_muted);
    let info = cx.sys.info();
    let rows = [
        ("Система", info.os.clone()),
        ("Платформа", info.machine.clone()),
        ("Видеоадаптер", info.gpu.clone()),
        ("Рендерер", info.renderer.clone()),
        ("Память", info.memory.clone()),
        ("Сеть", cx.sys.network()),
    ];
    let g = Rect::new(a.x, a.y + 96.0, a.w, rows.len() as f32 * 44.0 + 8.0);
    group(&mut cx.ui, g);
    for (i, (k, v)) in rows.iter().enumerate() {
        let y = g.y + 4.0 + i as f32 * 44.0;
        if i > 0 {
            cx.ui.p.rect(Rect::new(g.x + 16.0, y, g.w - 32.0, 1.0), 0.0, Paint::Solid(t.stroke_subtle));
        }
        cx.ui.p.text_in(Rect::new(g.x + 16.0, y, 160.0, 44.0), &text::BODY_MD, k, t.ink, 0.0);
        cx.ui.p.text_in(Rect::new(g.x + 176.0, y, g.w - 192.0, 44.0), &text::MONO_SM, v, t.ink_muted, 1.0);
    }
}
