//! Immediate-mode контекст интерфейса и компоненты дизайн-системы.
//!
//! Каждый кадр оболочка заново «описывает» интерфейс вызовами вида
//! `if ui.button(id, rect, "Установить", ButtonKind::Primary).clicked { … }`.
//! Состояние между кадрами — только в явных структурах (поля ввода, флаги)
//! и в хранилище анимаций [`Anims`], поэтому код приложений читается
//! сверху вниз как разметка.
//!
//! ## Слои ввода
//!
//! Окна перекрываются, поэтому наведение получает только верхний слой под
//! курсором. Каждая поверхность объявляет слой ([`Ui::layer`]) со своим
//! прямоугольником; на следующем кадре по этим прямоугольникам выбирается
//! слой под курсором (`hover_layer`). Задержка в один кадр незаметна при
//! 160 FPS и избавляет от двухпроходной раскладки.

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

use crate::anim::{Anims, Ease};
use crate::color::Color;
use crate::geom::{Radii, Rect, Vec2};
use crate::gfx::{GlassStyle, Paint, Painter, Shadow};
use crate::icons::Icon;
use crate::input::{FrameInput, Key, MouseButtons};
use crate::math;
use crate::tokens::{self, Theme, ThemeMode, blur, motion, noise, radius, text};

/// Идентификатор элемента интерфейса (хеш строки и индексов).
pub type Id = u64;

/// Хеш FNV-1a строки — стабильный идентификатор элемента.
///
/// ```
/// use orbita_ui::ui::id;
/// assert_eq!(id("ok"), id("ok"));
/// assert_ne!(id("ok"), id("cancel"));
/// ```
pub const fn id(s: &str) -> Id {
    let b = s.as_bytes();
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut i = 0;
    while i < b.len() {
        h ^= b[i] as u64;
        h = h.wrapping_mul(0x0100_0000_01b3);
        i += 1;
    }
    h
}

/// Производный идентификатор: `parent` + число (строки списков, окна).
///
/// ```
/// use orbita_ui::ui::{id, id2};
/// assert_ne!(id2(id("row"), 1), id2(id("row"), 2));
/// ```
pub const fn id2(parent: Id, n: u64) -> Id {
    let mut h = parent ^ n.wrapping_mul(0x9e37_79b9_7f4a_7c15);
    h ^= h >> 31;
    h = h.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    h ^ (h >> 29)
}

/// Вид курсора мыши (наборы `c-*` дизайн-системы).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CursorKind {
    /// Стрелка.
    Arrow,
    /// Рука — над кликабельным.
    Hand,
    /// I-образный — над текстом.
    Text,
    /// Перемещение (перетаскивание окна).
    Move,
    /// Изменение размера по горизонтали.
    ResizeH,
    /// Ожидание.
    Busy,
}

/// Результат взаимодействия с элементом за кадр.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Resp {
    /// Курсор над элементом (и элемент в верхнем слое).
    pub hovered: bool,
    /// Левая кнопка нажата на элементе в этом кадре.
    pub pressed: bool,
    /// Кнопка удерживается (элемент активен).
    pub down: bool,
    /// Клик: кнопку отпустили над элементом.
    pub clicked: bool,
    /// Двойной клик.
    pub double_clicked: bool,
    /// Клик правой кнопкой.
    pub right_clicked: bool,
    /// Смещение мыши за кадр, пока элемент активен.
    pub drag: Vec2,
}

/// Вид кнопки (`ob-btn--*`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ButtonKind {
    /// Основная: заливка `accent`.
    Primary,
    /// Нейтральная: `stroke-subtle`.
    Neutral,
    /// Призрачная: прозрачная, hover — `accent-soft`.
    Ghost,
    /// Опасная: красный текст на красноватой заливке.
    Danger,
}

/// Размер кнопки (`--h`: 28 / 36 / 44).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ButtonSize {
    /// 28px, `label-sm`.
    Sm,
    /// 36px, `label-md`.
    Md,
    /// 44px, `body-lg` 500.
    Lg,
}

/// Состояние однострочного поля ввода.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TextEdit {
    /// Текст.
    pub text: String,
    /// Позиция курсора (в символах).
    pub cursor: usize,
    /// Начало выделения (в символах); равно `cursor` — выделения нет.
    pub anchor: usize,
}

impl TextEdit {
    /// Поле с начальным текстом, курсор в конце.
    ///
    /// ```
    /// let e = orbita_ui::ui::TextEdit::new("abc");
    /// assert_eq!(e.cursor, 3);
    /// ```
    pub fn new(s: &str) -> TextEdit {
        let n = s.chars().count();
        TextEdit { text: String::from(s), cursor: n, anchor: n }
    }

    fn byte_at(&self, ch: usize) -> usize {
        self.text.char_indices().nth(ch).map(|(i, _)| i).unwrap_or(self.text.len())
    }

    fn len(&self) -> usize {
        self.text.chars().count()
    }

    fn selection(&self) -> (usize, usize) {
        (self.cursor.min(self.anchor), self.cursor.max(self.anchor))
    }

    fn delete_selection(&mut self) -> bool {
        let (a, b) = self.selection();
        if a == b {
            return false;
        }
        let (ba, bb) = (self.byte_at(a), self.byte_at(b));
        self.text.replace_range(ba..bb, "");
        self.cursor = a;
        self.anchor = a;
        true
    }

    /// Выделенный текст.
    pub fn selected(&self) -> &str {
        let (a, b) = self.selection();
        &self.text[self.byte_at(a)..self.byte_at(b)]
    }

    /// Применяет ввод кадра. Возвращает `true`, если текст изменился.
    ///
    /// Поддерживает: набор, Backspace/Delete, ←/→ (+Shift), Home/End,
    /// Ctrl+A/C/X/V (через внутренний буфер обмена).
    ///
    /// ```
    /// use orbita_ui::{FrameInput, ui::TextEdit};
    /// let mut e = TextEdit::new("ab");
    /// let mut clip = String::new();
    /// let mut i = FrameInput::default();
    /// i.text.push('c');
    /// assert!(e.apply(&i, &mut clip));
    /// assert_eq!(e.text, "abc");
    /// ```
    pub fn apply(&mut self, input: &FrameInput, clipboard: &mut String) -> bool {
        let mut changed = false;
        for k in &input.keys {
            if !k.down {
                continue;
            }
            let shift = k.mods.shift;
            match k.key {
                Key::Backspace => {
                    if !self.delete_selection() && self.cursor > 0 {
                        let b = self.byte_at(self.cursor - 1);
                        let e = self.byte_at(self.cursor);
                        self.text.replace_range(b..e, "");
                        self.cursor -= 1;
                        self.anchor = self.cursor;
                    }
                    changed = true;
                }
                Key::Delete => {
                    if !self.delete_selection() && self.cursor < self.len() {
                        let b = self.byte_at(self.cursor);
                        let e = self.byte_at(self.cursor + 1);
                        self.text.replace_range(b..e, "");
                    }
                    changed = true;
                }
                Key::Left => {
                    self.cursor = self.cursor.saturating_sub(1);
                    if !shift {
                        self.anchor = self.cursor;
                    }
                }
                Key::Right => {
                    self.cursor = (self.cursor + 1).min(self.len());
                    if !shift {
                        self.anchor = self.cursor;
                    }
                }
                Key::Home => {
                    self.cursor = 0;
                    if !shift {
                        self.anchor = 0;
                    }
                }
                Key::End => {
                    self.cursor = self.len();
                    if !shift {
                        self.anchor = self.cursor;
                    }
                }
                Key::Char('a') if k.mods.ctrl => {
                    self.anchor = 0;
                    self.cursor = self.len();
                }
                Key::Char('c') if k.mods.ctrl => {
                    *clipboard = String::from(self.selected());
                }
                Key::Char('x') if k.mods.ctrl => {
                    *clipboard = String::from(self.selected());
                    changed |= self.delete_selection();
                }
                Key::Char('v') if k.mods.ctrl => {
                    self.delete_selection();
                    let paste: String = clipboard.chars().filter(|c| *c != '\n').collect();
                    self.insert(&paste);
                    changed = true;
                }
                _ => {}
            }
        }
        if !input.text.is_empty() && !input.mods.ctrl {
            let typed: String = input.text.chars().filter(|c| !c.is_control()).collect();
            if !typed.is_empty() {
                self.delete_selection();
                self.insert(&typed);
                changed = true;
            }
        }
        changed
    }

    /// Вставляет строку в позицию курсора.
    pub fn insert(&mut self, s: &str) {
        let b = self.byte_at(self.cursor);
        self.text.insert_str(b, s);
        self.cursor += s.chars().count();
        self.anchor = self.cursor;
    }

    /// Заменяет текст целиком.
    pub fn set(&mut self, s: &str) {
        self.text = String::from(s);
        self.cursor = self.len();
        self.anchor = self.cursor;
    }
}

/// Пункт меню (`ob-menu__item`).
#[derive(Clone, Debug, PartialEq)]
pub struct MenuItem {
    /// Подпись.
    pub label: String,
    /// Иконка слева.
    pub icon: Option<Icon>,
    /// Горячая клавиша справа (моноширинно).
    pub kbd: Option<&'static str>,
    /// Деструктивное действие (красный).
    pub danger: bool,
    /// Недоступно (45% непрозрачности).
    pub disabled: bool,
    /// Отмечено галочкой.
    pub checked: bool,
    /// Разделитель вместо пункта.
    pub separator: bool,
}

impl MenuItem {
    /// Обычный пункт.
    pub fn new(label: &str, icon: Option<Icon>) -> MenuItem {
        MenuItem {
            label: String::from(label),
            icon,
            kbd: None,
            danger: false,
            disabled: false,
            checked: false,
            separator: false,
        }
    }

    /// Разделитель.
    pub fn sep() -> MenuItem {
        MenuItem { separator: true, ..MenuItem::new("", None) }
    }

    /// С горячей клавишей.
    pub fn kbd(mut self, k: &'static str) -> MenuItem {
        self.kbd = Some(k);
        self
    }

    /// Деструктивный.
    pub fn danger(mut self) -> MenuItem {
        self.danger = true;
        self
    }

    /// С галочкой.
    pub fn checked(mut self, on: bool) -> MenuItem {
        self.checked = on;
        self
    }
}

/// Открытое всплывающее меню (контекстное или выпадающий список).
#[derive(Clone, Debug, PartialEq)]
pub struct Popup {
    /// Владелец — элемент, открывший меню.
    pub owner: Id,
    /// Точка привязки (левый верх меню) в логических px.
    pub at: Vec2,
    /// Минимальная ширина.
    pub min_w: f32,
    /// Пункты.
    pub items: Vec<MenuItem>,
    /// Время открытия (для анимации `ob-pop`).
    pub opened: f64,
}

/// Пользовательские настройки внешнего вида, влияющие на все компоненты.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Look {
    /// Множитель силы размытия стекла (0 — без блюра, 1 — как в макете, до 1.5).
    pub blur: f32,
    /// Шум на стекле включён.
    pub noise: bool,
    /// Анимации включены.
    pub animations: bool,
    /// Акцентный набор курсоров.
    pub accent_cursor: bool,
}

impl Default for Look {
    fn default() -> Look {
        Look { blur: 1.0, noise: true, animations: true, accent_cursor: false }
    }
}

/// Контекст интерфейса на кадр.
pub struct Ui {
    /// Рисовальщик.
    pub p: Painter,
    /// Текущая тема (с учётом акцента).
    pub theme: Theme,
    /// Настройки вида.
    pub look: Look,
    /// Хранилище анимаций.
    pub anims: Anims,
    /// Время кадра (сек).
    pub now: f64,
    /// Длительность прошлого кадра (сек).
    pub dt: f32,
    /// Ввод кадра.
    pub input: FrameInput,
    /// Указатель (логические px); далеко за экраном, если мыши нет.
    pub mouse: Vec2,
    prev_buttons: MouseButtons,
    /// Элемент под курсором в этом кадре.
    pub hot: Option<Id>,
    /// Элемент, на котором зажата кнопка мыши.
    pub active: Option<Id>,
    /// Элемент с фокусом клавиатуры.
    pub focus: Option<Id>,
    /// Курсор, запрошенный элементами этого кадра.
    pub cursor: CursorKind,
    layer: u32,
    hover_layer: u32,
    layers_prev: Vec<(u32, Rect)>,
    layers_cur: Vec<(u32, Rect)>,
    hit_clip: Vec<Rect>,
    last_click: (f64, Vec2),
    double: bool,
    /// Внутренний буфер обмена.
    pub clipboard: String,
    /// Открытое всплывающее меню.
    pub popup: Option<Popup>,
    popup_result: Option<(Id, usize)>,
    /// Клавиши уже обработаны фокусным элементом (не передавать дальше).
    pub keys_taken: bool,
    /// Текст-подсказка под курсором (рисуется поверх всего).
    pub tooltip: Option<(String, Rect)>,
}

/// Номер слоя для всплывающих меню (выше всего, кроме курсора).
pub const LAYER_POPUP: u32 = 1_000_000;

impl Ui {
    /// Создаёт контекст с тёмной темой.
    pub fn new() -> Ui {
        Ui {
            p: Painter::new(),
            theme: tokens::DARK,
            look: Look::default(),
            anims: Anims::new(),
            now: 0.0,
            dt: 1.0 / 60.0,
            input: FrameInput::default(),
            mouse: Vec2::new(-1e6, -1e6),
            prev_buttons: MouseButtons(0),
            hot: None,
            active: None,
            focus: None,
            cursor: CursorKind::Arrow,
            layer: 0,
            hover_layer: 0,
            layers_prev: Vec::new(),
            layers_cur: Vec::new(),
            hit_clip: Vec::new(),
            last_click: (-10.0, Vec2::ZERO),
            double: false,
            clipboard: String::new(),
            popup: None,
            popup_result: None,
            keys_taken: false,
            tooltip: None,
        }
    }

    /// Начинает кадр: принимает ввод, выбирает слой под курсором.
    pub fn begin(&mut self, input: &FrameInput) {
        let prev_time = self.now;
        self.input = input.clone();
        self.now = input.time;
        self.dt = math::clamp((self.now - prev_time) as f32, 0.0, 0.1);
        self.anims.speed = if self.look.animations { 1.0 } else { 0.0 };
        self.mouse = input.mouse.unwrap_or(Vec2::new(-1e6, -1e6));
        self.hot = None;
        self.cursor = CursorKind::Arrow;
        self.keys_taken = false;
        self.tooltip = None;
        self.layers_prev = core::mem::take(&mut self.layers_cur);
        self.hover_layer = 0;
        for (layer, r) in &self.layers_prev {
            if r.contains(self.mouse) && *layer >= self.hover_layer {
                self.hover_layer = *layer;
            }
        }
        self.layer = 0;
        self.hit_clip.clear();
        // Двойной клик: два нажатия за 400 мс рядом.
        self.double = false;
        if self.left_pressed() {
            let (t, p) = self.last_click;
            if self.now - t < 0.4 && p.sub(self.mouse).length() < 6.0 {
                self.double = true;
                self.last_click = (-10.0, Vec2::ZERO);
            } else {
                self.last_click = (self.now, self.mouse);
            }
        }
        if !self.input.buttons.has(MouseButtons::LEFT) && !self.left_released() {
            self.active = None;
        }
        let bg = self.theme.surface_base;
        self.p.begin_frame(input.width, input.height, input.scale, bg);
    }

    /// Завершает кадр (после отрисовки курсора).
    pub fn end(&mut self) {
        if self.left_released() {
            self.active = None;
        }
        self.prev_buttons = self.input.buttons;
    }

    /// Левая кнопка нажата в этом кадре.
    pub fn left_pressed(&self) -> bool {
        self.input.buttons.has(MouseButtons::LEFT) && !self.prev_buttons.has(MouseButtons::LEFT)
    }

    /// Левая кнопка отпущена в этом кадре.
    pub fn left_released(&self) -> bool {
        !self.input.buttons.has(MouseButtons::LEFT) && self.prev_buttons.has(MouseButtons::LEFT)
    }

    /// Правая кнопка нажата в этом кадре.
    pub fn right_pressed(&self) -> bool {
        self.input.buttons.has(MouseButtons::RIGHT) && !self.prev_buttons.has(MouseButtons::RIGHT)
    }

    /// Логический размер экрана.
    pub fn screen(&self) -> Rect {
        self.p.screen
    }

    // ------------------------------------------------------------ слои

    /// Объявляет поверхность-слой: всё до следующего вызова принадлежит ей.
    ///
    /// Принимает: номер слоя (больше — выше) и его прямоугольник.
    pub fn layer(&mut self, layer: u32, rect: Rect) {
        self.layer = layer;
        self.layers_cur.push((layer, rect));
    }

    /// Текущий слой.
    pub fn current_layer(&self) -> u32 {
        self.layer
    }

    /// Находится ли курсор над текущим слоем (и он верхний под курсором).
    pub fn layer_hovered(&self) -> bool {
        self.layer == self.hover_layer
    }

    /// Слой под курсором (по раскладке прошлого кадра).
    pub fn hover_layer(&self) -> u32 {
        self.hover_layer
    }

    /// Отсечение для рисования и попадания мыши одновременно.
    pub fn push_clip(&mut self, r: Rect, radius: f32) {
        let cur = self.hit_clip.last().copied().unwrap_or(Rect::new(-1e6, -1e6, 2e6, 2e6));
        self.hit_clip.push(cur.intersect(&r));
        self.p.push_clip(r, radius);
    }

    /// Снимает отсечение.
    pub fn pop_clip(&mut self) {
        self.hit_clip.pop();
        self.p.pop_clip();
    }

    /// Курсор над прямоугольником с учётом слоя и отсечения.
    pub fn hovering(&self, r: Rect) -> bool {
        if !self.layer_hovered() || !r.contains(self.mouse) {
            return false;
        }
        match self.hit_clip.last() {
            Some(c) => c.contains(self.mouse),
            None => true,
        }
    }

    // ------------------------------------------------------------ взаимодействие

    /// Базовое взаимодействие с прямоугольной областью.
    ///
    /// Принимает: идентификатор и прямоугольник. Возвращает: [`Resp`].
    pub fn interact(&mut self, id: Id, r: Rect) -> Resp {
        let hovered = self.hovering(r) && (self.active.is_none() || self.active == Some(id));
        if hovered {
            self.hot = Some(id);
        }
        let mut resp = Resp { hovered, ..Default::default() };
        if hovered && self.left_pressed() {
            self.active = Some(id);
            resp.pressed = true;
            resp.double_clicked = self.double;
        }
        if hovered && self.right_pressed() {
            resp.right_clicked = true;
        }
        if self.active == Some(id) {
            resp.down = self.input.buttons.has(MouseButtons::LEFT);
            if self.left_released() {
                resp.clicked = self.hovering(r);
            }
        }
        resp
    }

    /// Взаимодействие для перетаскивания: смещение мыши за кадр в `drag`.
    pub fn drag_area(&mut self, id: Id, r: Rect, prev_mouse: &mut Vec2) -> Resp {
        let mut resp = self.interact(id, r);
        if resp.pressed {
            *prev_mouse = self.mouse;
        }
        if self.active == Some(id) && self.input.buttons.has(MouseButtons::LEFT) {
            resp.drag = self.mouse.sub(*prev_mouse);
            *prev_mouse = self.mouse;
        }
        resp
    }

    /// Плавное значение 0/1 для hover/active (переход 140 мс).
    pub fn hover_t(&mut self, id: Id, on: bool) -> f32 {
        let now = self.now;
        self.anims.to(id2(id, 0x40), if on { 1.0 } else { 0.0 }, motion::FAST, Ease::Soft, now)
    }

    /// Плавное значение с произвольной длительностью и кривой.
    pub fn anim(&mut self, key: Id, target: f32, dur: f32, ease: Ease) -> f32 {
        let now = self.now;
        self.anims.to(key, target, dur, ease, now)
    }

    /// Результат выбора в меню, открытом элементом `owner` (один раз).
    pub fn take_popup_result(&mut self, owner: Id) -> Option<usize> {
        match self.popup_result {
            Some((o, i)) if o == owner => {
                self.popup_result = None;
                Some(i)
            }
            _ => None,
        }
    }

    /// Открывает всплывающее меню.
    pub fn open_popup(&mut self, owner: Id, at: Vec2, min_w: f32, items: Vec<MenuItem>) {
        let now = self.now;
        self.anims.start(id2(owner, 0x77), 0.0, 1.0, motion::BASE, Ease::Out, now);
        self.popup = Some(Popup { owner, at, min_w, items, opened: now });
    }

    /// Открыто ли меню, принадлежащее `owner`.
    pub fn popup_open_for(&self, owner: Id) -> bool {
        self.popup.as_ref().map(|p| p.owner == owner).unwrap_or(false)
    }

    // ------------------------------------------------------------ стили

    /// Стиль обычного стекла (`ob-glass`).
    pub fn glass_style(&self) -> GlassStyle {
        let t = &self.theme;
        GlassStyle {
            blur: blur::MD * self.look.blur,
            saturate: 1.65,
            fill: t.glass_fill,
            border: t.glass_border,
            highlight: t.glass_highlight,
            noise: if self.look.noise { noise::DEFAULT } else { 0.0 },
        }
    }

    /// Стиль плотного стекла меню и тостов (`ob-menu`, `ob-toast`).
    pub fn menu_style(&self) -> GlassStyle {
        let t = &self.theme;
        GlassStyle {
            blur: blur::LG * self.look.blur,
            saturate: 1.8,
            fill: t.glass_fill_strong,
            border: Color::TRANSPARENT,
            highlight: t.glass_highlight,
            noise: if self.look.noise { noise::DEFAULT } else { 0.0 },
        }
    }

    /// Тень меню (`shadow-menu`).
    pub fn menu_shadow(&self) -> [Shadow; 1] {
        match self.theme.mode {
            ThemeMode::Dark => [Shadow::new(0.0, 16.0, 48.0, 0.0, Color::hex(0x000000a6))],
            ThemeMode::Light => [Shadow::new(0.0, 16.0, 48.0, 0.0, Color::hex(0x1e3a8a33))],
        }
    }

    /// Тени стеклянной панели (`shadow-glass`).
    pub fn glass_shadow(&self) -> [Shadow; 2] {
        match self.theme.mode {
            ThemeMode::Dark => [
                Shadow::new(0.0, 8.0, 32.0, 0.0, Color::hex(0x00000066)),
                Shadow::new(0.0, 1.0, 2.0, 0.0, Color::hex(0x00000080)),
            ],
            ThemeMode::Light => [
                Shadow::new(0.0, 8.0, 32.0, 0.0, Color::hex(0x1e3a8a24)),
                Shadow::new(0.0, 1.0, 2.0, 0.0, Color::hex(0x0f172a1a)),
            ],
        }
    }

    /// Свечение акцента (`shadow-glow`).
    pub fn glow(&self) -> [Shadow; 2] {
        let t = &self.theme;
        match t.mode {
            ThemeMode::Dark => [
                Shadow::new(0.0, 0.0, 24.0, 0.0, t.accent.with_alpha(0.5)),
                Shadow::new(0.0, 0.0, 2.0, 0.0, t.accent_text.with_alpha(0.8)),
            ],
            ThemeMode::Light => [
                Shadow::new(0.0, 6.0, 20.0, 0.0, t.accent.with_alpha(0.35)),
                Shadow::new(0.0, 0.0, 2.0, 0.0, t.accent),
            ],
        }
    }

    /// Кольцо фокуса: 2px `focus-ring` с зазором `gap`.
    pub fn focus_ring(&mut self, r: Rect, rad: f32, gap: f32) {
        let ring = r.inset(-(gap + 2.0));
        let c = self.theme.focus_ring;
        self.p.stroke_rect(ring, rad + gap + 2.0, 2.0, c);
    }

    /// Иконка в квадрате `r` цветом `color` (штрих 1.75 на сетке 24).
    pub fn icon(&mut self, icon: Icon, r: Rect, color: Color) {
        let theme = self.theme;
        self.p.icon(icon, r, color, &theme, 1.0);
    }

    /// Иконка с множителем толщины штриха.
    pub fn icon_w(&mut self, icon: Icon, r: Rect, color: Color, stroke_k: f32) {
        let theme = self.theme;
        self.p.icon(icon, r, color, &theme, stroke_k);
    }

    /// Текст (левый верх строки).
    pub fn label(&mut self, pos: Vec2, style: &tokens::TextStyle, s: &str, color: Color) -> f32 {
        self.p.text(pos, style, s, color)
    }

    // ------------------------------------------------------------ компоненты

    /// Ширина кнопки с подписью (и иконкой) для раскладки.
    pub fn button_width(&self, label: &str, size: ButtonSize, icon: bool) -> f32 {
        let (style, pad) = button_metrics(size);
        let mut w = self.p.measure(&style, label) + pad * 2.0;
        if icon {
            w += 18.0 + if label.is_empty() { 0.0 } else { 8.0 };
        }
        w
    }

    /// Кнопка `ob-btn`.
    ///
    /// Принимает: id, прямоугольник, подпись, вид, размер, иконку.
    /// Возвращает: [`Resp`] (`clicked` — сработала).
    pub fn button(&mut self, id: Id, r: Rect, label: &str, kind: ButtonKind, size: ButtonSize, icon: Option<Icon>) -> Resp {
        let resp = self.interact(id, r);
        if resp.hovered {
            self.cursor = CursorKind::Hand;
        }
        let h = self.hover_t(id, resp.hovered);
        let a = self.hover_t(id2(id, 1), resp.down && resp.hovered);
        let t = self.theme;
        let rad = match size {
            ButtonSize::Sm => radius::SM,
            ButtonSize::Md => radius::MD,
            ButtonSize::Lg => radius::MD,
        };
        let (bg, fg) = match kind {
            ButtonKind::Primary => {
                let base = t.accent.mix(t.accent_hover, h);
                (base.mix(t.accent_pressed, a), t.on_accent)
            }
            ButtonKind::Neutral => {
                let base = t.stroke_subtle.mix(t.stroke_default, h);
                (base.mix(t.accent_soft, a), t.ink)
            }
            ButtonKind::Ghost => (Color::TRANSPARENT.mix(t.accent_soft, math::max(h, a)), t.ink),
            ButtonKind::Danger => (t.danger.with_alpha(0.14 + 0.10 * h), t.danger),
        };
        // `.ob-btn:active { transform: scale(.98) }`
        let k = 1.0 - 0.02 * a;
        self.p.push_transform(r.center(), k, Vec2::ZERO);
        self.p.rect(r, rad, Paint::Solid(bg));
        let (style, pad) = button_metrics(size);
        let tw = if label.is_empty() { 0.0 } else { self.p.measure(&style, label) };
        let iw = if icon.is_some() { 18.0 } else { 0.0 };
        let gap = if icon.is_some() && !label.is_empty() { 8.0 } else { 0.0 };
        let total = tw + iw + gap;
        let x = r.x + math::max(pad, (r.w - total) * 0.5);
        if let Some(ic) = icon {
            self.icon(ic, Rect::new(x, r.y + (r.h - 18.0) * 0.5, 18.0, 18.0), fg);
        }
        if !label.is_empty() {
            self.p.text(Vec2::new(x + iw + gap, r.y + (r.h - style.line) * 0.5), &style, label, fg);
        }
        self.p.pop_transform();
        resp
    }

    /// Квадратная кнопка-иконка (кнопки окна `ob-wc`, чипы панели).
    pub fn icon_button(&mut self, id: Id, r: Rect, icon: Icon, danger_hover: bool) -> Resp {
        let resp = self.interact(id, r);
        let h = self.hover_t(id, resp.hovered);
        let t = self.theme;
        let bg = if danger_hover { t.danger.with_alpha(h) } else { t.accent_soft.fade(h) };
        let fg = if danger_hover && h > 0.5 {
            Color::rgb_hex(0x14161a)
        } else {
            t.ink_muted.mix(t.ink, h)
        };
        self.p.rect(r, radius::SM, Paint::Solid(bg));
        let s = if r.h <= 26.0 { 14.0 } else { 16.0 };
        self.icon(icon, r.center_box(s, s), fg);
        resp
    }

    /// Переключатель `ob-toggle` 44×26.
    pub fn toggle(&mut self, id: Id, r: Rect, on: &mut bool) -> Resp {
        let resp = self.interact(id, r);
        if resp.clicked {
            *on = !*on;
        }
        if resp.hovered {
            self.cursor = CursorKind::Hand;
        }
        let t = self.theme;
        let k = self.anim(id2(id, 2), if *on { 1.0 } else { 0.0 }, motion::BASE, Ease::Out);
        let bg = t.surface_sunken.mix(t.accent, k);
        let border = t.stroke_strong.mix(t.accent_line, k);
        self.p.rect_ex(r, Radii::all(radius::PILL), Paint::Solid(bg), 1.0, border);
        let knob = Rect::new(r.x + 3.0 + (r.w - 24.0) * k, r.y + (r.h - 18.0) * 0.5, 18.0, 18.0);
        self.p.shadow(knob, 9.0, Shadow::new(0.0, 1.0, 3.0, 0.0, Color::hex(0x0000004d)));
        self.p.rect(knob, 9.0, Paint::Solid(t.ink_muted.mix(t.on_accent, k)));
        resp
    }

    /// Чекбокс `ob-check` 18×18 с подписью справа.
    pub fn checkbox(&mut self, id: Id, pos: Vec2, label: &str, on: &mut bool) -> Resp {
        let w = 18.0 + if label.is_empty() { 0.0 } else { 10.0 + self.p.measure(&text::BODY_MD, label) };
        let hit = Rect::new(pos.x, pos.y, w, 20.0);
        let resp = self.interact(id, hit);
        if resp.clicked {
            *on = !*on;
        }
        let t = self.theme;
        let k = self.anim(id2(id, 2), if *on { 1.0 } else { 0.0 }, motion::BASE, Ease::Out);
        let b = Rect::new(pos.x, pos.y + 1.0, 18.0, 18.0);
        let bg = t.surface_sunken.mix(t.accent, k);
        self.p.rect_ex(b, Radii::all(radius::XS), Paint::Solid(bg), 1.5, t.stroke_strong.mix(t.accent_line, k));
        if k > 0.01 {
            self.p.push_opacity(k);
            self.icon_w(Icon::ICheck, b.center_box(12.0, 12.0), t.on_accent, 3.0 / 1.75);
            self.p.pop_opacity();
        }
        if !label.is_empty() {
            self.p.text(Vec2::new(pos.x + 28.0, pos.y), &text::BODY_MD, label, t.ink);
        }
        resp
    }

    /// Радиокнопка `ob-radio` 18×18 с подписью.
    pub fn radio(&mut self, id: Id, pos: Vec2, label: &str, selected: bool) -> Resp {
        let w = 18.0 + if label.is_empty() { 0.0 } else { 10.0 + self.p.measure(&text::BODY_MD, label) };
        let resp = self.interact(id, Rect::new(pos.x, pos.y, w, 20.0));
        let t = self.theme;
        let k = self.anim(id2(id, 2), if selected { 1.0 } else { 0.0 }, motion::BASE, Ease::Out);
        let b = Rect::new(pos.x, pos.y + 1.0, 18.0, 18.0);
        self.p.rect_ex(b, Radii::all(9.0), Paint::Solid(t.surface_sunken), 1.5, t.stroke_strong.mix(t.accent_line, k));
        if k > 0.01 {
            let d = 8.0 * k;
            self.p.rect(b.center_box(d, d), d * 0.5, Paint::Solid(t.accent_text));
        }
        if !label.is_empty() {
            self.p.text(Vec2::new(pos.x + 28.0, pos.y), &text::BODY_MD, label, t.ink);
        }
        resp
    }

    /// Слайдер `ob-slider`: трек 6px с градиентом и бегунок 18px.
    ///
    /// Принимает: значение `v ∈ [0, 1]`. Возвращает `true`, если изменилось.
    pub fn slider(&mut self, id: Id, r: Rect, v: &mut f32) -> bool {
        let hit = r.inset_xy(-4.0, -6.0);
        let resp = self.interact(id, hit);
        let mut changed = false;
        if self.active == Some(id) && self.input.buttons.has(MouseButtons::LEFT) {
            let nv = math::clamp((self.mouse.x - r.x) / r.w, 0.0, 1.0);
            if (nv - *v).abs_f() > 1e-4 {
                *v = nv;
                changed = true;
            }
        }
        if resp.hovered || self.active == Some(id) {
            self.cursor = CursorKind::Hand;
        }
        let t = self.theme;
        let track = Rect::new(r.x, r.y + (r.h - 6.0) * 0.5, r.w, 6.0);
        self.p.rect_ex(track, Radii::all(3.0), Paint::Solid(t.surface_sunken), 1.0, t.stroke_default);
        let fill = Rect::new(track.x, track.y, math::max(track.w * *v, 6.0), 6.0);
        self.p.rect(fill, 3.0, Paint::linear(90.0, t.amb_blue, t.accent_text));
        let h = self.hover_t(id, resp.hovered || self.active == Some(id));
        let size = 18.0 + 2.0 * h;
        let thumb = Rect::new(r.x + r.w * *v - size * 0.5, r.y + (r.h - size) * 0.5, size, size);
        self.p.shadow(thumb, size * 0.5, Shadow::new(0.0, 2.0, 6.0, 0.0, Color::hex(0x00000066)));
        self.p.rect_ex(thumb, Radii::all(size * 0.5), Paint::Solid(Color::WHITE), 2.0, t.accent);
        changed
    }

    /// Сегментированный переключатель `ob-seg`.
    ///
    /// Возвращает `true`, если выбор изменился.
    pub fn segmented(&mut self, id: Id, r: Rect, options: &[&str], selected: &mut usize) -> bool {
        let t = self.theme;
        self.p.rect_ex(r, Radii::all(radius::MD), Paint::Solid(t.surface_sunken), 1.0, t.stroke_default);
        let inner = r.inset(3.0);
        let n = options.len().max(1) as f32;
        let w = (inner.w - 2.0 * (n - 1.0)) / n;
        let mut changed = false;
        // Плавно скользящая подложка выбранного сегмента.
        let pos = self.anim(id2(id, 3), *selected as f32, motion::BASE, Ease::Out);
        let pill = Rect::new(inner.x + pos * (w + 2.0), inner.y, w, inner.h);
        let glow = self.glow();
        for s in &glow {
            self.p.shadow(pill, radius::MD - 3.0, *s);
        }
        self.p.rect(pill, radius::MD - 3.0, Paint::Solid(t.accent));
        for (i, opt) in options.iter().enumerate() {
            let cell = Rect::new(inner.x + i as f32 * (w + 2.0), inner.y, w, inner.h);
            let resp = self.interact(id2(id, 100 + i as u64), cell);
            if resp.clicked && *selected != i {
                *selected = i;
                changed = true;
            }
            let on = math::clamp(1.0 - (pos - i as f32).abs_f(), 0.0, 1.0);
            let fg = t.ink_muted.mix(t.on_accent, on);
            let style = tokens::TextStyle { face: crate::gfx::text::Face::Sans500, size: 13.0, line: 18.0, tracking: 0.0 };
            self.p.text_in(cell, &style, opt, fg, 0.5);
        }
        changed
    }

    /// Однострочное поле ввода `ob-field` (иконка, плейсхолдер, курсор).
    ///
    /// Возвращает: [`Resp`]; `clicked` здесь означает Enter.
    pub fn text_field(&mut self, id: Id, r: Rect, edit: &mut TextEdit, placeholder: &str, icon: Option<Icon>) -> Resp {
        let mut resp = self.interact(id, r);
        if resp.hovered {
            self.cursor = CursorKind::Text;
        }
        if resp.pressed {
            self.focus = Some(id);
        } else if self.left_pressed() && !resp.hovered && self.focus == Some(id) {
            self.focus = None;
        }
        let focused = self.focus == Some(id);
        let t = self.theme;
        let style = text::BODY_MD;
        let mut x = r.x + 12.0;
        let f = self.anim(id2(id, 5), if focused { 1.0 } else { 0.0 }, motion::FAST, Ease::Soft);
        self.p.rect(r, radius::SM, Paint::Solid(t.stroke_subtle.mix(t.stroke_default, f)));
        if f > 0.01 {
            self.p.push_opacity(f);
            self.p.stroke_rect(r.inset(-2.0), radius::SM + 2.0, 2.0, t.focus_ring);
            self.p.pop_opacity();
        }
        if let Some(ic) = icon {
            self.icon(ic, Rect::new(x, r.y + (r.h - 16.0) * 0.5, 16.0, 16.0), t.ink_muted);
            x += 24.0;
        }
        let ty = r.y + (r.h - style.line) * 0.5;
        let area = Rect::new(x, r.y, r.right() - x - 8.0, r.h);
        if focused && !self.keys_taken {
            let mut clip = core::mem::take(&mut self.clipboard);
            edit.apply(&self.input, &mut clip);
            self.clipboard = clip;
            if self.input.pressed(Key::Enter) {
                resp.clicked = true;
            }
            self.keys_taken = true;
        }
        self.push_clip(area, 0.0);
        if edit.text.is_empty() {
            self.p.text(Vec2::new(x, ty), &style, placeholder, t.ink_faint);
        } else {
            let carets = self.p.fonts.caret_positions(&style, &edit.text);
            let (a, b) = (edit.cursor.min(edit.anchor), edit.cursor.max(edit.anchor));
            if a != b {
                let sx = x + carets[a.min(carets.len() - 1)];
                let ex = x + carets[b.min(carets.len() - 1)];
                self.p.rect(Rect::new(sx, ty, ex - sx, style.line), radius::XS, Paint::Solid(t.selection_bg));
            }
            self.p.text(Vec2::new(x, ty), &style, &edit.text, t.ink);
        }
        if focused {
            let carets = self.p.fonts.caret_positions(&style, &edit.text);
            let cx = x + carets[edit.cursor.min(carets.len() - 1)];
            let blink = ((self.now * 1.0 / 1.1).fract_f() < 0.56) || !self.look.animations;
            self.anims.mark_active();
            if blink {
                self.p.rect(Rect::new(cx, ty + 1.0, 1.5, style.line - 2.0), 0.0, Paint::Solid(t.accent_text));
            }
        }
        self.pop_clip();
        resp
    }

    /// Тег `ob-tag` (капсула 22px). Возвращает его ширину.
    pub fn tag(&mut self, pos: Vec2, label: &str) -> f32 {
        let t = self.theme;
        let w = self.p.measure(&text::LABEL_SM, label) + 20.0;
        let r = Rect::new(pos.x, pos.y, w, 22.0);
        self.p.rect_ex(r, Radii::all(11.0), Paint::Solid(t.accent_soft), 1.0, t.stroke_default);
        self.p.text_in(r, &text::LABEL_SM, label, t.ink, 0.5);
        w
    }

    /// Триггер выпадающего списка `ob-select`; по клику открывает меню.
    ///
    /// Возвращает: `Some(i)` — в этом кадре выбран вариант `i`.
    pub fn select(&mut self, id: Id, r: Rect, options: &[&str], selected: usize) -> Option<usize> {
        let resp = self.interact(id, r);
        let open = self.popup_open_for(id);
        let h = self.hover_t(id, resp.hovered || open);
        let t = self.theme;
        self.p.rect(r, radius::MD, Paint::Solid(t.stroke_subtle.mix(t.stroke_default, h)));
        let label = options.get(selected).copied().unwrap_or("");
        self.p.text_in(Rect::new(r.x + 12.0, r.y, r.w - 40.0, r.h), &text::BODY_MD, label, t.ink, 0.0);
        self.icon(Icon::IChevD, Rect::new(r.right() - 28.0, r.y + (r.h - 16.0) * 0.5, 16.0, 16.0), t.ink_muted);
        if resp.clicked {
            let items = options
                .iter()
                .enumerate()
                .map(|(i, o)| MenuItem::new(o, None).checked(i == selected))
                .collect();
            self.open_popup(id, Vec2::new(r.x, r.bottom() + 6.0), r.w, items);
        }
        self.take_popup_result(id)
    }

    /// Пункт боковой навигации `ob-nav` (34px).
    pub fn nav_item(&mut self, id: Id, r: Rect, icon: Option<Icon>, label: &str, on: bool) -> Resp {
        let resp = self.interact(id, r);
        let h = self.hover_t(id, resp.hovered);
        let s = self.anim(id2(id, 9), if on { 1.0 } else { 0.0 }, motion::FAST, Ease::Soft);
        let t = self.theme;
        let bg = t.accent_soft.fade(h * (1.0 - s)).mix(t.accent, s);
        self.p.rect(r, radius::SM, Paint::Solid(bg));
        let fg = t.ink.mix(t.on_accent, s);
        let ic = t.ink_muted.mix(t.on_accent, s);
        let mut x = r.x + 12.0;
        if let Some(i) = icon {
            self.icon(i, Rect::new(x, r.y + (r.h - 18.0) * 0.5, 18.0, 18.0), ic);
            x += 30.0;
        }
        self.p.text_in(Rect::new(x, r.y, r.right() - x - 8.0, r.h), &text::LABEL_MD, label, fg, 0.0);
        resp
    }

    /// Пункт меню-поповера; возвращает true при клике.
    fn menu_row(&mut self, id: Id, r: Rect, item: &MenuItem, kbd_w: f32) -> bool {
        let resp = self.interact(id, r);
        let t = self.theme;
        let h = if item.disabled { 0.0 } else { self.hover_t(id, resp.hovered) };
        let bg = if item.danger { t.danger.with_alpha(0.16 * h) } else { t.accent_soft.fade(h) };
        self.p.rect(r, radius::SM, Paint::Solid(bg));
        if item.disabled {
            self.p.push_opacity(0.45);
        }
        let fg = if item.danger { t.danger } else { t.ink };
        let ic = if item.danger { t.danger } else { t.ink_muted.mix(t.accent_text, h) };
        let mut x = r.x + 12.0;
        if item.checked {
            self.icon(Icon::ICheck, Rect::new(x, r.y + (r.h - 16.0) * 0.5, 16.0, 16.0), t.accent_text);
            x += 28.0;
        } else if let Some(i) = item.icon {
            self.icon(i, Rect::new(x, r.y + (r.h - 18.0) * 0.5, 18.0, 18.0), ic);
            x += 30.0;
        }
        self.p.text_in(Rect::new(x, r.y, r.right() - x - kbd_w - 12.0, r.h), &text::BODY_MD, &item.label, fg, 0.0);
        if let Some(k) = item.kbd {
            let st = tokens::TextStyle { face: crate::gfx::text::Face::Mono400, size: 12.0, line: 16.0, tracking: 0.0 };
            self.p.text_in(Rect::new(r.right() - kbd_w - 12.0, r.y, kbd_w, r.h), &st, k, t.ink_muted, 1.0);
        }
        if item.disabled {
            self.p.pop_opacity();
        }
        resp.clicked && !item.disabled
    }

    /// Рисует открытое меню (вызывается оболочкой поверх всего).
    pub fn draw_popup(&mut self) {
        let Some(popup) = self.popup.clone() else { return };
        let screen = self.screen();
        let t = self.theme;
        let st_kbd = tokens::TextStyle { face: crate::gfx::text::Face::Mono400, size: 12.0, line: 16.0, tracking: 0.0 };
        let mut label_w: f32 = 0.0;
        let mut kbd_w: f32 = 0.0;
        let mut h = 12.0;
        for it in &popup.items {
            if it.separator {
                h += 9.0;
                continue;
            }
            h += 36.0;
            let extra = if it.icon.is_some() || it.checked { 30.0 } else { 0.0 };
            label_w = math::max(label_w, self.p.measure(&text::BODY_MD, &it.label) + extra);
            if let Some(k) = it.kbd {
                kbd_w = math::max(kbd_w, self.p.measure(&st_kbd, k));
            }
        }
        let w = math::max(popup.min_w, math::max(220.0, label_w + kbd_w + 60.0));
        let mut x = popup.at.x;
        let mut y = popup.at.y;
        if x + w > screen.right() - 8.0 {
            x = screen.right() - 8.0 - w;
        }
        if y + h > screen.bottom() - 8.0 {
            y = math::max(8.0, popup.at.y - h - 6.0);
        }
        let r = Rect::new(x, y, w, h);
        self.layer(LAYER_POPUP, r);
        let k = self.anims.get(id2(popup.owner, 0x77), self.now).unwrap_or(1.0);
        if !self.anims.finished(id2(popup.owner, 0x77), self.now) {
            self.anims.mark_active();
        }
        // `ob-pop`: из translateY(6px) scale(.97) и прозрачности.
        self.p.push_opacity(k);
        self.p.push_transform(Vec2::new(r.center().x, r.y), 0.97 + 0.03 * k, Vec2::new(0.0, 6.0 * (1.0 - k)));
        let style = self.menu_style();
        let shadow = self.menu_shadow();
        self.p.glass(r, radius::LG, &style, &shadow);
        let mut yy = r.y + 6.0;
        let mut chosen = None;
        for (i, it) in popup.items.iter().enumerate() {
            if it.separator {
                self.p.rect(Rect::new(r.x + 12.0, yy + 4.0, r.w - 24.0, 1.0), 0.0, Paint::Solid(t.stroke_subtle));
                yy += 9.0;
                continue;
            }
            let row = Rect::new(r.x + 6.0, yy + 1.0, r.w - 12.0, 34.0);
            if self.menu_row(id2(popup.owner, 0x1000 + i as u64), row, it, kbd_w) {
                chosen = Some(i);
            }
            yy += 36.0;
        }
        self.p.pop_transform();
        self.p.pop_opacity();
        if let Some(i) = chosen {
            self.popup_result = Some((popup.owner, i));
            self.popup = None;
        } else if (self.left_pressed() || self.right_pressed()) && !r.contains(self.mouse) {
            self.popup = None;
        } else if self.input.pressed(Key::Escape) {
            self.popup = None;
            self.keys_taken = true;
        }
    }

    /// Вертикальная прокрутка: обрабатывает колесо над `viewport` и
    /// возвращает плавное смещение содержимого (px).
    ///
    /// Принимает: id, видимую область, полную высоту содержимого,
    /// целевое смещение (хранится у вызывающего).
    pub fn scroll(&mut self, id: Id, viewport: Rect, content_h: f32, target: &mut f32) -> f32 {
        let max = math::max(0.0, content_h - viewport.h);
        if self.hovering(viewport) && self.input.wheel.y != 0.0 {
            *target += self.input.wheel.y;
        }
        *target = math::clamp(*target, 0.0, max);
        let v = self.anim(id2(id, 0x5c), *target, motion::BASE, Ease::Out);
        // Полоса прокрутки — тонкая капсула, появляется только когда есть что крутить.
        if max > 0.5 {
            let t = self.theme;
            let bar_h = math::max(24.0, viewport.h * viewport.h / content_h);
            let y = viewport.y + (viewport.h - bar_h) * (v / max);
            let hov = self.hovering(viewport);
            let a = self.hover_t(id2(id, 0x5d), hov);
            self.p.rect(
                Rect::new(viewport.right() - 6.0, y + 2.0, 4.0, bar_h - 4.0),
                2.0,
                Paint::Solid(t.ink_muted.with_alpha(0.25 + 0.25 * a)),
            );
        }
        v
    }

    /// Подсказка рядом с элементом (`ob-dock__tip`).
    pub fn show_tooltip(&mut self, text: &str, anchor: Rect) {
        self.tooltip = Some((String::from(text), anchor));
    }

    /// Рисует подсказку (вызывается оболочкой поверх всего).
    pub fn draw_tooltip(&mut self) {
        let Some((txt, anchor)) = self.tooltip.clone() else { return };
        let t = self.theme;
        let w = self.p.measure(&text::LABEL_SM, &txt) + 20.0;
        let r = Rect::new(anchor.center().x - w * 0.5, anchor.y - 14.0 - 24.0, w, 24.0);
        let style = GlassStyle { blur: blur::MD * self.look.blur, saturate: 1.6, fill: t.glass_fill_strong, border: Color::TRANSPARENT, highlight: Color::TRANSPARENT, noise: 0.0 };
        let sh = self.menu_shadow();
        self.p.glass(r, radius::MD, &style, &sh);
        self.p.text_in(r, &text::LABEL_SM, &txt, t.ink, 0.5);
    }
}

impl Default for Ui {
    fn default() -> Self {
        Ui::new()
    }
}

fn button_metrics(size: ButtonSize) -> (tokens::TextStyle, f32) {
    match size {
        ButtonSize::Sm => (text::LABEL_SM, 12.0),
        ButtonSize::Md => (text::LABEL_MD, 16.0),
        ButtonSize::Lg => (tokens::TextStyle { face: crate::gfx::text::Face::Sans500, size: 16.0, line: 24.0, tracking: 0.0 }, 24.0),
    }
}

/// Высота кнопки для размера.
pub fn button_height(size: ButtonSize) -> f32 {
    match size {
        ButtonSize::Sm => 28.0,
        ButtonSize::Md => 36.0,
        ButtonSize::Lg => 44.0,
    }
}

/// Мелкие числовые помощники для `no_std`.
pub trait FloatExt {
    /// Модуль.
    fn abs_f(self) -> f32;
    /// Дробная часть.
    fn fract_f(self) -> f32;
}

impl FloatExt for f32 {
    fn abs_f(self) -> f32 {
        math::abs(self)
    }
    fn fract_f(self) -> f32 {
        self - math::floor(self)
    }
}

impl FloatExt for f64 {
    fn abs_f(self) -> f32 {
        math::abs(self as f32)
    }
    fn fract_f(self) -> f32 {
        (self - libm::floor(self)) as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::{KeyEvent, Modifiers};

    fn frame_input(mouse: Vec2, buttons: u8, time: f64) -> FrameInput {
        FrameInput { width: 400, height: 300, scale: 1.0, time, mouse: Some(mouse), buttons: MouseButtons(buttons), ..Default::default() }
    }

    #[test]
    fn button_click_cycle() {
        let mut ui = Ui::new();
        let r = Rect::new(10.0, 10.0, 100.0, 36.0);
        let id = id("btn");
        let mut clicked = false;
        for (i, b) in [0u8, 1, 1, 0].iter().enumerate() {
            ui.begin(&frame_input(Vec2::new(20.0, 20.0), *b, i as f64 * 0.01));
            ui.layer(1, ui.screen());
            clicked |= ui.button(id, r, "Установить", ButtonKind::Primary, ButtonSize::Md, None).clicked;
            ui.end();
        }
        assert!(clicked);
    }

    #[test]
    fn upper_layer_steals_hover() {
        let mut ui = Ui::new();
        let r = Rect::new(0.0, 0.0, 100.0, 100.0);
        for t in 0..2 {
            ui.begin(&frame_input(Vec2::new(50.0, 50.0), 0, t as f64));
            ui.layer(1, r);
            let low = ui.interact(id("low"), r);
            ui.layer(2, r);
            let high = ui.interact(id("high"), r);
            if t == 1 {
                assert!(!low.hovered);
                assert!(high.hovered);
            }
            ui.end();
        }
    }

    #[test]
    fn text_edit_selection_and_clipboard() {
        let mut e = TextEdit::new("hello");
        let mut clip = String::new();
        let mut i = FrameInput::default();
        let ctrl = Modifiers { ctrl: true, ..Default::default() };
        i.keys.push(KeyEvent { key: Key::Char('a'), down: true, mods: ctrl });
        i.keys.push(KeyEvent { key: Key::Char('x'), down: true, mods: ctrl });
        e.apply(&i, &mut clip);
        assert_eq!(e.text, "");
        assert_eq!(clip, "hello");
    }

    #[test]
    fn toggle_flips() {
        let mut ui = Ui::new();
        let mut on = false;
        let r = Rect::new(0.0, 0.0, 44.0, 26.0);
        for (i, b) in [0u8, 1, 0].iter().enumerate() {
            ui.begin(&frame_input(Vec2::new(10.0, 10.0), *b, i as f64));
            ui.layer(1, ui.screen());
            ui.toggle(id("t"), r, &mut on);
            ui.end();
        }
        assert!(on);
    }
}
