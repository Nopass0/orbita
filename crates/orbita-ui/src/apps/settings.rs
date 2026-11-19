//! Настройки (`Settings`, `FullscreenSettings`): разделы слева, справа —
//! группы строк «название, пояснение, контрол».

extern crate alloc;

use alloc::format;

use super::AppCx;
use crate::color::Color;
use crate::geom::{Radii, Rect, Vec2};
use crate::gfx::Paint;
use crate::icons::Icon;
use crate::math;
use crate::shell::{Notice, Status, Wallpaper, status_color, wallpaper};
use crate::tokens::{AccentChoice, ThemeMode, noise, radius, text};
use crate::ui::{ButtonKind, ButtonSize, CursorKind, TextEdit, Ui, id2};

/// Раздел настроек.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Section {
    /// Экран и производительность.
    Display,
    /// Оформление.
    Appearance,
    /// Сеть.
    Network,
    /// Уведомления.
    Notifications,
    /// Безопасность.
    Security,
    /// Аккаунт.
    Account,
    /// О системе.
    About,
}

impl Section {
    const ALL: [Section; 7] = [
        Section::Display,
        Section::Appearance,
        Section::Network,
        Section::Notifications,
        Section::Security,
        Section::Account,
        Section::About,
    ];

    fn title(self) -> &'static str {
        match self {
            Section::Display => "Экран",
            Section::Appearance => "Оформление",
            Section::Network => "Сеть",
            Section::Notifications => "Уведомления",
            Section::Security => "Безопасность",
            Section::Account => "Аккаунт",
            Section::About => "О системе",
        }
    }

    fn icon(self) -> Icon {
        match self {
            Section::Display => Icon::IDisplay,
            Section::Appearance => Icon::IPalette,
            Section::Network => Icon::IWifi,
            Section::Notifications => Icon::IBell,
            Section::Security => Icon::ILock,
            Section::Account => Icon::IUser,
            Section::About => Icon::IOrbit,
        }
    }

    fn key(self) -> &'static str {
        match self {
            Section::Display => "display",
            Section::Appearance => "appearance",
            Section::Network => "network",
            Section::Notifications => "notifications",
            Section::Security => "security",
            Section::Account => "account",
            Section::About => "about",
        }
    }
}

/// Состояние окна настроек.
pub struct SettingsApp {
    section: Section,
    search: TextEdit,
    scroll: f32,
}

impl SettingsApp {
    /// Открывает настройки на разделе `arg` (`appearance`, `display`, …).
    pub fn new(arg: Option<&str>) -> SettingsApp {
        let section = arg
            .and_then(|a| Section::ALL.iter().copied().find(|s| s.key() == a))
            .unwrap_or(Section::Appearance);
        SettingsApp { section, search: TextEdit::default(), scroll: 0.0 }
    }

    /// Ключ текущего раздела (для сессии).
    pub fn section_key(&self) -> &'static str {
        self.section.key()
    }

    /// Рисует окно.
    pub fn ui(&mut self, cx: &mut AppCx, body: Rect) {
        let t = cx.ui.theme;
        let (side, main) = body.split_left(220.0);
        cx.ui.p.rect(side, Radii { tl: 0.0, tr: 0.0, br: 0.0, bl: radius::XL }, Paint::Solid(t.surface_sunken.percent(0.26)));
        let sx = side.x + 12.0;
        let sid = cx.id("search");
        cx.ui.text_field(sid, Rect::new(sx, side.y + 8.0, side.w - 24.0, 36.0), &mut self.search, "Поиск", Some(Icon::ISearch));
        let q = self.search.text.to_lowercase();
        let mut y = side.y + 56.0;
        for (i, s) in Section::ALL.iter().enumerate() {
            if !q.is_empty() && !s.title().to_lowercase().contains(&q) {
                continue;
            }
            let r = Rect::new(sx, y, side.w - 24.0, 34.0);
            if cx.ui.nav_item(id2(cx.id("sec"), i as u64), r, Some(s.icon()), s.title(), self.section == *s).clicked {
                self.section = *s;
                self.scroll = 0.0;
            }
            y += 36.0;
        }

        let content = main.inset_xy(28.0, 16.0);
        let content_h = match self.section {
            Section::Appearance => 640.0,
            Section::Display => 560.0,
            _ => 520.0,
        };
        let sid = cx.id("scroll");
        let off = cx.ui.scroll(sid, content, content_h, &mut self.scroll);
        cx.ui.push_clip(content.inset_xy(-8.0, -4.0), 0.0);
        let area = Rect::new(content.x, content.y - off, content.w - 10.0, content_h);
        match self.section {
            Section::Appearance => self.appearance(cx, area),
            Section::Display => self.display(cx, area),
            Section::Network => self.network(cx, area),
            Section::Notifications => self.notifications(cx, area),
            Section::Security => self.security(cx, area),
            Section::Account => self.account(cx, area),
            Section::About => super::about::about_page(cx, area),
        }
        cx.ui.pop_clip();
    }

    fn appearance(&mut self, cx: &mut AppCx, a: Rect) {
        let t = cx.ui.theme;
        cx.ui.p.text(Vec2::new(a.x, a.y), &text::TITLE_LG, "Оформление", t.ink);
        // Превью обоев — те же обои, уменьшенные трансформацией.
        let mut x = a.x;
        let y = a.y + 46.0;
        let n = 4usize;
        let tw = math::min(150.0, (a.w - (n as f32 - 1.0) * 12.0) / n as f32);
        let th = tw * 10.0 / 16.0;
        let screen = cx.ui.screen();
        let choices = [Wallpaper::WallA, Wallpaper::WallB, Wallpaper::Aurora, Wallpaper::Linear];
        for (i, wp) in choices.iter().enumerate() {
            let r = Rect::new(x, y, tw, th);
            let id = id2(cx.id("wp"), i as u64);
            let resp = cx.ui.interact(id, r);
            let on = cx.settings.wallpaper == *wp;
            let h = cx.ui.hover_t(id, resp.hovered);
            cx.ui.push_clip(r, radius::MD);
            let k = tw / screen.w;
            cx.ui.p.push_transform(Vec2::ZERO, k, Vec2::new(r.x, r.y));
            let theme = cx.ui.theme;
            let full = Rect::new(0.0, 0.0, screen.w, screen.w * 10.0 / 16.0);
            wallpaper::draw(&mut cx.ui.p, &theme, *wp, full, false);
            cx.ui.p.pop_transform();
            cx.ui.pop_clip();
            if on {
                cx.ui.p.stroke_rect(r.inset(-4.0), radius::MD + 4.0, 2.0, t.accent_line);
            } else if h > 0.01 {
                cx.ui.p.stroke_rect(r.inset(-4.0), radius::MD + 4.0, 2.0, t.stroke_default.fade(h));
            }
            if resp.clicked {
                cx.settings.wallpaper = *wp;
            }
            if resp.hovered {
                cx.ui.cursor = CursorKind::Hand;
            }
            x += tw + 12.0;
        }
        // Группа 1: тема, акцент, обои.
        let g1 = Rect::new(a.x, y + th + 24.0, a.w, 3.0 * 64.0 + 8.0);
        group(&mut cx.ui, g1);
        let mut ry = g1.y + 4.0;
        row_label(&mut cx.ui, g1, ry, "Тема", Some("Фон #1f1f1f или белый"));
        let mut sel = if cx.settings.mode == ThemeMode::Dark { 0 } else { 1 };
        let seg = Rect::new(g1.right() - 16.0 - 230.0, ry + 14.0, 230.0, 36.0);
        if cx.ui.segmented(cx.id("theme"), seg, &["Тёмная", "Светлая"], &mut sel) {
            cx.settings.mode = if sel == 0 { ThemeMode::Dark } else { ThemeMode::Light };
        }
        ry += 64.0;
        divider(&mut cx.ui, g1, ry);
        row_label(&mut cx.ui, g1, ry, "Акцентный цвет", Some("Кнопки, выбор, фокус"));
        let mut sxp = g1.right() - 16.0 - AccentChoice::ALL.len() as f32 * 38.0 + 12.0;
        for (i, ac) in AccentChoice::ALL.iter().enumerate() {
            let base = crate::tokens::Theme::for_mode(cx.settings.mode);
            let c = ac.swatch(&base);
            let r = Rect::new(sxp, ry + 19.0, 26.0, 26.0);
            let id = id2(cx.id("swatch"), i as u64);
            let resp = cx.ui.interact(id, r.inset(-4.0));
            cx.ui.p.rect(r, 13.0, Paint::Solid(c));
            if cx.settings.accent == *ac {
                cx.ui.p.stroke_rect(r.inset(-4.0), 17.0, 2.0, t.accent_line);
            }
            if resp.clicked {
                cx.settings.accent = *ac;
            }
            if resp.hovered {
                cx.ui.cursor = CursorKind::Hand;
            }
            sxp += 38.0;
        }
        ry += 64.0;
        divider(&mut cx.ui, g1, ry);
        row_label(&mut cx.ui, g1, ry, "Фон рабочего стола", None);
        let names: [&str; 7] = [
            Wallpaper::ALL[0].title(),
            Wallpaper::ALL[1].title(),
            Wallpaper::ALL[2].title(),
            Wallpaper::ALL[3].title(),
            Wallpaper::ALL[4].title(),
            Wallpaper::ALL[5].title(),
            Wallpaper::ALL[6].title(),
        ];
        let sr = Rect::new(g1.right() - 16.0 - 200.0, ry + 14.0, 200.0, 36.0);
        if let Some(i) = cx.ui.select(cx.id("wp-select"), sr, &names, cx.settings.wallpaper.index()) {
            cx.settings.wallpaper = Wallpaper::from_index(i);
        }

        // Группа 2: блюр, шум, анимации, курсор.
        let g2 = Rect::new(a.x, g1.bottom() + 16.0, a.w, 4.0 * 64.0 + 8.0);
        group(&mut cx.ui, g2);
        let mut ry = g2.y + 4.0;
        row_label(&mut cx.ui, g2, ry, "Блюр стекла", Some(&format!("{}%", (cx.settings.look.blur * 100.0) as u32)));
        let mut v = cx.settings.look.blur / 1.5;
        if cx.ui.slider(cx.id("blur"), Rect::new(g2.right() - 16.0 - 220.0, ry + 22.0, 220.0, 20.0), &mut v) {
            cx.settings.look.blur = v * 1.5;
        }
        ry += 64.0;
        divider(&mut cx.ui, g2, ry);
        row_label(&mut cx.ui, g2, ry, "Шум на стекле", Some("Зерно поверх блюра"));
        let mut on = cx.settings.look.noise;
        cx.ui.toggle(cx.id("noise"), Rect::new(g2.right() - 16.0 - 44.0, ry + 19.0, 44.0, 26.0), &mut on);
        cx.settings.look.noise = on;
        ry += 64.0;
        divider(&mut cx.ui, g2, ry);
        row_label(&mut cx.ui, g2, ry, "Анимации окон", Some("Плавное открытие и перенос"));
        let mut on = cx.settings.look.animations;
        cx.ui.toggle(cx.id("anim"), Rect::new(g2.right() - 16.0 - 44.0, ry + 19.0, 44.0, 26.0), &mut on);
        cx.settings.look.animations = on;
        ry += 64.0;
        divider(&mut cx.ui, g2, ry);
        row_label(&mut cx.ui, g2, ry, "Курсор", Some("Основной или акцентный набор"));
        let mut cur = cx.settings.look.accent_cursor as usize;
        if cx.ui.segmented(cx.id("cursor"), Rect::new(g2.right() - 16.0 - 230.0, ry + 14.0, 230.0, 36.0), &["Основной", "Акцентный"], &mut cur) {
            cx.settings.look.accent_cursor = cur == 1;
        }
    }

    fn display(&mut self, cx: &mut AppCx, a: Rect) {
        let t = cx.ui.theme;
        cx.ui.p.text(Vec2::new(a.x, a.y), &text::TITLE_LG, "Экран", t.ink);
        let attn = Rect::new(a.x, a.y + 46.0, a.w, 112.0);
        let fps = format!("{:.0} FPS", cx.stats.fps);
        let sub = format!("Кадр: CPU {:.2} мс · GPU {:.2} мс · {} примитивов", cx.stats.cpu_ms, cx.stats.gpu_ms, cx.stats.instances);
        attention(&mut cx.ui, attn, &fps, &sub);
        let g = Rect::new(a.x, attn.bottom() + 16.0, a.w, 2.0 * 64.0 + 8.0);
        group(&mut cx.ui, g);
        let mut ry = g.y + 4.0;
        row_label(&mut cx.ui, g, ry, "Показывать FPS", Some("Счётчик в верхней панели"));
        let mut on = cx.settings.show_fps;
        cx.ui.toggle(cx.id("fps"), Rect::new(g.right() - 60.0, ry + 19.0, 44.0, 26.0), &mut on);
        cx.settings.show_fps = on;
        ry += 64.0;
        divider(&mut cx.ui, g, ry);
        row_label(&mut cx.ui, g, ry, "Вертикальная синхронизация", Some("Выключено — без ограничения частоты"));
        let mut on = cx.settings.vsync;
        cx.ui.toggle(cx.id("vsync"), Rect::new(g.right() - 60.0, ry + 19.0, 44.0, 26.0), &mut on);
        cx.settings.vsync = on;
        let info = cx.sys.info();
        let screen = cx.ui.screen();
        let scale = cx.ui.p.scale;
        let g2 = Rect::new(a.x, g.bottom() + 16.0, a.w, 4.0 * 48.0 + 8.0);
        group(&mut cx.ui, g2);
        let rows = [
            ("Разрешение", format!("{}×{} (×{:.2})", (screen.w * scale) as u32, (screen.h * scale) as u32, scale)),
            ("Видеоадаптер", info.gpu.clone()),
            ("Рендерер", info.renderer.clone()),
            ("Платформа", info.machine.clone()),
        ];
        for (i, (k, v)) in rows.iter().enumerate() {
            let y = g2.y + 4.0 + i as f32 * 48.0;
            if i > 0 {
                cx.ui.p.rect(Rect::new(g2.x + 16.0, y, g2.w - 32.0, 1.0), 0.0, Paint::Solid(t.stroke_subtle));
            }
            cx.ui.p.text_in(Rect::new(g2.x + 16.0, y, 200.0, 48.0), &text::BODY_MD, k, t.ink, 0.0);
            cx.ui.p.text_in(Rect::new(g2.x + 220.0, y, g2.w - 236.0, 48.0), &text::MONO_SM, v, t.ink_muted, 1.0);
        }
    }

    fn network(&mut self, cx: &mut AppCx, a: Rect) {
        let t = cx.ui.theme;
        cx.ui.p.text(Vec2::new(a.x, a.y), &text::TITLE_LG, "Сеть", t.ink);
        let g = Rect::new(a.x, a.y + 46.0, a.w, 3.0 * 64.0 + 8.0);
        group(&mut cx.ui, g);
        let mut ry = g.y + 4.0;
        row_label(&mut cx.ui, g, ry, "Wi‑Fi", Some(if cx.settings.wifi { "Подключено к orbita-net" } else { "Выключено" }));
        let mut on = cx.settings.wifi;
        cx.ui.toggle(cx.id("wifi"), Rect::new(g.right() - 60.0, ry + 19.0, 44.0, 26.0), &mut on);
        cx.settings.wifi = on;
        ry += 64.0;
        divider(&mut cx.ui, g, ry);
        row_label(&mut cx.ui, g, ry, "Проводное подключение", Some(&cx.sys.network()));
        status_word(&mut cx.ui, Rect::new(g.right() - 200.0, ry, 184.0, 64.0), Status::Success, "ok");
        ry += 64.0;
        divider(&mut cx.ui, g, ry);
        row_label(&mut cx.ui, g, ry, "Брандмауэр", Some("Входящие соединения фильтруются"));
        status_word(&mut cx.ui, Rect::new(g.right() - 200.0, ry, 184.0, 64.0), Status::Success, "Сеть защищена");
    }

    fn notifications(&mut self, cx: &mut AppCx, a: Rect) {
        let t = cx.ui.theme;
        cx.ui.p.text(Vec2::new(a.x, a.y), &text::TITLE_LG, "Уведомления", t.ink);
        let g = Rect::new(a.x, a.y + 46.0, a.w, 2.0 * 64.0 + 8.0);
        group(&mut cx.ui, g);
        let mut ry = g.y + 4.0;
        row_label(&mut cx.ui, g, ry, "Не беспокоить", Some("Тосты не появляются, центр сохраняет всё"));
        let mut on = cx.settings.dnd;
        cx.ui.toggle(cx.id("dnd"), Rect::new(g.right() - 60.0, ry + 19.0, 44.0, 26.0), &mut on);
        cx.settings.dnd = on;
        ry += 64.0;
        divider(&mut cx.ui, g, ry);
        row_label(&mut cx.ui, g, ry, "Проверить уведомления", Some("Все четыре статуса"));
        let kinds = [
            (Status::Info, "Обновление доступно", "Orbita OS 2.0 готова к установке"),
            (Status::Success, "Копирование завершено", "128 файлов в «Проекты»"),
            (Status::Warning, "Мало места на диске", "Осталось 6 ГБ"),
            (Status::Danger, "Ошибка сети", "error: DHCP не ответил"),
        ];
        let bw = cx.ui.button_width("Показать", ButtonSize::Md, false);
        if cx.ui.button(cx.id("notify"), Rect::new(g.right() - 16.0 - bw, ry + 14.0, bw, 36.0), "Показать", ButtonKind::Primary, ButtonSize::Md, None).clicked {
            for (s, title, text) in kinds.iter() {
                cx.notify(Notice::new(title, text, *s));
            }
        }
        // Кнопки всех видов — витрина компонента Button.
        let g2 = Rect::new(a.x, g.bottom() + 16.0, a.w, 76.0);
        group(&mut cx.ui, g2);
        let mut bx = g2.x + 16.0;
        for (i, (label, kind)) in [("Установить", ButtonKind::Primary), ("Отмена", ButtonKind::Neutral), ("Подробнее", ButtonKind::Ghost), ("Удалить", ButtonKind::Danger)]
            .iter()
            .enumerate()
        {
            let w = cx.ui.button_width(label, ButtonSize::Md, false);
            cx.ui.button(id2(cx.id("demo-btn"), i as u64), Rect::new(bx, g2.y + 20.0, w, 36.0), label, *kind, ButtonSize::Md, None);
            bx += w + 10.0;
        }
        let _ = Color::WHITE;
    }

    fn security(&mut self, cx: &mut AppCx, a: Rect) {
        let t = cx.ui.theme;
        cx.ui.p.text(Vec2::new(a.x, a.y), &text::TITLE_LG, "Безопасность", t.ink);
        attention(&mut cx.ui, Rect::new(a.x, a.y + 46.0, a.w, 104.0), "Система защищена", "Изоляция приложений в ring 3, проверка пакетов, брандмауэр");
        let g = Rect::new(a.x, a.y + 166.0, a.w, 3.0 * 64.0 + 8.0);
        group(&mut cx.ui, g);
        let rows = [
            ("Изоляция приложений", "Приложения работают в ring 3", Status::Success, "ok"),
            ("Обновления", "Доступна версия 2.0", Status::Warning, "Требует внимания"),
            ("Подпись пакетов", "Ещё не реализовано", Status::Danger, "error"),
        ];
        for (i, (title, sub, st, word)) in rows.iter().enumerate() {
            let ry = g.y + 4.0 + i as f32 * 64.0;
            if i > 0 {
                divider(&mut cx.ui, g, ry);
            }
            row_label(&mut cx.ui, g, ry, title, Some(sub));
            status_word(&mut cx.ui, Rect::new(g.right() - 220.0, ry, 204.0, 64.0), *st, word);
        }
    }

    fn account(&mut self, cx: &mut AppCx, a: Rect) {
        let t = cx.ui.theme;
        cx.ui.p.text(Vec2::new(a.x, a.y), &text::TITLE_LG, "Аккаунт", t.ink);
        let g = Rect::new(a.x, a.y + 46.0, a.w, 96.0);
        group(&mut cx.ui, g);
        let av = Rect::new(g.x + 20.0, g.y + 20.0, 56.0, 56.0);
        cx.ui.p.rect(av, 28.0, Paint::linear(145.0, t.amb_cyan, t.amb_indigo));
        cx.ui.p.text_in(av, &text::TITLE_MD, "ОР", Color::WHITE, 0.5);
        cx.ui.p.text(Vec2::new(av.right() + 16.0, g.y + 26.0), &text::TITLE_MD, "Пользователь Orbita", t.ink);
        cx.ui.p.text(Vec2::new(av.right() + 16.0, g.y + 52.0), &text::MONO_SM, "orbita@node · /home/user", t.ink_muted);
        let mut tx = a.x;
        for tag in ["Администратор", "Разработчик", "Тёмная тема"] {
            tx += cx.ui.tag(Vec2::new(tx, g.bottom() + 16.0), tag) + 8.0;
        }
        let mut c1 = true;
        let mut c2 = false;
        cx.ui.checkbox(cx.id("c1"), Vec2::new(a.x, g.bottom() + 56.0), "Входить автоматически", &mut c1);
        cx.ui.checkbox(cx.id("c2"), Vec2::new(a.x, g.bottom() + 86.0), "Синхронизировать настройки", &mut c2);
        cx.ui.radio(cx.id("r1"), Vec2::new(a.x + 300.0, g.bottom() + 56.0), "Локальный профиль", true);
        cx.ui.radio(cx.id("r2"), Vec2::new(a.x + 300.0, g.bottom() + 86.0), "Сетевой профиль", false);
    }
}

/// Карточка группы строк (`ob-pane`).
pub fn group(ui: &mut Ui, r: Rect) {
    let t = ui.theme;
    ui.p.rect(r, radius::LG, Paint::Solid(t.stroke_subtle));
}

fn divider(ui: &mut Ui, g: Rect, y: f32) {
    let t = ui.theme;
    ui.p.rect(Rect::new(g.x + 16.0, y, g.w - 32.0, 1.0), 0.0, Paint::Solid(t.stroke_subtle));
}

/// Левая часть строки настроек: название и пояснение.
pub fn row_label(ui: &mut Ui, g: Rect, y: f32, title: &str, sub: Option<&str>) {
    let t = ui.theme;
    match sub {
        Some(s) => {
            ui.p.text(Vec2::new(g.x + 16.0, y + 12.0), &text::LABEL_MD, title, t.ink);
            ui.p.text(Vec2::new(g.x + 16.0, y + 33.0), &text::BODY_SM, s, t.ink_muted);
        }
        None => {
            ui.p.text_in(Rect::new(g.x + 16.0, y, g.w * 0.5, 64.0), &text::LABEL_MD, title, t.ink, 0.0);
        }
    }
}

/// Статус словом с точкой цвета (цвет никогда не единственный признак).
pub fn status_word(ui: &mut Ui, r: Rect, s: Status, word: &str) {
    let t = ui.theme;
    let c = status_color(&t, s);
    let w = ui.p.measure(&text::LABEL_MD, word);
    let x = r.right() - w;
    ui.p.rect(Rect::new(x - 16.0, r.y + r.h * 0.5 - 4.0, 8.0, 8.0), 4.0, Paint::Solid(c));
    ui.p.text_in(Rect::new(x, r.y, w + 1.0, r.h), &text::LABEL_MD, word, c, 0.0);
}

/// Блок внимания `ob-attn`: стекло с синим оттенком, переплетённые
/// ambient-пятна под сильным блюром и шум `noise-strong`.
pub fn attention(ui: &mut Ui, r: Rect, title: &str, body: &str) {
    let t = ui.theme;
    let sh = ui.glass_shadow();
    for s in &sh {
        ui.p.shadow(r, radius::XL, *s);
    }
    ui.p.backdrop(r, radius::XL, crate::tokens::blur::LG * ui.look.blur, 1.8);
    ui.push_clip(r, radius::XL);
    let k = if t.mode == ThemeMode::Light { 0.7 } else { 1.0 };
    let big = r.inset(-r.w * 0.3);
    ui.p.rect(r, 0.0, Paint::Solid(t.glass_fill_accent));
    for (cx, cy, rx, ry, c, p) in [
        (0.15, 0.25, 0.40, 0.50, t.amb_blue, 0.9),
        (0.85, 0.20, 0.35, 0.45, t.amb_cyan, 0.7),
        (0.70, 0.90, 0.40, 0.50, t.amb_indigo, 0.85),
        (0.20, 0.95, 0.30, 0.40, t.amb_teal, 0.55),
        (0.55, 0.45, 0.22, 0.30, t.amb_rose, 0.3),
    ] {
        // Пятна задаются в координатах увеличенного на 30% слоя (`inset:-30%`).
        let bx = (r.x - big.x + cx * r.w) / big.w;
        let by = (r.y - big.y + cy * r.h) / big.h;
        ui.p.rect(big, 0.0, Paint::blob(bx, by, rx * r.w / big.w * 1.4, ry * r.h / big.h * 1.4, c.percent(p * k), 0.7));
    }
    ui.p.noise(r, 0.0, if ui.look.noise { noise::STRONG } else { 0.0 });
    ui.pop_clip();
    ui.p.stroke_rect(r, radius::XL, 1.0, t.glass_border);
    ui.p.inset_highlight(r, radius::XL, t.glass_highlight);
    let ink = if t.mode == ThemeMode::Dark { Color::rgb_hex(0xe1e5ee) } else { t.ink_muted };
    ui.p.text(Vec2::new(r.x + 24.0, r.y + 24.0), &text::TITLE_MD, title, t.ink);
    ui.p.text_in(Rect::new(r.x + 24.0, r.y + 52.0, r.w - 48.0, 20.0), &text::BODY_MD, body, ink, 0.0);
}
