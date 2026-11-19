//! Сохранение и восстановление сессии рабочего стола.
//!
//! Используется горячей перезагрузкой эмулятора: перед заменой DLL
//! оболочки состояние (настройки + окна) сериализуется в текст, после
//! загрузки новой версии — восстанавливается. Формат — строки `ключ=значение`,
//! устойчив к добавлению новых полей.

extern crate alloc;

use alloc::format;
use alloc::string::String;

use crate::apps::AppKind;
use crate::geom::Rect;
use crate::shell::{Desktop, Wallpaper};
use crate::tokens::{AccentChoice, ThemeMode};

/// Сериализует сессию.
///
/// ```
/// use orbita_ui::{Desktop, apps::AppKind};
/// let mut d = Desktop::new(orbita_ui::system::DemoSystem::boxed());
/// d.settings.mode = orbita_ui::tokens::ThemeMode::Light;
/// d.open(AppKind::Files, Some("/etc"));
/// let s = d.save_session();
/// let mut d2 = Desktop::new(orbita_ui::system::DemoSystem::boxed());
/// d2.restore_session(&s);
/// assert_eq!(d2.settings.mode, orbita_ui::tokens::ThemeMode::Light);
/// assert_eq!(d2.windows.len(), 1);
/// ```
pub fn save(d: &Desktop) -> String {
    let s = &d.settings;
    let mut out = String::new();
    let b = |v: bool| if v { 1 } else { 0 };
    out.push_str(&format!("mode={}\n", if s.mode == ThemeMode::Dark { "dark" } else { "light" }));
    out.push_str(&format!("accent={}\n", s.accent.index()));
    out.push_str(&format!("wallpaper={}\n", s.wallpaper.index()));
    out.push_str(&format!("blur={}\n", s.look.blur));
    out.push_str(&format!("noise={}\n", b(s.look.noise)));
    out.push_str(&format!("anim={}\n", b(s.look.animations)));
    out.push_str(&format!("cursor={}\n", b(s.look.accent_cursor)));
    out.push_str(&format!("fps={}\n", b(s.show_fps)));
    out.push_str(&format!("vsync={}\n", b(s.vsync)));
    out.push_str(&format!("dnd={}\n", b(s.dnd)));
    for w in &d.windows {
        if w.closing {
            continue;
        }
        let arg = w.app.session_arg().unwrap_or_default().replace('|', "/");
        out.push_str(&format!(
            "win={}|{}|{}|{}|{}|{}|{}|{}\n",
            w.app.kind().key(),
            w.rect.x,
            w.rect.y,
            w.rect.w,
            w.rect.h,
            b(w.maximized),
            b(w.minimized),
            arg
        ));
    }
    out
}

/// Восстанавливает сессию (неизвестные ключи игнорируются).
pub fn restore(d: &mut Desktop, text: &str) {
    for line in text.lines() {
        let Some((k, v)) = line.split_once('=') else { continue };
        let s = &mut d.settings;
        match k {
            "mode" => s.mode = if v == "light" { ThemeMode::Light } else { ThemeMode::Dark },
            "accent" => s.accent = AccentChoice::from_index(v.parse().unwrap_or(0)),
            "wallpaper" => s.wallpaper = Wallpaper::from_index(v.parse().unwrap_or(0)),
            "blur" => s.look.blur = v.parse().unwrap_or(1.0),
            "noise" => s.look.noise = v == "1",
            "anim" => s.look.animations = v == "1",
            "cursor" => s.look.accent_cursor = v == "1",
            "fps" => s.show_fps = v == "1",
            "vsync" => s.vsync = v == "1",
            "dnd" => s.dnd = v == "1",
            "win" => {
                let f: alloc::vec::Vec<&str> = v.split('|').collect();
                if f.len() < 8 {
                    continue;
                }
                let Some(kind) = AppKind::from_key(f[0]) else { continue };
                let num = |i: usize| f[i].parse::<f32>().unwrap_or(100.0);
                let rect = Rect::new(num(1), num(2), num(3), num(4));
                let arg = if f[7].is_empty() { None } else { Some(f[7]) };
                let id = d.open_at(kind, arg, rect, f[5] == "1");
                if f[6] == "1" {
                    d.set_minimized(id, true);
                }
            }
            _ => {}
        }
    }
}
