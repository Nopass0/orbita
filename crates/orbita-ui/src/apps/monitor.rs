//! Монитор производительности: FPS, время кадра, график, сведения о GPU.

extern crate alloc;

use alloc::format;
use alloc::vec::Vec;

use super::AppCx;
use super::settings::{attention, group};
use crate::geom::{Rect, Vec2};
use crate::gfx::Paint;
use crate::math;
use crate::tokens::{radius, text};

/// Целевой FPS из требований (минимум 160 при наличии GPU).
pub const TARGET_FPS: f32 = 160.0;

/// Состояние монитора.
pub struct MonitorApp;

impl MonitorApp {
    /// Новый монитор.
    pub fn new() -> MonitorApp {
        MonitorApp
    }

    /// Рисует монитор.
    pub fn ui(&mut self, cx: &mut AppCx, body: Rect) {
        let t = cx.ui.theme;
        let a = body.inset_xy(20.0, 8.0);
        let s = cx.stats;
        let ok = s.fps >= TARGET_FPS;
        let title = format!("{:.0} FPS", s.fps);
        let sub = format!(
            "{} · цель ≥ {:.0} FPS · {}",
            if ok { "ok" } else { "ниже цели" },
            TARGET_FPS,
            cx.sys.info().renderer
        );
        attention(&mut cx.ui, Rect::new(a.x, a.y, a.w, 100.0), &title, &sub);

        // График FPS за последние 240 кадров.
        let g = Rect::new(a.x, a.y + 116.0, a.w, 180.0);
        group(&mut cx.ui, g);
        cx.ui.p.text(Vec2::new(g.x + 16.0, g.y + 12.0), &text::OVERLINE, "ЧАСТОТА КАДРОВ", t.ink_muted);
        let plot = Rect::new(g.x + 16.0, g.y + 36.0, g.w - 32.0, g.h - 52.0);
        let n = s.history.len();
        let mut vals: Vec<f32> = Vec::with_capacity(n);
        for i in 0..n {
            vals.push(s.history[(s.head + i) % n]);
        }
        let max = math::max(vals.iter().copied().fold(0.0, math::max), TARGET_FPS * 1.25);
        // Линия цели 160 FPS.
        let ty = plot.bottom() - plot.h * (TARGET_FPS / max);
        cx.ui.p.rect(Rect::new(plot.x, ty, plot.w, 1.0), 0.0, Paint::Solid(t.warning.percent(0.6)));
        cx.ui.p.text(Vec2::new(plot.right() - 60.0, ty - 18.0), &text::MONO_SM, "160", t.warning);
        // Столбики (по одному на кадр истории).
        let bw = plot.w / n as f32;
        for (i, v) in vals.iter().enumerate() {
            if *v <= 0.0 {
                continue;
            }
            let h = plot.h * math::clamp(*v / max, 0.0, 1.0);
            let c = if *v >= TARGET_FPS { t.accent_text } else { t.warning };
            cx.ui.p.rect(Rect::new(plot.x + i as f32 * bw, plot.bottom() - h, math::max(bw - 0.5, 0.5), h), 0.0, Paint::Solid(c.percent(0.75)));
        }

        // Метрики.
        let info = cx.sys.info();
        let rows = [
            ("CPU-время кадра", format!("{:.2} мс", s.cpu_ms)),
            ("GPU-время кадра", format!("{:.2} мс", s.gpu_ms)),
            ("Примитивов в кадре", format!("{}", s.instances)),
            ("Снимков стекла (blur)", format!("{}", s.batches)),
            ("Видеоадаптер", info.gpu.clone()),
            ("Процессоров", format!("{}", info.cpus)),
            ("Память", info.memory.clone()),
            ("Время работы", format!("{:.0} с", s.uptime)),
        ];
        let m = Rect::new(a.x, g.bottom() + 16.0, a.w, rows.len().div_ceil(2) as f32 * 44.0 + 8.0);
        group(&mut cx.ui, m);
        let cw = (m.w - 32.0) * 0.5;
        for (i, (k, v)) in rows.iter().enumerate() {
            let col = (i % 2) as f32;
            let row = (i / 2) as f32;
            let r = Rect::new(m.x + 16.0 + col * (cw + 0.0), m.y + 4.0 + row * 44.0, cw - 12.0, 44.0);
            cx.ui.p.text_in(Rect::new(r.x, r.y, r.w * 0.5, r.h), &text::BODY_SM, k, t.ink_muted, 0.0);
            cx.ui.p.text_in(Rect::new(r.x + r.w * 0.5, r.y, r.w * 0.5, r.h), &text::MONO_SM, v, t.ink, 1.0);
        }
        let _ = radius::LG;
        cx.ui.anims.mark_active();
    }
}

impl Default for MonitorApp {
    fn default() -> Self {
        MonitorApp::new()
    }
}
