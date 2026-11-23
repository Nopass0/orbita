//! Хром оболочки: верхняя панель, док, лаунчер, центр управления, тосты,
//! иконки рабочего стола и курсор — по макетам `TopBar`, `Dock`,
//! `Launcher`, `Notifications`, `Selection`, `Cursors`.

extern crate alloc;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use super::{Desktop, Notice, Status, TOPBAR_H, status_badge, win_key};
use crate::anim::Ease;
use crate::apps::{AppKind, draw_app_icon};
use crate::color::Color;
use crate::geom::{Radii, Rect, Vec2};
use crate::gfx::{GlassStyle, Paint, Shadow};
use crate::icons::Icon;
use crate::input::MouseButtons;
use crate::math;
use crate::system::WallClock;
use crate::tokens::{ThemeMode, blur, motion, radius, text};
use crate::ui::{CursorKind, MenuItem, id, id2};

const WEEKDAYS: [&str; 7] = ["Пн", "Вт", "Ср", "Чт", "Пт", "Сб", "Вс"];
const MONTHS: [&str; 12] = ["янв", "фев", "мар", "апр", "мая", "июн", "июл", "авг", "сен", "окт", "ноя", "дек"];

/// Текст часов «Чт 19 ноя  19:32». Без RTC — время с момента загрузки.
///
/// ```
/// use orbita_ui::system::WallClock;
/// let s = orbita_ui::shell::chrome::clock_text(Some(WallClock { hour: 9, minute: 5, weekday: 3, day: 19, month: 11 }), 0.0);
/// assert_eq!(s, "Чт 19 ноя  09:05");
/// ```
pub fn clock_text(c: Option<WallClock>, uptime: f64) -> String {
    match c {
        Some(c) => format!(
            "{} {} {}  {:02}:{:02}",
            WEEKDAYS[(c.weekday % 7) as usize],
            c.day,
            MONTHS[((c.month.max(1) - 1) % 12) as usize],
            c.hour,
            c.minute
        ),
        None => {
            let s = uptime as u64;
            format!("{:02}:{:02}:{:02}", s / 3600, (s / 60) % 60, s % 60)
        }
    }
}

// ------------------------------------------------------------------ рабочий стол

/// Иконки рабочего стола со скруглённым выделением и рамкой-маркизой.
pub fn desktop_icons(d: &mut Desktop) {
    let t = d.ui.theme;
    let screen = d.ui.screen();
    let mut y = TOPBAR_H + 16.0;
    let mut open: Option<(String, bool)> = None;
    let marquee_rect = d.marquee.map(|start| Rect::from_min_max(start, d.ui.mouse));
    for i in 0..d.icons.len() {
        let r = Rect::new(18.0, y, 92.0, 98.0);
        let rid = id2(id("dicon"), i as u64);
        let resp = d.ui.interact(rid, r);
        if let Some(m) = marquee_rect {
            d.icons[i].selected = !m.intersect(&r).is_empty();
        }
        if resp.pressed {
            for (j, ic) in d.icons.iter_mut().enumerate() {
                ic.selected = j == i;
            }
            if resp.double_clicked {
                open = Some((d.icons[i].path.clone(), d.icons[i].is_dir));
            }
        }
        let sel = d.icons[i].selected;
        let h = d.ui.hover_t(rid, resp.hovered);
        let s = d.ui.anim(id2(rid, 1), if sel { 1.0 } else { 0.0 }, motion::FAST, Ease::Soft);
        if s > 0.01 {
            // `.ob-dicon.is-selected`: тонированное стекло, акцентная рамка.
            d.ui.p.shadow(r, radius::XL, Shadow::new(0.0, 8.0, 24.0, -8.0, Color::hex(0x00000066).fade(s)));
            d.ui.p.backdrop(r, radius::XL, blur::MD * d.ui.look.blur, 1.4);
            d.ui.p.rect_ex(r, Radii::all(radius::XL), Paint::Solid(t.glass_fill_accent.fade(s)), 1.5, t.accent_line.fade(s));
        } else if h > 0.01 {
            d.ui.p.rect(r, radius::XL, Paint::Solid(t.glass_fill_strong.percent(0.7).fade(h)));
        }
        let icon_r = Rect::new(r.x + 18.0, r.y + 10.0, 56.0, 56.0);
        d.ui.icon(d.icons[i].icon, icon_r, t.ink);
        let name = d.icons[i].name.clone();
        let nw = math::min(d.ui.p.measure(&text::LABEL_SM, &name) + 16.0, 88.0);
        let nr = Rect::new(r.center().x - nw * 0.5, icon_r.bottom() + 6.0, nw, 16.0);
        if s > 0.5 {
            d.ui.p.rect(nr, radius::PILL, Paint::Solid(t.accent));
            d.ui.p.text_in(nr, &text::LABEL_SM, &name, t.on_accent, 0.5);
        } else {
            // Лёгкая тень текста для читаемости на обоях.
            d.ui.p.text_in(nr.translate(Vec2::new(0.0, 1.0)), &text::LABEL_SM, &name, t.surface_base.percent(0.55), 0.5);
            d.ui.p.text_in(nr, &text::LABEL_SM, &name, t.ink, 0.5);
        }
        y += 104.0;
    }
    if let Some((path, is_dir)) = open {
        if is_dir {
            d.open(AppKind::Files, Some(&path));
        } else {
            d.open(AppKind::Editor, Some(&path));
        }
    }

    // Пустое место рабочего стола: маркиза и контекстное меню.
    let bg = id("desktop-bg");
    let resp = d.ui.interact(bg, screen);
    if resp.pressed && d.ui.hot == Some(bg) {
        d.marquee = Some(d.ui.mouse);
        for ic in d.icons.iter_mut() {
            ic.selected = false;
        }
        d.center_open = false;
    }
    if let Some(start) = d.marquee {
        if d.ui.input.buttons.has(MouseButtons::LEFT) {
            let m = Rect::from_min_max(start, d.ui.mouse);
            if m.w > 2.0 || m.h > 2.0 {
                // `.ob-marquee`: скруглённая рамка 1.5px + заливка accent 22%.
                d.ui.p.rect_ex(m, Radii::all(math::min(radius::XL, math::min(m.w, m.h) * 0.5)), Paint::Solid(t.accent.percent(0.22)), 1.5, t.accent_line);
            }
        } else {
            d.marquee = None;
        }
    }
    if resp.right_clicked {
        let items = alloc::vec![
            MenuItem::new("Открыть терминал", Some(Icon::ITerminal)).kbd("Ctrl Alt T"),
            MenuItem::new("Открыть проводник", Some(Icon::IFolder)),
            MenuItem::new("Новая папка", Some(Icon::IPlus)),
            MenuItem::sep(),
            MenuItem::new("Обои и оформление", Some(Icon::IPalette)),
            MenuItem::new("Сменить тему", Some(Icon::IDisplay)),
        ];
        let m = d.ui.mouse;
        d.ui.open_popup(bg, m, 240.0, items);
    }
    if let Some(i) = d.ui.take_popup_result(bg) {
        match i {
            0 => {
                d.open(AppKind::Terminal, None);
            }
            1 => {
                d.open(AppKind::Files, None);
            }
            2 => {
                let mut n = 1;
                let path = loop {
                    let p = format!("/home/user/Новая папка {n}");
                    if d.sys.list_dir(&p).is_err() {
                        break p;
                    }
                    n += 1;
                };
                match d.sys.mkdir(&path) {
                    Ok(()) => d.notify(Notice::new("Папка создана", &path, Status::Success)),
                    Err(e) => d.notify(Notice::new("Ошибка", &e, Status::Danger)),
                }
            }
            4 => {
                d.open(AppKind::Settings, Some("appearance"));
            }
            5 => {
                d.settings.mode = if d.settings.mode == ThemeMode::Dark { ThemeMode::Light } else { ThemeMode::Dark };
            }
            _ => {}
        }
    }
}

// ------------------------------------------------------------------ верхняя панель

fn chip(d: &mut Desktop, key: u64, r: Rect, on: bool) -> crate::ui::Resp {
    let resp = d.ui.interact(key, r);
    let h = d.ui.hover_t(key, resp.hovered || on);
    let t = d.ui.theme;
    d.ui.p.rect(r, radius::SM, Paint::Solid(t.accent_soft.fade(h)));
    resp
}

/// Верхняя панель `ob-topbar` (v3): во всю ширину, 40px, плотное стекло.
pub fn topbar(d: &mut Desktop) {
    let screen = d.ui.screen();
    let t = d.ui.theme;
    let bar = Rect::new(0.0, 0.0, screen.w, TOPBAR_H);
    d.ui.layer(10_000, bar);
    let k = d.ui.anim(id("topbar-in"), 1.0, 0.6, Ease::Soft);
    let style = GlassStyle {
        blur: blur::LG * d.ui.look.blur,
        saturate: 1.8,
        fill: t.glass_fill_strong.percent(0.8),
        border: Color::TRANSPARENT,
        highlight: Color::TRANSPARENT,
        noise: if d.ui.look.noise { 0.05 } else { 0.0 },
    };
    d.ui.p.push_opacity(k);
    d.ui.p.glass(bar, 0.0, &style, &[]);
    d.ui.p.rect(Rect::new(0.0, TOPBAR_H - 1.0, screen.w, 1.0), 0.0, Paint::Solid(t.stroke_subtle));

    let mut x = 12.0;
    let cy = 5.0;
    // Логотип — открывает лаунчер.
    let logo_r = Rect::new(x, cy, 34.0, 30.0);
    if chip(d, id("tb-logo"), logo_r, d.launcher.open).clicked {
        d.toggle_launcher();
    }
    let theme = d.ui.theme;
    super::logo::draw(&mut d.ui.p, logo_r.center(), 28.0, &theme, -0.45, 0.0);
    x += 36.0;

    // Имя активного приложения (полужирное) и меню.
    let fi = d.focused_index();
    let app_name = fi.map(|i| String::from(d.windows[i].app.kind().title())).unwrap_or_else(|| String::from("Рабочий стол"));
    let st_app = crate::tokens::TextStyle { face: crate::gfx::text::Face::Sans700, size: 14.0, line: 20.0, tracking: 0.1 };
    let w = d.ui.p.measure(&st_app, &app_name) + 20.0;
    let r = Rect::new(x, cy, w, 30.0);
    chip(d, id("tb-app"), r, false);
    d.ui.p.text_in(r, &st_app, &app_name, t.ink, 0.5);
    x += w + 2.0;
    let menus = fi.map(|i| d.windows[i].app.menus());
    let menu_names = ["Файл", "Правка", "Вид"];
    for (mi, name) in menu_names.iter().enumerate() {
        let w = d.ui.p.measure(&text::LABEL_MD, name) + 20.0;
        let r = Rect::new(x, cy, w, 30.0);
        let key = id2(id("tb-menu"), mi as u64);
        let open = d.ui.popup_open_for(key);
        let resp = chip(d, key, r, open);
        d.ui.p.text_in(r, &text::LABEL_MD, name, t.ink, 0.5);
        if resp.clicked {
            let items = match &menus {
                Some(m) => m[mi].1.clone(),
                None => alloc::vec![
                    MenuItem::new("Терминал", Some(Icon::ITerminal)),
                    MenuItem::new("Проводник", Some(Icon::IFolder)),
                    MenuItem::new("Настройки", Some(Icon::ITune)),
                ],
            };
            d.ui.open_popup(key, Vec2::new(r.x, TOPBAR_H + 4.0), 220.0, items);
        }
        if let Some(choice) = d.ui.take_popup_result(key) {
            topbar_menu_action(d, mi, choice, menus.is_some());
        }
        x += w + 2.0;
    }

    // Часы по центру.
    let clock = clock_text(d.sys.clock(), d.stats.uptime);
    let cw = d.ui.p.measure(&text::LABEL_MD, &clock) + 20.0;
    let cr = Rect::new((screen.w - cw) * 0.5, cy, cw, 30.0);
    if chip(d, id("tb-clock"), cr, d.center_open).clicked {
        d.center_open = !d.center_open;
    }
    d.ui.p.text_in(cr, &text::LABEL_MD, &clock, t.ink, 0.5);

    // Трей справа.
    let mut rx = screen.w - 12.0;
    // Кнопки окна, если активное окно развёрнуто (полноэкранный режим).
    if let Some(i) = fi {
        if d.windows[i].maximized {
            let wid = d.windows[i].id;
            rx -= 3.0 * 40.0;
            let base = win_key(wid, 20);
            let b0 = Rect::new(rx, cy, 38.0, 30.0);
            if d.ui.icon_button(id2(base, 1), b0, Icon::IMinus, false).clicked {
                d.set_minimized(wid, true);
            }
            if d.ui.icon_button(id2(base, 2), b0.translate(Vec2::new(40.0, 0.0)), Icon::IRestore, false).clicked {
                d.toggle_maximize(wid);
            }
            if d.ui.icon_button(id2(base, 3), b0.translate(Vec2::new(80.0, 0.0)), Icon::IClose, true).clicked {
                d.close(wid);
            }
            rx -= 10.0;
            d.ui.p.rect(Rect::new(rx, 10.0, 1.0, 20.0), 0.0, Paint::Solid(t.stroke_default));
            rx -= 10.0;
        }
    }
    let has_unread = !d.notices.is_empty();
    let bell = Rect::new(rx - 38.0, cy, 38.0, 30.0);
    if chip(d, id("tb-bell"), bell, d.center_open).clicked {
        d.center_open = !d.center_open;
        d.launcher.open = false;
    }
    d.ui.icon(Icon::IBell, bell.center_box(18.0, 18.0), t.ink);
    if has_unread {
        d.ui.p.rect(Rect::new(bell.right() - 13.0, bell.y + 3.0, 7.0, 7.0), 3.5, Paint::Solid(t.accent_text));
    }
    rx = bell.x - 2.0;
    let bat_text = "87%";
    let bw = 18.0 + 8.0 + d.ui.p.measure(&text::LABEL_MD, bat_text) + 20.0;
    let bat = Rect::new(rx - bw, cy, bw, 30.0);
    chip(d, id("tb-bat"), bat, false);
    d.ui.icon(Icon::IBattery, Rect::new(bat.x + 10.0, bat.y + 6.0, 18.0, 18.0), t.ink);
    d.ui.p.text_in(Rect::new(bat.x + 36.0, bat.y, bw - 36.0, 30.0), &text::LABEL_MD, bat_text, t.ink, 0.0);
    rx = bat.x - 2.0;
    for (n, icon) in [(1u64, Icon::IVolume), (2, Icon::IWifi)] {
        let r = Rect::new(rx - 38.0, cy, 38.0, 30.0);
        if chip(d, id2(id("tb-tray"), n), r, false).clicked {
            d.center_open = !d.center_open;
        }
        let c = if n == 2 && !d.settings.wifi { t.ink_faint } else { t.ink };
        d.ui.icon(icon, r.center_box(18.0, 18.0), c);
        rx = r.x - 2.0;
    }
    // Счётчик FPS (моноширинный, как числа в дизайн-системе).
    if d.settings.show_fps {
        let s = format!("{:>4.0} FPS", d.stats.fps);
        let w = d.ui.p.measure(&text::MONO_SM, &s) + 20.0;
        let r = Rect::new(rx - w, cy, w, 30.0);
        if chip(d, id("tb-fps"), r, false).clicked {
            d.open(AppKind::Monitor, None);
        }
        let good = d.stats.fps >= 160.0 || d.stats.fps == 0.0;
        let c = if good { t.success } else { t.warning };
        d.ui.p.rect(Rect::new(r.x + 9.0, r.y + 12.0, 6.0, 6.0), 3.0, Paint::Solid(c));
        d.ui.p.text_in(Rect::new(r.x + 18.0, r.y, w - 18.0, 30.0), &text::MONO_SM, &s, t.ink_muted, 0.0);
    }
    d.ui.p.pop_opacity();
}

fn topbar_menu_action(d: &mut Desktop, menu: usize, choice: usize, has_window: bool) {
    let focused = d.focused;
    if !has_window {
        match choice {
            0 => {
                d.open(AppKind::Terminal, None);
            }
            1 => {
                d.open(AppKind::Files, None);
            }
            _ => {
                d.open(AppKind::Settings, None);
            }
        }
        return;
    }
    match (menu, choice) {
        (0, 0) => {
            if let Some(i) = d.focused_index() {
                let k = d.windows[i].app.kind();
                d.open(k, None);
            }
        }
        (0, 1) => {
            d.open(AppKind::Files, None);
        }
        (0, 3) => {
            if let Some(f) = focused {
                d.close(f);
            }
        }
        (2, 0) => {
            if let Some(f) = focused {
                d.toggle_maximize(f);
            }
        }
        (2, 1) => {
            if let Some(f) = focused {
                d.set_minimized(f, true);
            }
        }
        (2, 3) => {
            d.open(AppKind::Settings, Some("appearance"));
        }
        _ => {}
    }
}

// ------------------------------------------------------------------ док

/// Плавающий док `ob-dock` (v3): шесть приложений, увеличение под курсором,
/// точка запущенного приложения, подсказка.
pub fn dock(d: &mut Desktop) {
    let screen = d.ui.screen();
    let t = d.ui.theme;
    let n = AppKind::ALL.len() as f32;
    let item = 44.0;
    let gap = 10.0;
    let w = n * item + (n - 1.0) * gap + 28.0;
    let h = item + 20.0;
    // Док прячется, если активное окно развёрнуто на весь экран.
    let hide = d.focused_index().map(|i| d.windows[i].maximized && !d.windows[i].minimized).unwrap_or(false);
    let near_bottom = d.ui.mouse.y > screen.h - 12.0;
    let hidden = d.ui.anim(id("dock-hide"), if hide && !near_bottom { 1.0 } else { 0.0 }, motion::BASE, Ease::Out);
    let rise = d.ui.anim(id("dock-rise"), 1.0, motion::SLOW, Ease::Out);
    let y = screen.h - h - 14.0 + (1.0 - rise) * 24.0 + hidden * (h + 30.0);
    let r = Rect::new((screen.w - w) * 0.5, y, w, h);
    d.ui.layer(10_001, r.inset(-8.0));
    d.ui.p.push_opacity(rise);
    let style = GlassStyle {
        blur: blur::LG * d.ui.look.blur,
        saturate: 1.8,
        fill: t.glass_fill.percent(0.85),
        border: t.stroke_subtle,
        highlight: t.glass_highlight.percent(0.6),
        noise: if d.ui.look.noise { 0.08 } else { 0.0 },
    };
    let sh = match t.mode {
        ThemeMode::Dark => Shadow::new(0.0, 18.0, 40.0, -12.0, Color::hex(0x00000066)),
        ThemeMode::Light => Shadow::new(0.0, 18.0, 40.0, -12.0, Color::hex(0x1e3a8a40)),
    };
    d.ui.p.glass(r, 26.0, &style, &[sh]);
    // Индекс ближайшей к курсору иконки — для «волны» увеличения.
    let mx = d.ui.mouse.x;
    let over = d.ui.hovering(r.inset(-4.0));
    let mut x = r.x + 14.0;
    for (i, kind) in AppKind::ALL.iter().copied().enumerate() {
        let cell = Rect::new(x, r.y + 10.0, item, item);
        let key = id2(id("dock"), i as u64);
        let resp = d.ui.interact(key, cell);
        let dist = math::abs(mx - cell.center().x) / (item + gap);
        let target = if over {
            if dist < 0.5 { 1.0 } else if dist < 1.5 { 0.42 } else { 0.0 }
        } else {
            0.0
        };
        let m = d.ui.anim(id2(key, 1), target, motion::BASE, Ease::Out);
        // `.is-hover`: translateY(-8px) scale(1.16); `.is-near`: -3px, 1.07.
        let scale = 1.0 + 0.16 * m;
        let lift = 8.0 * m;
        let ic = Rect::new(cell.center().x - item * scale * 0.5, cell.bottom() - item * scale - lift, item * scale, item * scale);
        let press = d.ui.hover_t(id2(key, 2), resp.down);
        d.ui.p.push_transform(ic.center(), 1.0 - 0.06 * press, Vec2::ZERO);
        draw_app_icon(&mut d.ui, kind, ic, true);
        d.ui.p.pop_transform();
        let running = d.windows.iter().any(|w| w.app.kind() == kind && !w.closing);
        let run_t = d.ui.anim(id2(key, 3), if running { 1.0 } else { 0.0 }, motion::BASE, Ease::Out);
        if run_t > 0.01 {
            let dot = Rect::new(cell.center().x - 2.0, cell.bottom() + 4.0, 4.0, 4.0);
            d.ui.p.rect(dot, 2.0, Paint::Solid(t.ink_muted.fade(run_t)));
        }
        if resp.hovered {
            d.ui.cursor = CursorKind::Hand;
            d.ui.show_tooltip(kind.title(), ic);
        }
        if resp.clicked {
            dock_click(d, kind);
        }
        x += item + gap;
    }
    d.ui.p.pop_opacity();
}

fn dock_click(d: &mut Desktop, kind: AppKind) {
    if kind == AppKind::About && !d.windows.iter().any(|w| w.app.kind() == AppKind::About) {
        // Первая иконка дока — «Orbita»: открывает лаунчер, как на макете.
        d.toggle_launcher();
        return;
    }
    // Есть окно приложения: свернуто — развернуть, в фокусе — свернуть,
    // иначе — поднять. Нет окна — открыть.
    let found = d.windows.iter().rev().find(|w| w.app.kind() == kind && !w.closing).map(|w| (w.id, w.minimized));
    match found {
        Some((wid, true)) => d.set_minimized(wid, false),
        Some((wid, false)) => {
            if d.focused == Some(wid) {
                d.set_minimized(wid, true);
            } else {
                d.focus(wid);
            }
        }
        None => {
            d.open(kind, None);
        }
    }
    d.launcher.open = false;
}

// ------------------------------------------------------------------ лаунчер

struct LaunchEntry {
    title: &'static str,
    kind: AppKind,
    arg: Option<&'static str>,
    glyph: Icon,
    style: u8,
}

const LAUNCH: [LaunchEntry; 12] = [
    LaunchEntry { title: "Orbita", kind: AppKind::About, arg: None, glyph: Icon::IOrbit, style: 0 },
    LaunchEntry { title: "Проводник", kind: AppKind::Files, arg: None, glyph: Icon::IFolder, style: 1 },
    LaunchEntry { title: "Терминал", kind: AppKind::Terminal, arg: None, glyph: Icon::ITerminal, style: 2 },
    LaunchEntry { title: "Монитор", kind: AppKind::Monitor, arg: None, glyph: Icon::IDisplay, style: 3 },
    LaunchEntry { title: "Настройки", kind: AppKind::Settings, arg: None, glyph: Icon::ITune, style: 4 },
    LaunchEntry { title: "Редактор", kind: AppKind::Editor, arg: None, glyph: Icon::ICode, style: 5 },
    LaunchEntry { title: "Фото", kind: AppKind::Files, arg: Some("/home/user/Изображения"), glyph: Icon::IImage, style: 6 },
    LaunchEntry { title: "Архивы", kind: AppKind::Files, arg: Some("/home/user/Загрузки"), glyph: Icon::IArchive, style: 1 },
    LaunchEntry { title: "Сеть", kind: AppKind::Settings, arg: Some("network"), glyph: Icon::IWifi, style: 1 },
    LaunchEntry { title: "Экран", kind: AppKind::Settings, arg: Some("display"), glyph: Icon::IDisplay, style: 5 },
    LaunchEntry { title: "Оформление", kind: AppKind::Settings, arg: Some("appearance"), glyph: Icon::IPalette, style: 6 },
    LaunchEntry { title: "Защита", kind: AppKind::Settings, arg: Some("security"), glyph: Icon::ILock, style: 4 },
];

fn launch_icon(d: &mut Desktop, e: &LaunchEntry, r: Rect) {
    let t = d.ui.theme;
    let paint = match e.style {
        0 => Paint::Radial { cx: 0.3, cy: 0.2, rx: 0.8, ry: 0.8, c0: t.amb_cyan, c1: t.amb_blue, c2: t.accent_deep, mid: 0.55, end: 1.0 },
        1 => Paint::linear(145.0, t.amb_cyan, t.amb_blue),
        2 => Paint::linear(145.0, Color::rgb_hex(0x2a2d38), Color::rgb_hex(0x0d0e12)),
        3 => Paint::Linear { angle: 145.0, c0: t.amb_teal, c1: t.amb_blue, c2: t.accent_deep, mid: 0.7 },
        4 => Paint::linear(145.0, Color::rgb_hex(0x6b7385), Color::rgb_hex(0x2c303b)),
        5 => Paint::linear(145.0, t.amb_indigo, t.accent_deep),
        _ => Paint::Linear { angle: 145.0, c0: t.amb_amber, c1: t.amb_rose, c2: t.amb_indigo, mid: 0.6 },
    };
    let rad = r.w * 0.3;
    d.ui.p.shadow(r, rad, Shadow::new(0.0, 4.0, 12.0, 0.0, Color::hex(0x00000040)));
    d.ui.p.rect(r, rad, paint);
    d.ui.p.noise(r, rad, 0.14);
    let g = r.w * 0.5;
    d.ui.icon_w(e.glyph, r.center_box(g, g), Color::WHITE, 2.0 / 1.75);
}

/// Лаунчер на весь экран: размытый фон, поиск-капсула, сетка 6×2.
pub fn launcher(d: &mut Desktop) {
    let k = d.ui.anim(id("launcher"), if d.launcher.open { 1.0 } else { 0.0 }, motion::BASE + 0.06, Ease::Out);
    if k < 0.003 {
        return;
    }
    let screen = d.ui.screen();
    let t = d.ui.theme;
    d.ui.layer(50_000, screen);
    d.ui.p.push_opacity(k);
    d.ui.p.backdrop(screen, 0.0, blur::XL * math::max(d.ui.look.blur, 0.4), 1.5);
    let dim = match t.mode {
        ThemeMode::Dark => Color::hex(0x0b0f1c99),
        ThemeMode::Light => Color::hex(0xffffff59),
    };
    d.ui.p.rect(screen, 0.0, Paint::Solid(dim));
    d.ui.p.noise(screen, 0.0, 0.05);
    d.ui.p.push_transform(screen.center(), 1.04 - 0.04 * k, Vec2::ZERO);

    // Поиск.
    let sw = 420.0;
    let sr = Rect::new((screen.w - sw) * 0.5, TOPBAR_H + 40.0, sw, 42.0);
    let sid = id("launcher-search");
    d.ui.p.rect(sr, radius::PILL, Paint::Solid(t.glass_fill_strong));
    d.ui.p.stroke_rect(sr, radius::PILL, 1.0, t.stroke_subtle);
    let mut search = core::mem::take(&mut d.launcher.search);
    d.ui.focus = Some(sid);
    let resp = d.ui.text_field(sid, sr.inset_xy(8.0, 3.0), &mut search, "Поиск приложений и файлов", Some(Icon::ISearch));
    let q = search.text.to_lowercase();
    d.launcher.search = search;

    // Сетка 6×2.
    let cols = 6usize;
    let cell_w = 128.0;
    let cell_h = 120.0;
    let gx = (screen.w - cols as f32 * cell_w) * 0.5;
    let gy = sr.bottom() + 56.0;
    let mut launched: Option<(AppKind, Option<&'static str>)> = None;
    let mut shown = 0usize;
    let mut first: Option<(AppKind, Option<&'static str>)> = None;
    for (i, e) in LAUNCH.iter().enumerate() {
        if !q.is_empty() && !e.title.to_lowercase().contains(&q) {
            continue;
        }
        if first.is_none() {
            first = Some((e.kind, e.arg));
        }
        let col = shown % cols;
        let row = shown / cols;
        shown += 1;
        let cell = Rect::new(gx + col as f32 * cell_w, gy + row as f32 * cell_h, cell_w - 8.0, cell_h - 8.0);
        let key = id2(id("launch"), i as u64);
        let r = d.ui.interact(key, cell);
        let h = d.ui.hover_t(key, r.hovered);
        // Появление «лесенкой», как `ob-rise` с задержкой.
        let stagger = d.ui.anim(id2(key, 1), if d.launcher.open { 1.0 } else { 0.0 }, motion::SLOW + i as f32 * 0.02, Ease::Out);
        d.ui.p.push_transform(cell.center(), 0.9 + 0.1 * stagger, Vec2::new(0.0, 12.0 * (1.0 - stagger)));
        if h > 0.01 {
            d.ui.p.rect_ex(cell, Radii::all(radius::LG), Paint::Solid(t.glass_fill_accent.fade(h)), 1.5, t.accent_line.fade(h));
        }
        let ir = Rect::new(cell.center().x - 28.0, cell.y + 14.0, 56.0, 56.0);
        launch_icon(d, e, ir);
        let lr = Rect::new(cell.x, ir.bottom() + 10.0, cell.w, 18.0);
        d.ui.p.text_in(lr, &text::LABEL_SM, e.title, t.ink, 0.5);
        d.ui.p.pop_transform();
        if r.clicked {
            launched = Some((e.kind, e.arg));
        }
        if r.hovered {
            d.ui.cursor = CursorKind::Hand;
        }
    }
    if shown == 0 {
        let r = Rect::new(0.0, gy + 40.0, screen.w, 24.0);
        d.ui.p.text_in(r, &text::BODY_LG, "Ничего не найдено", t.ink_muted, 0.5);
    }
    if resp.clicked {
        launched = first;
    }
    // Индикатор страниц.
    let py = gy + 2.0 * cell_h + 24.0;
    for i in 0..3 {
        let w = if i == 0 { 18.0 } else { 6.0 };
        let x = screen.w * 0.5 - 22.0 + i as f32 * 14.0 + if i > 0 { 12.0 } else { 0.0 };
        let c = if i == 0 { t.ink } else { t.ink_faint };
        d.ui.p.rect(Rect::new(x, py, w, 6.0), 3.0, Paint::Solid(c));
    }
    d.ui.p.pop_transform();
    d.ui.p.pop_opacity();
    // Клик мимо иконок закрывает лаунчер.
    let bg = d.ui.interact(id("launcher-bg"), screen);
    if bg.clicked && d.ui.hot == Some(id("launcher-bg")) {
        d.launcher.open = false;
    }
    if let Some((kind, arg)) = launched {
        d.launcher.open = false;
        d.ui.focus = None;
        d.open(kind, arg);
    }
}

// ------------------------------------------------------------------ центр управления

/// Центр управления `ob-center`: быстрые настройки, громкость, уведомления.
pub fn control_center(d: &mut Desktop) {
    let k = d.ui.anim(id("center"), if d.center_open { 1.0 } else { 0.0 }, motion::BASE, Ease::Out);
    if k < 0.003 {
        return;
    }
    let screen = d.ui.screen();
    let t = d.ui.theme;
    let w = 340.0;
    let list_n = d.notices.len().min(4);
    let h = 16.0 + 28.0 + 12.0 + 112.0 + 12.0 + 28.0 + 12.0 + list_n as f32 * 72.0 + if list_n == 0 { 40.0 } else { 0.0 } + 4.0;
    let r = Rect::new(screen.w - w - 12.0, TOPBAR_H + 10.0, w, h);
    d.ui.layer(60_000, r);
    d.ui.p.push_opacity(k);
    d.ui.p.push_transform(Vec2::new(r.right(), r.y), 0.97 + 0.03 * k, Vec2::new(0.0, -6.0 * (1.0 - k)));
    let style = d.ui.menu_style();
    let sh = d.ui.menu_shadow();
    d.ui.p.glass(r, radius::XL, &style, &sh);
    let inner = r.inset(16.0);
    let mut y = inner.y;
    d.ui.p.text(Vec2::new(inner.x, y + 4.0), &text::TITLE_SM, "Центр управления", t.ink);
    let cw = d.ui.button_width("Очистить", crate::ui::ButtonSize::Sm, false);
    if d.ui
        .button(id("center-clear"), Rect::new(inner.right() - cw, y, cw, 28.0), "Очистить", crate::ui::ButtonKind::Ghost, crate::ui::ButtonSize::Sm, None)
        .clicked
    {
        d.notices.clear();
        d.toasts.clear();
    }
    y += 40.0;
    // Плитки 2×2.
    let tw = (inner.w - 8.0) * 0.5;
    let dark = d.settings.mode == ThemeMode::Dark;
    let tiles: [(Icon, &str, &str, bool); 4] = [
        (Icon::IWifi, "Wi‑Fi", if d.settings.wifi { "orbita-net" } else { "Выкл" }, d.settings.wifi),
        (Icon::IBell, "Не беспокоить", if d.settings.dnd { "Вкл" } else { "Выкл" }, d.settings.dnd),
        (Icon::ILock, "Защита", "Активна", true),
        (Icon::IDisplay, "Тема", if dark { "Тёмная" } else { "Светлая" }, false),
    ];
    for (i, (icon, title, sub, on)) in tiles.iter().enumerate() {
        let tr = Rect::new(inner.x + (i % 2) as f32 * (tw + 8.0), y + (i / 2) as f32 * 60.0, tw, 52.0);
        let key = id2(id("qtile"), i as u64);
        let resp = d.ui.interact(key, tr);
        let h = d.ui.hover_t(key, resp.hovered);
        let s = d.ui.anim(id2(key, 1), if *on { 1.0 } else { 0.0 }, motion::BASE, Ease::Soft);
        let bg = t.stroke_subtle.mix(t.stroke_default, h * (1.0 - s)).mix(t.accent, s);
        d.ui.p.rect(tr, radius::LG, Paint::Solid(bg));
        let fg = t.ink.mix(t.on_accent, s);
        d.ui.icon(*icon, Rect::new(tr.x + 12.0, tr.y + 17.0, 18.0, 18.0), fg);
        let st = crate::tokens::TextStyle { face: crate::gfx::text::Face::Sans500, size: 13.0, line: 16.0, tracking: 0.0 };
        d.ui.p.text(Vec2::new(tr.x + 40.0, tr.y + 9.0), &st, title, fg);
        let sm = crate::tokens::TextStyle { face: crate::gfx::text::Face::Sans400, size: 11.0, line: 14.0, tracking: 0.0 };
        d.ui.p.text(Vec2::new(tr.x + 40.0, tr.y + 27.0), &sm, sub, t.ink_muted.mix(t.on_accent, s));
        if resp.clicked {
            match i {
                0 => d.settings.wifi = !d.settings.wifi,
                1 => d.settings.dnd = !d.settings.dnd,
                2 => {
                    d.open(AppKind::Settings, Some("security"));
                }
                _ => d.settings.mode = if dark { ThemeMode::Light } else { ThemeMode::Dark },
            }
        }
    }
    y += 120.0;
    // Громкость.
    d.ui.icon(Icon::IVolume, Rect::new(inner.x, y + 5.0, 18.0, 18.0), t.ink_muted);
    let mut vol = d.settings.volume;
    d.ui.slider(id("center-vol"), Rect::new(inner.x + 30.0, y + 4.0, inner.w - 34.0, 20.0), &mut vol);
    d.settings.volume = vol;
    y += 40.0;
    // Уведомления.
    if d.notices.is_empty() {
        d.ui.p.text_in(Rect::new(inner.x, y, inner.w, 32.0), &text::BODY_SM, "Новых уведомлений нет", t.ink_muted, 0.5);
    }
    let now = d.ui.now;
    let mut dismiss = None;
    for (i, n) in d.notices.iter().take(4).enumerate() {
        let nr = Rect::new(inner.x, y, inner.w, 64.0);
        let key = id2(id("notif"), n.id);
        let resp = d.ui.interact(key, nr);
        let h = d.ui.hover_t(key, resp.hovered);
        d.ui.p.rect(nr, radius::LG, Paint::Solid(t.stroke_subtle.mix(t.stroke_default, h)));
        status_badge(&mut d.ui, Rect::new(nr.x + 12.0, nr.y + 16.0, 32.0, 32.0), n.status, n.icon);
        let tx = nr.x + 56.0;
        let ago = ago_text(now - n.time);
        let aw = d.ui.p.measure(&text::LABEL_SM, &ago);
        d.ui.p.text_in(Rect::new(tx, nr.y + 12.0, nr.right() - tx - aw - 18.0, 20.0), &text::LABEL_MD, &n.title, t.ink, 0.0);
        d.ui.p.text(Vec2::new(nr.right() - aw - 12.0, nr.y + 14.0), &text::LABEL_SM, &ago, t.ink_muted);
        d.ui.p.text_in(Rect::new(tx, nr.y + 32.0, nr.right() - tx - 12.0, 18.0), &text::BODY_SM, &n.text, t.ink_muted, 0.0);
        if resp.clicked {
            dismiss = Some(i);
        }
        y += 72.0;
    }
    if let Some(i) = dismiss {
        d.notices.remove(i);
    }
    d.ui.p.pop_transform();
    d.ui.p.pop_opacity();
    if d.ui.left_pressed() && !r.contains(d.ui.mouse) && d.ui.hover_layer() < 10_000 {
        d.center_open = false;
    }
}

fn ago_text(dt: f64) -> String {
    let s = dt as u64;
    if s < 60 {
        String::from("сейчас")
    } else if s < 3600 {
        format!("{} мин", s / 60)
    } else {
        format!("{} ч", s / 3600)
    }
}

// ------------------------------------------------------------------ тосты

/// Время жизни тоста, сек.
const TOAST_LIFE: f64 = 6.0;

/// Тосты `ob-toast`: справа сверху, въезжают справа, гаснут через 6 с.
pub fn toasts(d: &mut Desktop) {
    if d.center_open {
        // Пока открыт центр управления, тосты в нём же — не перекрываем его.
        d.toasts.clear();
        return;
    }
    let screen = d.ui.screen();
    let t = d.ui.theme;
    let now = d.ui.now;
    let mut y = TOPBAR_H + 12.0;
    let ids: Vec<u64> = d.toasts.clone();
    let mut expired = Vec::new();
    for tid in ids.iter().rev().take(4) {
        let Some(n) = d.notices.iter().find(|n| n.id == *tid).cloned() else {
            expired.push(*tid);
            continue;
        };
        let age = now - n.time;
        let key = id2(id("toast"), n.id);
        let enter = d.ui.anims.get(key, now).unwrap_or(1.0);
        if !d.ui.anims.finished(key, now) {
            d.ui.anims.mark_active();
        }
        let leave = if age > TOAST_LIFE { math::clamp(((age - TOAST_LIFE) / 0.3) as f32, 0.0, 1.0) } else { 0.0 };
        if leave >= 1.0 {
            expired.push(n.id);
            continue;
        }
        if age <= TOAST_LIFE + 0.4 {
            d.ui.anims.mark_active();
        }
        let a = enter * (1.0 - leave);
        let w = 340.0;
        let h = 72.0;
        let r = Rect::new(screen.w - w - 16.0, y, w, h);
        d.ui.layer(70_000, r);
        d.ui.p.push_opacity(a);
        d.ui.p.push_transform(r.center(), 0.98 + 0.02 * enter, Vec2::new(28.0 * (1.0 - enter) + 28.0 * leave, 0.0));
        let style = d.ui.menu_style();
        let sh = d.ui.menu_shadow();
        d.ui.p.glass(r, radius::XL, &style, &sh);
        status_badge(&mut d.ui, Rect::new(r.x + 12.0, r.y + 16.0, 40.0, 40.0), n.status, n.icon);
        let tx = r.x + 64.0;
        d.ui.p.text_in(Rect::new(tx, r.y + 14.0, r.w - 120.0, 20.0), &text::TITLE_SM.with_face(crate::gfx::text::Face::Sans600), &n.title, t.ink, 0.0);
        d.ui.p.text(Vec2::new(r.right() - 56.0, r.y + 16.0), &text::LABEL_SM, "сейчас", t.ink_muted);
        d.ui.p.text_in(Rect::new(tx, r.y + 36.0, r.w - 80.0, 18.0), &text::BODY_SM, &n.text, t.ink_muted, 0.0);
        // Полоса оставшегося времени.
        let left = 1.0 - math::clamp((age / TOAST_LIFE) as f32, 0.0, 1.0);
        d.ui.p.rect(Rect::new(tx, r.bottom() - 10.0, (r.w - 80.0) * left, 3.0), 1.5, Paint::Solid(t.accent_text.percent(0.6)));
        let resp = d.ui.interact(key, r);
        if resp.clicked {
            expired.push(n.id);
        }
        d.ui.p.pop_transform();
        d.ui.p.pop_opacity();
        y += (h + 10.0) * a;
    }
    d.toasts.retain(|t| !expired.contains(t));
}

// ------------------------------------------------------------------ загрузчик

/// Длительность анимации загрузки: заставка + растворение.
pub const INTRO_HOLD: f64 = 1.4;
/// Длительность растворения заставки в рабочий стол.
pub const INTRO_FADE: f64 = 0.8;

/// Анимированный загрузчик: логотип Orbita на плавно переплетающихся
/// ambient-градиентах, затем растворение в рабочий стол.
///
/// Рисуется поверх всего (кроме курсора) в первые
/// `INTRO_HOLD + INTRO_FADE` секунд после первого кадра. Первый кадр
/// совпадает с ранней заставкой ядра (UEFI GOP), поэтому переход незаметен.
pub fn intro(d: &mut Desktop) {
    let Some(t0) = d.intro_start else { return };
    let t = d.ui.now - t0;
    if t >= INTRO_HOLD + INTRO_FADE {
        d.intro_start = None;
        return;
    }
    d.ui.anims.mark_active();
    let screen = d.ui.screen();
    let th = crate::tokens::DARK;
    // Растворение: 1 → 0 по кривой --ob-ease-soft.
    let fade = if t < INTRO_HOLD { 1.0 } else { 1.0 - crate::anim::Ease::Soft.apply(((t - INTRO_HOLD) / INTRO_FADE) as f32) };
    d.ui.layer(90_000, if fade > 0.5 { screen } else { Rect::ZERO });
    d.ui.p.push_opacity(fade);
    d.ui.p.rect(screen, 0.0, Paint::Solid(th.surface_base));
    // Четыре пятна медленно движутся по эллипсам с разными периодами.
    let ph = t as f32;
    let blob = |p: &mut crate::gfx::Painter, cx: f32, cy: f32, r: f32, c: Color, k: f32| {
        p.rect(screen, 0.0, Paint::blob(cx, cy, r, r * 1.2, c.percent(k), 0.75));
    };
    blob(&mut d.ui.p, 0.5 + 0.18 * math::sin(ph * 0.9), 1.05 + 0.06 * math::cos(ph * 0.7), 0.7, th.amb_blue, 0.6);
    blob(&mut d.ui.p, 0.15 + 0.08 * math::cos(ph * 0.6), -0.05 + 0.08 * math::sin(ph * 0.8), 0.45, th.amb_indigo, 0.4);
    blob(&mut d.ui.p, 0.9 + 0.07 * math::sin(ph * 0.75 + 1.0), 0.25 + 0.1 * math::cos(ph * 0.5), 0.4, th.amb_cyan, 0.28 * math::min(1.0, ph));
    blob(&mut d.ui.p, 0.1 + 0.06 * math::sin(ph * 0.55 + 2.0), 0.9 + 0.05 * math::cos(ph * 0.65), 0.35, th.amb_teal, 0.22 * math::min(1.0, ph));
    d.ui.p.noise(screen, 0.0, 0.05);
    // Логотип: лёгкое «дыхание», при растворении — увеличение.
    let c = screen.center();
    let breathe = 1.0 + 0.03 * math::sin(ph * 2.4);
    let grow = 1.0 + 0.25 * (1.0 - fade);
    // Спутник облетает планету — индикатор загрузки (≈1.6 с на круг).
    let angle = -0.45 + ph * 3.9;
    super::logo::draw(&mut d.ui.p, c, 128.0 * breathe * grow, &th, angle, 1.0);
    d.ui.p.pop_opacity();
}

// ------------------------------------------------------------------ курсор

/// Рисует курсор дизайн-системы (основной или акцентный набор).
pub fn cursor(d: &mut Desktop) {
    if d.ui.input.mouse.is_none() {
        return;
    }
    let accent = d.ui.look.accent_cursor;
    let (icon, hot) = match d.ui.cursor {
        CursorKind::Arrow => (if accent { Icon::CArrowA } else { Icon::CArrow }, Vec2::new(7.0, 4.0)),
        CursorKind::Hand => (if accent { Icon::CHandA } else { Icon::CHand }, Vec2::new(14.0, 4.0)),
        CursorKind::Text => (if accent { Icon::CTextA } else { Icon::CText }, Vec2::new(16.0, 16.0)),
        CursorKind::Move => (if accent { Icon::CMoveA } else { Icon::CMove }, Vec2::new(16.0, 16.0)),
        CursorKind::ResizeH => (if accent { Icon::CRszA } else { Icon::CRsz }, Vec2::new(16.0, 16.0)),
        CursorKind::Busy => (if accent { Icon::CBusyA } else { Icon::CBusy }, Vec2::new(16.0, 16.0)),
    };
    let size = 28.0;
    let k = size / 32.0;
    let m = d.ui.mouse;
    let r = Rect::new(m.x - hot.x * k, m.y - hot.y * k, size, size);
    d.ui.layer(u32::MAX, Rect::ZERO);
    let theme = d.ui.theme;
    d.ui.p.icon(icon, r, theme.cursor_fill, &theme, 1.0);
}
