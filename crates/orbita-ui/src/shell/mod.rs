//! Оболочка рабочего стола Orbita OS.
//!
//! [`Desktop`] — корневой объект: хранит окна, настройки, уведомления,
//! состояние лаунчера и центра управления, и каждый кадр собирает экран:
//!
//! ```text
//! обои (кэш) → иконки рабочего стола → окна (по z) → верхняя панель
//!   → док → лаунчер → центр управления → тосты → меню → подсказка → курсор
//! ```
//!
//! ```
//! use orbita_ui::{Desktop, FrameInput, apps::AppKind};
//! let mut d = Desktop::new(orbita_ui::system::DemoSystem::boxed());
//! d.open(AppKind::Terminal, None);
//! let f = d.frame(&FrameInput { width: 1280, height: 800, scale: 1.0, time: 1.0, ..Default::default() });
//! assert!(f.batches.len() >= 2); // обои + стекло окна
//! ```

extern crate alloc;

use alloc::boxed::Box;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::anim::Ease;
use crate::apps::{App, AppCx, AppKind, Request};
use crate::color::Color;
use crate::geom::{Rect, Vec2};
use crate::gfx::{Frame, Paint, Shadow};
use crate::icons::Icon;
use crate::input::{FrameInput, Key};
use crate::math;
use crate::system::SystemPort;
use crate::tokens::{self, AccentChoice, ThemeMode, motion, radius, text};
use crate::ui::{CursorKind, Look, Ui, id, id2};

pub mod chrome;
pub mod logo;
pub mod wallpaper;

pub use wallpaper::Wallpaper;

/// Высота верхней панели.
pub const TOPBAR_H: f32 = 40.0;
/// Высота строки заголовка окна (v2: 46px).
pub const WINDOW_BAR_H: f32 = 46.0;

/// Настройки внешнего вида и поведения (сохраняются в сессии).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    /// Тема.
    pub mode: ThemeMode,
    /// Акцентный цвет.
    pub accent: AccentChoice,
    /// Обои.
    pub wallpaper: Wallpaper,
    /// Блюр/шум/анимации/курсор.
    pub look: Look,
    /// Показывать FPS в верхней панели.
    pub show_fps: bool,
    /// Вертикальная синхронизация (просьба к платформе).
    pub vsync: bool,
    /// «Не беспокоить» — тосты не показываются.
    pub dnd: bool,
    /// Громкость (демо-контрол центра управления).
    pub volume: f32,
    /// Wi‑Fi включён (демо).
    pub wifi: bool,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings {
            mode: ThemeMode::Dark,
            accent: AccentChoice::Blue,
            wallpaper: Wallpaper::WallA,
            look: Look::default(),
            show_fps: true,
            vsync: false,
            dnd: false,
            volume: 0.62,
            wifi: true,
        }
    }
}

/// Статус уведомления (цвет значка; всегда дублируется словом).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    /// Информация (акцент).
    Info,
    /// Успех.
    Success,
    /// Внимание.
    Warning,
    /// Ошибка.
    Danger,
}

/// Уведомление: тост и запись в центре управления.
#[derive(Clone, Debug, PartialEq)]
pub struct Notice {
    /// Заголовок.
    pub title: String,
    /// Текст.
    pub text: String,
    /// Статус.
    pub status: Status,
    /// Иконка значка.
    pub icon: Icon,
    /// Время появления (заполняет оболочка).
    pub time: f64,
    /// Уникальный номер (заполняет оболочка).
    pub id: u64,
}

impl Notice {
    /// Конструктор.
    ///
    /// ```
    /// use orbita_ui::shell::{Notice, Status};
    /// let n = Notice::new("Копирование завершено", "128 файлов в «Проекты»", Status::Success);
    /// assert_eq!(n.status, Status::Success);
    /// ```
    pub fn new(title: &str, text: &str, status: Status) -> Notice {
        let icon = match status {
            Status::Info => Icon::IDownload,
            Status::Success => Icon::ICheck,
            Status::Warning => Icon::IBell,
            Status::Danger => Icon::IClose,
        };
        Notice { title: String::from(title), text: String::from(text), status, icon, time: 0.0, id: 0 }
    }
}

/// Статистика кадров (для монитора и счётчика FPS).
#[derive(Clone, Debug)]
pub struct Stats {
    /// Текущий FPS (сообщает платформа).
    pub fps: f32,
    /// CPU-время кадра, мс.
    pub cpu_ms: f32,
    /// GPU-время кадра, мс.
    pub gpu_ms: f32,
    /// История FPS (кольцевой буфер).
    pub history: [f32; 240],
    /// Позиция записи в истории.
    pub head: usize,
    /// Инстансов в прошлом кадре.
    pub instances: usize,
    /// Пакетов (снимков стекла) в прошлом кадре.
    pub batches: usize,
    /// Время работы, сек.
    pub uptime: f64,
}

impl Default for Stats {
    fn default() -> Stats {
        Stats { fps: 0.0, cpu_ms: 0.0, gpu_ms: 0.0, history: [0.0; 240], head: 0, instances: 0, batches: 0, uptime: 0.0 }
    }
}

/// Открытое окно.
pub struct Window {
    /// Уникальный номер.
    pub id: u64,
    /// Приложение.
    pub app: App,
    /// Прямоугольник в обычном состоянии.
    pub rect: Rect,
    /// Развёрнуто на весь экран.
    pub maximized: bool,
    /// Свёрнуто в док.
    pub minimized: bool,
    /// Окно закрывается (идёт анимация, затем удаление).
    pub closing: bool,
    drag_prev: Vec2,
    resize_prev: Vec2,
}

/// Состояние лаунчера.
pub struct LauncherState {
    /// Открыт ли.
    pub open: bool,
    /// Поле поиска.
    pub search: crate::ui::TextEdit,
}

/// Иконка на рабочем столе.
pub struct DesktopIcon {
    /// Подпись.
    pub name: String,
    /// Путь, который открывается двойным кликом.
    pub path: String,
    /// Файловая иконка (`fi-*`).
    pub icon: Icon,
    /// Каталог ли.
    pub is_dir: bool,
    /// Выделена ли.
    pub selected: bool,
}

/// Рабочий стол — корень оболочки.
pub struct Desktop {
    /// UI-контекст.
    pub ui: Ui,
    /// Порт системы.
    pub sys: Box<dyn SystemPort>,
    /// Настройки.
    pub settings: Settings,
    /// Окна; последний — верхний.
    pub windows: Vec<Window>,
    next_id: u64,
    /// Окно в фокусе.
    pub focused: Option<u64>,
    /// Лаунчер.
    pub launcher: LauncherState,
    /// Центр управления открыт.
    pub center_open: bool,
    /// Все уведомления (центр управления).
    pub notices: Vec<Notice>,
    /// Активные тосты (номера уведомлений).
    pub toasts: Vec<u64>,
    next_notice: u64,
    /// Иконки рабочего стола.
    pub icons: Vec<DesktopIcon>,
    marquee: Option<Vec2>,
    /// Статистика.
    pub stats: Stats,
    start_time: Option<f64>,
    requests: Vec<Request>,
    /// Открытое меню верхней панели (индекс «Файл/Правка/Вид»).
    pub topbar_menu: Option<usize>,
    welcomed: bool,
    raise: Option<u64>,
    /// Время старта анимации загрузки (`None` — загрузчик завершён).
    pub intro_start: Option<f64>,
}

impl Desktop {
    /// Создаёт рабочий стол поверх порта системы.
    pub fn new(sys: Box<dyn SystemPort>) -> Desktop {
        let icons = alloc::vec![
            DesktopIcon { name: String::from("Проекты"), path: String::from("/home/user/Проекты"), icon: Icon::FiFolder, is_dir: true, selected: false },
            DesktopIcon { name: String::from("Документы"), path: String::from("/home/user/Документы"), icon: Icon::FiFolder, is_dir: true, selected: false },
            DesktopIcon { name: String::from("readme.md"), path: String::from("/home/user/readme.md"), icon: Icon::FiDoc, is_dir: false, selected: false },
            DesktopIcon { name: String::from("hello.sh"), path: String::from("/home/user/hello.sh"), icon: Icon::FiCode, is_dir: false, selected: false },
            DesktopIcon { name: String::from("backup.zip"), path: String::from("/home/user/Загрузки"), icon: Icon::FiZip, is_dir: true, selected: false },
        ];
        Desktop {
            ui: Ui::new(),
            sys,
            settings: Settings::default(),
            windows: Vec::new(),
            next_id: 1,
            focused: None,
            launcher: LauncherState { open: false, search: crate::ui::TextEdit::default() },
            center_open: false,
            notices: Vec::new(),
            toasts: Vec::new(),
            next_notice: 1,
            icons,
            marquee: None,
            stats: Stats::default(),
            start_time: None,
            requests: Vec::new(),
            topbar_menu: None,
            welcomed: false,
            raise: None,
            intro_start: None,
        }
    }

    /// Открывает приложение в новом окне (с анимацией `ob-win`).
    ///
    /// Принимает: вид приложения и аргумент (путь/раздел).
    /// Возвращает: номер окна.
    pub fn open(&mut self, kind: AppKind, arg: Option<&str>) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        let app = App::new(kind, arg, self.sys.as_mut());
        let (w, h) = kind.default_size();
        let screen = if self.ui.screen().w > 0.0 { self.ui.screen() } else { Rect::new(0.0, 0.0, 1280.0, 800.0) };
        let n = self.windows.len() as f32;
        let w = math::min(w, screen.w - 40.0);
        let h = math::min(h, screen.h - TOPBAR_H - 110.0);
        let x = math::max(20.0, (screen.w - w) * 0.5 - 60.0 + n * 32.0);
        let y = math::max(TOPBAR_H + 16.0, (screen.h - h) * 0.5 - 40.0 + n * 28.0);
        self.windows.push(Window {
            id,
            app,
            rect: Rect::new(x, y, w, h),
            maximized: false,
            minimized: false,
            closing: false,
            drag_prev: Vec2::ZERO,
            resize_prev: Vec2::ZERO,
        });
        let now = self.ui.now;
        self.ui.anims.start(win_key(id, 1), 0.0, 1.0, motion::WINDOW, Ease::Out, now);
        self.focus(id);
        id
    }

    /// Открывает окно с заданным прямоугольником (восстановление сессии).
    pub fn open_at(&mut self, kind: AppKind, arg: Option<&str>, rect: Rect, maximized: bool) -> u64 {
        let id = self.open(kind, arg);
        if let Some(w) = self.windows.iter_mut().find(|w| w.id == id) {
            w.rect = rect;
            w.maximized = maximized;
        }
        let now = self.ui.now;
        self.ui.anims.start(win_key(id, 2), if maximized { 1.0 } else { 0.0 }, if maximized { 1.0 } else { 0.0 }, 0.0, Ease::Out, now);
        id
    }

    /// Переводит фокус на окно и поднимает его наверх.
    pub fn focus(&mut self, id: u64) {
        if let Some(i) = self.windows.iter().position(|w| w.id == id) {
            let w = self.windows.remove(i);
            self.windows.push(w);
            self.focused = Some(id);
        }
    }

    /// Закрывает окно (с анимацией).
    pub fn close(&mut self, id: u64) {
        let now = self.ui.now;
        if let Some(w) = self.windows.iter_mut().find(|w| w.id == id) {
            if !w.closing {
                w.closing = true;
                let cur = self.ui.anims.get(win_key(id, 1), now).unwrap_or(1.0);
                self.ui.anims.start(win_key(id, 1), cur, 0.0, motion::BASE, Ease::Soft, now);
            }
        }
        if self.focused == Some(id) {
            self.focused = self.windows.iter().rev().find(|w| !w.closing && !w.minimized && w.id != id).map(|w| w.id);
        }
    }

    /// Сворачивает/разворачивает окно из дока.
    pub fn set_minimized(&mut self, id: u64, min: bool) {
        let now = self.ui.now;
        if let Some(w) = self.windows.iter_mut().find(|w| w.id == id) {
            w.minimized = min;
            let _ = self.ui.anims.to(win_key(id, 3), if min { 1.0 } else { 0.0 }, motion::WINDOW, Ease::Out, now);
        }
        if min {
            if self.focused == Some(id) {
                self.focused = self.windows.iter().rev().find(|w| !w.minimized && !w.closing && w.id != id).map(|w| w.id);
            }
        } else {
            self.focus(id);
        }
    }

    /// Разворачивает/восстанавливает окно.
    pub fn toggle_maximize(&mut self, id: u64) {
        if let Some(w) = self.windows.iter_mut().find(|w| w.id == id) {
            w.maximized = !w.maximized;
        }
    }

    /// Добавляет уведомление (и тост, если не «Не беспокоить»).
    pub fn notify(&mut self, mut n: Notice) {
        n.time = self.ui.now;
        n.id = self.next_notice;
        self.next_notice += 1;
        if !self.settings.dnd {
            self.toasts.push(n.id);
            let now = self.ui.now;
            self.ui.anims.start(id2(id("toast"), n.id), 0.0, 1.0, motion::TOAST, Ease::Out, now);
        }
        self.notices.insert(0, n);
        if self.notices.len() > 30 {
            self.notices.truncate(30);
        }
    }

    /// Применяет тему/акцент из настроек к UI.
    fn apply_settings(&mut self) {
        let base = tokens::Theme::for_mode(self.settings.mode);
        self.ui.theme = base.with_accent(self.settings.accent);
        self.ui.look = self.settings.look;
    }

    /// Окно, которому принадлежит клавиатура.
    fn focused_index(&self) -> Option<usize> {
        let f = self.focused?;
        self.windows.iter().position(|w| w.id == f && !w.minimized && !w.closing)
    }

    /// Нужно ли перерисовывать (идут анимации) — платформа может спать,
    /// если `false` и ввода нет.
    pub fn animating(&mut self) -> bool {
        self.ui.anims.take_active()
    }

    /// Строит кадр.
    ///
    /// Принимает: ввод за кадр. Возвращает: готовый [`Frame`] для рендерера.
    pub fn frame(&mut self, input: &FrameInput) -> &Frame {
        self.apply_settings();
        if self.start_time.is_none() {
            self.start_time = Some(input.time);
        }
        self.stats.uptime = input.time - self.start_time.unwrap_or(0.0);
        if input.fps > 0.0 {
            self.stats.fps = input.fps;
            self.stats.history[self.stats.head] = input.fps;
            self.stats.head = (self.stats.head + 1) % self.stats.history.len();
        }
        self.stats.cpu_ms = input.cpu_ms;
        self.stats.gpu_ms = input.gpu_ms;
        if let Some(r) = self.raise.take() {
            self.focus(r);
        }
        self.ui.begin(input);
        let screen = self.ui.screen();

        if !self.welcomed {
            self.welcomed = true;
            if self.settings.look.animations {
                self.intro_start = Some(self.ui.now);
            }
            let info = self.sys.info();
            self.notify(Notice::new(
                "Добро пожаловать",
                &format!("{} · {}", info.os, if info.gpu.is_empty() { "графика готова" } else { info.gpu.as_str() }),
                Status::Info,
            ));
        }

        self.global_shortcuts();

        // 1. Обои — статичный кэшируемый пакет.
        let theme = self.ui.theme;
        let key = wallpaper::cache_key(&theme, self.settings.wallpaper, input.width, input.height, self.settings.look.noise);
        self.ui.p.begin_cached(key);
        wallpaper::draw(&mut self.ui.p, &theme, self.settings.wallpaper, screen, self.settings.look.noise);
        self.ui.p.end_cached();

        // 2. Рабочий стол (иконки, рамка выделения, контекстное меню).
        self.ui.layer(1, screen);
        chrome::desktop_icons(self);

        // 3. Окна.
        self.draw_windows();

        // 4. Верхняя панель и док.
        chrome::topbar(self);
        chrome::dock(self);

        // 5. Оверлеи.
        chrome::launcher(self);
        chrome::control_center(self);
        chrome::toasts(self);
        self.ui.draw_popup();
        self.ui.draw_tooltip();
        chrome::intro(self);

        // 6. Запросы приложений.
        let reqs = core::mem::take(&mut self.requests);
        for r in reqs {
            match r {
                Request::Open(kind, arg) => {
                    self.open(kind, arg.as_deref());
                }
                Request::CloseSelf => {
                    if let Some(f) = self.focused {
                        self.close(f);
                    }
                }
                Request::Notify(n) => self.notify(n),
            }
        }

        // 7. Удаляем закрытые окна после анимации.
        let now = self.ui.now;
        let anims = &self.ui.anims;
        let mut gone = Vec::new();
        self.windows.retain(|w| {
            let keep = !(w.closing && anims.finished(win_key(w.id, 1), now));
            if !keep {
                gone.push(w.id);
            }
            keep
        });
        for g in gone {
            for k in 1..6 {
                self.ui.anims.remove(win_key(g, k));
            }
        }

        // 8. Курсор — последним, поверх всего.
        chrome::cursor(self);
        self.ui.end();
        let f = self.ui.p.end_frame();
        self.stats.instances = f.instances.len();
        self.stats.batches = f.batches.iter().filter(|b| b.capture != 0).count();
        self.ui.p.frame()
    }

    fn global_shortcuts(&mut self) {
        let input = self.ui.input.clone();
        for k in &input.keys {
            if !k.down {
                continue;
            }
            match k.key {
                Key::Super => self.toggle_launcher(),
                Key::Space if k.mods.ctrl => self.toggle_launcher(),
                Key::Escape => {
                    if self.launcher.open {
                        self.launcher.open = false;
                    } else if self.center_open {
                        self.center_open = false;
                    }
                }
                Key::Char('t') if k.mods.ctrl && k.mods.alt => {
                    self.open(AppKind::Terminal, None);
                }
                Key::Char('w') if k.mods.ctrl => {
                    if let Some(f) = self.focused {
                        self.close(f);
                    }
                }
                Key::Char('n') if k.mods.ctrl && k.mods.shift => {
                    self.open(AppKind::Files, None);
                }
                Key::F(11) => {
                    if let Some(f) = self.focused {
                        self.toggle_maximize(f);
                    }
                }
                Key::Tab if k.mods.alt || k.mods.ctrl => {
                    // Перебор окон: нижнее видимое окно поднимается наверх.
                    if let Some(w) = self.windows.iter().find(|w| !w.closing).map(|w| w.id) {
                        self.set_minimized(w, false);
                    }
                }
                _ => {}
            }
        }
    }

    /// Открывает/закрывает лаунчер.
    pub fn toggle_launcher(&mut self) {
        self.launcher.open = !self.launcher.open;
        self.center_open = false;
        if self.launcher.open {
            self.launcher.search = crate::ui::TextEdit::default();
            self.ui.focus = Some(id("launcher-search"));
        }
    }

    /// Прямоугольник рабочей области (под верхней панелью).
    pub fn work_area(&self) -> Rect {
        let s = self.ui.screen();
        Rect::new(0.0, TOPBAR_H, s.w, s.h - TOPBAR_H)
    }

    fn draw_windows(&mut self) {
        let n = self.windows.len();
        let work = self.work_area();
        let mut i = 0;
        while i < n.min(self.windows.len()) {
            let wid = self.windows[i].id;
            let focused = self.focused == Some(wid) && !self.launcher.open;
            let layer = 100 + i as u32;
            self.draw_window(i, layer, focused, work);
            i += 1;
        }
    }

    fn draw_window(&mut self, idx: usize, layer: u32, focused: bool, work: Rect) {
        let wid = self.windows[idx].id;
        let now = self.ui.now;
        let open_t = self.ui.anims.get(win_key(wid, 1), now).unwrap_or(1.0);
        if !self.ui.anims.finished(win_key(wid, 1), now) {
            self.ui.anims.mark_active();
        }
        let minimized = self.windows[idx].minimized;
        let min_t = self.ui.anim(win_key(wid, 3), if minimized { 1.0 } else { 0.0 }, motion::WINDOW, Ease::Out);
        if min_t > 0.999 && minimized {
            return;
        }
        let maximized = self.windows[idx].maximized;
        let max_t = self.ui.anim(win_key(wid, 2), if maximized { 1.0 } else { 0.0 }, motion::BASE + 0.08, Ease::Out);
        let normal = self.windows[idx].rect;
        let r = normal.lerp(&work, max_t);
        let rad = radius::XL * (1.0 - max_t);
        let t = self.ui.theme;

        // Слой ввода: окно целиком + полоса для изменения размера.
        self.ui.layer(layer, r.inset(-6.0));

        // Анимации появления/закрытия/сворачивания: прозрачность и масштаб.
        let dock_target = Vec2::new(self.ui.screen().w * 0.5, self.ui.screen().h - 30.0);
        let alpha = open_t * (1.0 - min_t);
        let k = (0.965 + 0.035 * open_t) * (1.0 - 0.75 * min_t);
        let offset = Vec2::new(0.0, 14.0 * (1.0 - open_t)).add(dock_target.sub(r.center()).scale(min_t * 0.9));
        self.ui.p.push_opacity(alpha);
        self.ui.p.push_transform(r.center(), k, offset);

        // Корпус окна: мягкая тень, стекло, контур, кромка.
        let shadow = match t.mode {
            ThemeMode::Dark => Shadow::new(0.0, 24.0, 70.0, -14.0, Color::hex(0x00000073)),
            ThemeMode::Light => Shadow::new(0.0, 24.0, 70.0, -14.0, Color::hex(0x1e3a8a40)),
        };
        let fa = self.ui.anim(win_key(wid, 4), if focused { 1.0 } else { 0.0 }, motion::BASE, Ease::Soft);
        let mut style = self.ui.menu_style();
        style.border = t.stroke_subtle;
        style.fill = t.glass_fill.mix(t.glass_fill_strong, fa);
        style.saturate = 1.0 + 0.8 * fa;
        if let Some(bg) = self.windows[idx].app.tinted_bg(&self.ui) {
            style.fill = bg;
        }
        let shadow = Shadow { color: shadow.color.fade(0.55 + 0.45 * fa), ..shadow };
        if max_t < 0.999 {
            self.ui.p.glass(r, rad, &style, &[shadow]);
        } else {
            style.border = Color::TRANSPARENT;
            if self.windows[idx].app.tinted_bg(&self.ui).is_none() {
                style.fill = t.glass_fill_strong.mix(t.surface_base, 0.25);
            }
            self.ui.p.glass(r, 0.0, &style, &[]);
        }

        // Строка заголовка.
        let (bar, body) = r.split_top(WINDOW_BAR_H);
        let title = self.windows[idx].app.title();
        let kind = self.windows[idx].app.kind();
        let mut tx = bar.x + 16.0;
        self.ui.icon(kind.glyph(), Rect::new(tx, bar.y + (bar.h - 16.0) * 0.5, 16.0, 16.0), t.ink_muted);
        tx += 26.0;
        let title_color = t.ink_muted.mix(t.ink, fa);
        let tw = self.ui.p.measure(&text::TITLE_SM, &title);
        self.ui.p.text_in(Rect::new(tx, bar.y, math::min(tw + 4.0, bar.w * 0.4), bar.h), &text::TITLE_SM, &title, title_color, 0.0);

        // Кнопки окна.
        let ctrl_w = 36.0 * 3.0 + 4.0;
        let cx = bar.right() - 8.0 - ctrl_w;
        let cy = bar.y + (bar.h - 30.0) * 0.5;
        let base = win_key(wid, 10);
        // В развёрнутом виде кнопки окна живут в верхней панели (макет
        // FullscreenX) — в самом окне их не дублируем.
        if max_t < 0.5 {
            if self.ui.icon_button(id2(base, 1), Rect::new(cx, cy, 36.0, 30.0), Icon::IMinus, false).clicked {
                self.set_minimized(wid, true);
            }
            let max_icon = if maximized { Icon::IRestore } else { Icon::IMax };
            if self.ui.icon_button(id2(base, 2), Rect::new(cx + 38.0, cy, 36.0, 30.0), max_icon, false).clicked {
                self.toggle_maximize(wid);
            }
            if self.ui.icon_button(id2(base, 3), Rect::new(cx + 76.0, cy, 36.0, 30.0), Icon::IClose, true).clicked {
                self.close(wid);
            }
        }

        // Содержимое строки заголовка от приложения (адресная строка).
        let extra_w = self.windows[idx].app.title_bar_width();
        let extra = if extra_w > 0.0 {
            let x0 = tx + tw + 20.0;
            let w = math::min(extra_w, cx - 12.0 - x0);
            Some(Rect::new(x0, bar.y + 6.0, math::max(0.0, w), bar.h - 12.0))
        } else {
            None
        };

        // Перетаскивание за заголовок (двойной клик — развернуть).
        let drag_id = id2(base, 4);
        let drag_area = Rect::new(bar.x, bar.y, cx - bar.x - 4.0, bar.h);
        let mut prev = self.windows[idx].drag_prev;
        let resp = self.ui.drag_area(drag_id, drag_area, &mut prev);
        self.windows[idx].drag_prev = prev;
        if resp.pressed {
            self.focus_pending(wid);
            if resp.double_clicked {
                self.toggle_maximize(wid);
            }
        }
        if (resp.drag.x != 0.0 || resp.drag.y != 0.0) && resp.down {
            let w = &mut self.windows[idx];
            if w.maximized {
                // Перетаскивание развёрнутого окна «отрывает» его.
                w.maximized = false;
                let m = self.ui.mouse;
                w.rect.x = m.x - w.rect.w * 0.5;
                w.rect.y = m.y - 20.0;
                self.ui.anims.start(win_key(wid, 2), 0.0, 0.0, 0.0, Ease::Out, now);
            } else {
                w.rect.x += resp.drag.x;
                w.rect.y = math::max(TOPBAR_H, w.rect.y + resp.drag.y);
            }
            self.ui.cursor = CursorKind::Move;
        }
        // Изменение размера за правый нижний угол и края.
        if !maximized {
            let grip = Rect::new(r.right() - 14.0, r.bottom() - 14.0, 20.0, 20.0);
            let edge_r = Rect::new(r.right() - 4.0, r.y + 20.0, 10.0, r.h - 40.0);
            let edge_b = Rect::new(r.x + 20.0, r.bottom() - 4.0, r.w - 40.0, 10.0);
            for (n, area) in [(5u64, grip), (6, edge_r), (7, edge_b)] {
                let mut p = self.windows[idx].resize_prev;
                let rr = self.ui.drag_area(id2(base, n), area, &mut p);
                self.windows[idx].resize_prev = p;
                if rr.hovered || rr.down {
                    self.ui.cursor = if n == 7 { CursorKind::Move } else { CursorKind::ResizeH };
                }
                if rr.pressed {
                    self.focus_pending(wid);
                }
                if rr.down {
                    let w = &mut self.windows[idx];
                    if n != 7 {
                        w.rect.w = math::max(360.0, w.rect.w + rr.drag.x);
                    }
                    if n != 6 {
                        w.rect.h = math::max(240.0, w.rect.h + rr.drag.y);
                    }
                }
            }
        }

        // Клик в любом месте окна — фокус.
        if self.ui.layer_hovered() && (self.ui.left_pressed() || self.ui.right_pressed()) && r.contains(self.ui.mouse) {
            self.focus_pending(wid);
        }

        // Тело окна — приложение.
        self.ui.push_clip(r, rad);
        let mut requests = core::mem::take(&mut self.requests);
        {
            let Desktop { ui, sys, settings, stats, windows, .. } = self;
            let mut cx = AppCx {
                ui,
                sys: sys.as_mut(),
                settings,
                stats,
                requests: &mut requests,
                focused: focused && self_focus_ok(windows, idx),
                wid: win_key(wid, 0),
            };
            if let Some(e) = extra {
                windows[idx].app.title_bar(&mut cx, e);
            }
            windows[idx].app.ui(&mut cx, body);
        }
        self.requests = requests;
        self.ui.pop_clip();
        self.ui.p.pop_transform();
        self.ui.p.pop_opacity();
    }

    fn focus_pending(&mut self, id: u64) {
        // Фокус меняем сразу, а порядок окон — в начале следующего кадра
        // (чтобы не переставлять массив во время его обхода).
        self.focused = Some(id);
        self.raise = Some(id);
    }

    /// Сохраняет сессию (настройки + окна) в текст — для hot reload.
    pub fn save_session(&self) -> String {
        crate::session::save(self)
    }

    /// Восстанавливает сессию из текста [`Desktop::save_session`].
    pub fn restore_session(&mut self, s: &str) {
        crate::session::restore(self, s)
    }

    /// Просьба к платформе: включить вертикальную синхронизацию.
    pub fn wants_vsync(&self) -> bool {
        self.settings.vsync
    }

    /// Обработка строки «оболочечных» команд терминала (`ui …`).
    ///
    /// Возвращает строки ответа или `None`, если команда не про оболочку.
    pub fn shell_command(settings: &mut Settings, line: &str) -> Option<Vec<String>> {
        let mut parts = line.split_whitespace();
        let cmd = parts.next()?;
        let arg = parts.next().unwrap_or("");
        let out = match cmd {
            "theme" => {
                settings.mode = match arg {
                    "light" | "светлая" => ThemeMode::Light,
                    "dark" | "тёмная" | "темная" => ThemeMode::Dark,
                    _ => {
                        if settings.mode == ThemeMode::Dark { ThemeMode::Light } else { ThemeMode::Dark }
                    }
                };
                format!("тема: {}", if settings.mode == ThemeMode::Dark { "тёмная" } else { "светлая" })
            }
            "wallpaper" => {
                let i = arg.parse::<usize>().ok().unwrap_or((settings.wallpaper.index() + 1) % Wallpaper::ALL.len());
                settings.wallpaper = Wallpaper::from_index(i);
                format!("обои: {}", settings.wallpaper.title())
            }
            "accent" => {
                let i = arg.parse::<usize>().ok().unwrap_or((settings.accent.index() + 1) % AccentChoice::ALL.len());
                settings.accent = AccentChoice::from_index(i);
                format!("акцент: {i}")
            }
            "fps" => {
                settings.show_fps = !settings.show_fps;
                format!("счётчик FPS: {}", if settings.show_fps { "показан" } else { "скрыт" })
            }
            "vsync" => {
                settings.vsync = arg != "off";
                format!("vsync: {}", if settings.vsync { "включён" } else { "выключен (без ограничения FPS)" })
            }
            "blur" => {
                let v = arg.parse::<f32>().ok().unwrap_or(1.0);
                settings.look.blur = math::clamp(v, 0.0, 1.5);
                format!("блюр: {}%", (settings.look.blur * 100.0) as u32)
            }
            "anim" => {
                settings.look.animations = arg != "off";
                format!("анимации: {}", if settings.look.animations { "включены" } else { "выключены" })
            }
            _ => return None,
        };
        Some(alloc::vec![out])
    }
}

fn self_focus_ok(windows: &[Window], idx: usize) -> bool {
    !windows[idx].minimized && !windows[idx].closing
}

/// Ключ анимации окна: `n` — номер свойства (1 — появление, 2 — разворот,
/// 3 — сворачивание, 4 — фокус, 10+ — элементы).
pub fn win_key(wid: u64, n: u64) -> u64 {
    id2(id2(id("window"), wid), n)
}

/// Цвет статуса.
pub fn status_color(t: &tokens::Theme, s: Status) -> Color {
    match s {
        Status::Info => t.accent_text,
        Status::Success => t.success,
        Status::Warning => t.warning,
        Status::Danger => t.danger,
    }
}

/// Значок статуса `ob-toast__badge`: квадрат 40/32px с тонированной заливкой.
pub fn status_badge(ui: &mut Ui, r: Rect, s: Status, icon: Icon) {
    let c = status_color(&ui.theme, s);
    ui.p.rect(r, if r.w > 34.0 { 12.0 } else { 10.0 }, Paint::Solid(c.with_alpha(0.18)));
    let sz = if r.w > 34.0 { 18.0 } else { 16.0 };
    ui.icon(icon, r.center_box(sz, sz), c);
}
