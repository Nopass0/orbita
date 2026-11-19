//! Базовые приложения Orbita OS и общий контекст для них.
//!
//! | Приложение | Модуль | Что умеет |
//! |---|---|---|
//! | Терминал | [`terminal`] | настоящий shell ОС, история, цвета, прокрутка |
//! | Проводник | [`explorer`] | навигация, плитки/список, сведения, удаление, новая папка |
//! | Настройки | [`settings`] | тема, акцент, обои, блюр, шум, анимации, курсор, FPS |
//! | Монитор | [`monitor`] | график FPS, время кадра, GPU, память |
//! | Редактор | [`editor`] | многострочное редактирование, сохранение `Ctrl+S` |
//! | Orbita | [`about`] | сведения о системе |

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

use crate::color::Color;
use crate::geom::{Rect, Vec2};
use crate::gfx::{Paint, Shadow};
use crate::icons::Icon;
use crate::shell::{Notice, Settings, Stats};
use crate::system::SystemPort;
use crate::ui::Ui;

pub mod about;
pub mod editor;
pub mod explorer;
pub mod monitor;
pub mod settings;
pub mod terminal;

/// Вид приложения.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AppKind {
    /// «Orbita» — сведения о системе (иконка лаунчера).
    About,
    /// Проводник.
    Files,
    /// Терминал.
    Terminal,
    /// Редактор текста.
    Editor,
    /// Монитор производительности.
    Monitor,
    /// Настройки.
    Settings,
}

impl AppKind {
    /// Все приложения (порядок дока).
    pub const ALL: [AppKind; 6] =
        [AppKind::About, AppKind::Files, AppKind::Terminal, AppKind::Editor, AppKind::Monitor, AppKind::Settings];

    /// Название (существительное, как требует дизайн-система).
    ///
    /// ```
    /// assert_eq!(orbita_ui::apps::AppKind::Files.title(), "Проводник");
    /// ```
    pub fn title(self) -> &'static str {
        match self {
            AppKind::About => "Orbita",
            AppKind::Files => "Проводник",
            AppKind::Terminal => "Терминал",
            AppKind::Editor => "Редактор",
            AppKind::Monitor => "Монитор",
            AppKind::Settings => "Настройки",
        }
    }

    /// Белый глиф на иконке.
    pub fn glyph(self) -> Icon {
        match self {
            AppKind::About => Icon::IOrbit,
            AppKind::Files => Icon::IFolder,
            AppKind::Terminal => Icon::ITerminal,
            AppKind::Editor => Icon::ICode,
            AppKind::Monitor => Icon::IDisplay,
            AppKind::Settings => Icon::ITune,
        }
    }

    /// Строковый ключ (сессия, команды терминала).
    pub fn key(self) -> &'static str {
        match self {
            AppKind::About => "about",
            AppKind::Files => "files",
            AppKind::Terminal => "terminal",
            AppKind::Editor => "editor",
            AppKind::Monitor => "monitor",
            AppKind::Settings => "settings",
        }
    }

    /// Обратное к [`AppKind::key`] (+ русские синонимы).
    ///
    /// ```
    /// use orbita_ui::apps::AppKind;
    /// assert_eq!(AppKind::from_key("files"), Some(AppKind::Files));
    /// assert_eq!(AppKind::from_key("терминал"), Some(AppKind::Terminal));
    /// ```
    pub fn from_key(k: &str) -> Option<AppKind> {
        let k = k.trim();
        AppKind::ALL.iter().copied().find(|a| a.key() == k || a.title().to_lowercase() == k.to_lowercase())
    }

    /// Начальный размер окна.
    pub fn default_size(self) -> (f32, f32) {
        match self {
            AppKind::About => (520.0, 420.0),
            AppKind::Files => (860.0, 540.0),
            AppKind::Terminal => (760.0, 460.0),
            AppKind::Editor => (760.0, 520.0),
            AppKind::Monitor => (720.0, 520.0),
            AppKind::Settings => (900.0, 600.0),
        }
    }
}

/// Рисует иконку приложения `ob-app`: скруглённый квадрат (30% радиуса)
/// с градиентом ambient-цветов, шумом и белым глифом.
///
/// Принимает: контекст, вид приложения, квадрат и признак «в доке»
/// (в доке радиус 14px, шум слабее, глиф 21px).
pub fn draw_app_icon(ui: &mut Ui, kind: AppKind, r: Rect, dock: bool) {
    let t = ui.theme;
    let rad = if dock { 14.0 * r.w / 44.0 } else { r.w * 0.3 };
    let paint = match kind {
        AppKind::About => Paint::Radial {
            cx: 0.3,
            cy: 0.2,
            rx: 0.8,
            ry: 0.8,
            c0: t.amb_cyan,
            c1: t.amb_blue,
            c2: t.accent_deep,
            mid: 0.55,
            end: 1.0,
        },
        AppKind::Files => Paint::linear(145.0, t.amb_cyan, t.amb_blue),
        AppKind::Terminal => Paint::linear(145.0, Color::rgb_hex(0x2a2d38), Color::rgb_hex(0x0d0e12)),
        AppKind::Editor => Paint::linear(145.0, t.amb_indigo, t.accent_deep),
        AppKind::Monitor => Paint::Linear { angle: 145.0, c0: t.amb_teal, c1: t.amb_blue, c2: t.accent_deep, mid: 0.7 },
        AppKind::Settings => Paint::linear(145.0, Color::rgb_hex(0x6b7385), Color::rgb_hex(0x2c303b)),
    };
    let shadow = if dock {
        Shadow::new(0.0, 2.0, 8.0, 0.0, Color::hex(0x00000033))
    } else {
        Shadow::new(0.0, 4.0, 12.0, 0.0, Color::hex(0x00000040))
    };
    ui.p.shadow(r, rad, shadow);
    ui.p.rect(r, rad, paint);
    ui.p.noise(r, rad, if dock { 0.06 } else { 0.14 });
    let g = if dock { r.w * 21.0 / 44.0 } else { r.w * 0.5 };
    if kind == AppKind::About {
        // Логотип Orbita — ядро с орбитой (символ `logo`), белая версия.
        ui.icon_w(Icon::IOrbit, r.center_box(g, g), Color::WHITE, 1.9 / 1.75);
    } else {
        ui.icon_w(kind.glyph(), r.center_box(g, g), Color::WHITE, if dock { 1.9 / 1.75 } else { 2.0 / 1.75 });
    }
}

/// Запрос приложения к оболочке.
#[derive(Clone, Debug, PartialEq)]
pub enum Request {
    /// Открыть приложение (с аргументом: путь, раздел настроек).
    Open(AppKind, Option<String>),
    /// Закрыть текущее окно.
    CloseSelf,
    /// Показать уведомление.
    Notify(Notice),
}

/// Контекст, который оболочка передаёт приложению каждый кадр.
pub struct AppCx<'a> {
    /// UI-контекст (рисование, ввод, анимации).
    pub ui: &'a mut Ui,
    /// Порт системы.
    pub sys: &'a mut dyn SystemPort,
    /// Общие настройки (приложение «Настройки» их меняет).
    pub settings: &'a mut Settings,
    /// Статистика кадров.
    pub stats: &'a Stats,
    /// Запросы к оболочке, накопленные за кадр.
    pub requests: &'a mut Vec<Request>,
    /// Окно в фокусе (клавиатура принадлежит приложению).
    pub focused: bool,
    /// Идентификатор окна (база для id элементов).
    pub wid: u64,
}

impl AppCx<'_> {
    /// Показать уведомление.
    pub fn notify(&mut self, n: Notice) {
        self.requests.push(Request::Notify(n));
    }

    /// Идентификатор элемента внутри окна.
    pub fn id(&self, s: &str) -> u64 {
        crate::ui::id2(self.wid, crate::ui::id(s))
    }
}

/// Состояние открытого приложения.
pub enum App {
    /// Сведения о системе.
    About(about::AboutApp),
    /// Проводник.
    Files(explorer::ExplorerApp),
    /// Терминал.
    Terminal(terminal::TerminalApp),
    /// Редактор.
    Editor(editor::EditorApp),
    /// Монитор.
    Monitor(monitor::MonitorApp),
    /// Настройки.
    Settings(settings::SettingsApp),
}

impl App {
    /// Создаёт приложение.
    ///
    /// Принимает: вид, необязательный аргумент (путь/раздел) и порт системы.
    pub fn new(kind: AppKind, arg: Option<&str>, sys: &mut dyn SystemPort) -> App {
        match kind {
            AppKind::About => App::About(about::AboutApp::new()),
            AppKind::Files => App::Files(explorer::ExplorerApp::new(arg.unwrap_or("/home/user"), sys)),
            AppKind::Terminal => App::Terminal(terminal::TerminalApp::new(sys)),
            AppKind::Editor => App::Editor(editor::EditorApp::new(arg, sys)),
            AppKind::Monitor => App::Monitor(monitor::MonitorApp::new()),
            AppKind::Settings => App::Settings(settings::SettingsApp::new(arg)),
        }
    }

    /// Вид приложения.
    pub fn kind(&self) -> AppKind {
        match self {
            App::About(_) => AppKind::About,
            App::Files(_) => AppKind::Files,
            App::Terminal(_) => AppKind::Terminal,
            App::Editor(_) => AppKind::Editor,
            App::Monitor(_) => AppKind::Monitor,
            App::Settings(_) => AppKind::Settings,
        }
    }

    /// Заголовок окна.
    pub fn title(&self) -> String {
        match self {
            App::Terminal(t) => t.title(),
            App::Editor(e) => e.title(),
            _ => String::from(self.kind().title()),
        }
    }

    /// Аргумент для восстановления после перезагрузки (путь, раздел).
    pub fn session_arg(&self) -> Option<String> {
        match self {
            App::Files(e) => Some(e.path.clone()),
            App::Editor(e) => e.path.clone(),
            App::Settings(s) => Some(String::from(s.section_key())),
            _ => None,
        }
    }

    /// Фон окна: терминал — `term-bg` 72%, остальные — плотное стекло.
    pub fn tinted_bg(&self, ui: &Ui) -> Option<Color> {
        match self {
            App::Terminal(_) => Some(ui.theme.term_bg.percent(0.72)),
            _ => None,
        }
    }

    /// Содержимое в строке заголовка (адресная строка проводника).
    pub fn title_bar(&mut self, cx: &mut AppCx, area: Rect) {
        if let App::Files(e) = self {
            e.title_bar(cx, area)
        }
    }

    /// Ширина, которую приложение занимает в строке заголовка.
    pub fn title_bar_width(&self) -> f32 {
        match self {
            App::Files(_) => 460.0,
            _ => 0.0,
        }
    }

    /// Рисует тело окна.
    pub fn ui(&mut self, cx: &mut AppCx, body: Rect) {
        match self {
            App::About(a) => a.ui(cx, body),
            App::Files(a) => a.ui(cx, body),
            App::Terminal(a) => a.ui(cx, body),
            App::Editor(a) => a.ui(cx, body),
            App::Monitor(a) => a.ui(cx, body),
            App::Settings(a) => a.ui(cx, body),
        }
    }

    /// Пункты меню «Файл / Правка / Вид» верхней панели для приложения.
    pub fn menus(&self) -> [(&'static str, Vec<crate::ui::MenuItem>); 3] {
        use crate::ui::MenuItem;
        let file = alloc::vec![
            MenuItem::new("Новое окно", Some(Icon::IPlus)).kbd("Ctrl N"),
            MenuItem::new("Открыть проводник", Some(Icon::IFolder)),
            MenuItem::sep(),
            MenuItem::new("Закрыть окно", Some(Icon::IClose)).kbd("Ctrl W"),
        ];
        let edit = alloc::vec![
            MenuItem::new("Копировать", Some(Icon::ICopy)).kbd("Ctrl C"),
            MenuItem::new("Вставить", Some(Icon::IFile)).kbd("Ctrl V"),
            MenuItem::new("Вырезать", Some(Icon::IScissors)).kbd("Ctrl X"),
        ];
        let view = alloc::vec![
            MenuItem::new("Развернуть", Some(Icon::IMax)).kbd("F11"),
            MenuItem::new("Свернуть", Some(Icon::IMinus)),
            MenuItem::sep(),
            MenuItem::new("Оформление…", Some(Icon::IPalette)),
        ];
        [("Файл", file), ("Правка", edit), ("Вид", view)]
    }
}

/// Шапка раздела: заголовок `title-lg` и подзаголовок.
pub fn section_header(ui: &mut Ui, pos: Vec2, title: &str, subtitle: Option<&str>) -> f32 {
    let t = ui.theme;
    ui.p.text(pos, &crate::tokens::text::TITLE_LG, title, t.ink);
    if let Some(s) = subtitle {
        ui.p.text(Vec2::new(pos.x, pos.y + 34.0), &crate::tokens::text::BODY_MD, s, t.ink_muted);
        58.0
    } else {
        40.0
    }
}
