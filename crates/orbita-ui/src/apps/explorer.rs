//! Проводник (`Explorer`): боковая навигация, адресная строка в заголовке,
//! карточки файлов или список, панель сведений и индикатор диска.

extern crate alloc;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use super::{AppCx, AppKind, Request};
use crate::anim::Ease;
use crate::geom::{Radii, Rect, Vec2};
use crate::gfx::Paint;
use crate::icons::Icon;
use crate::input::Key;
use crate::math;
use crate::shell::{Notice, Status};
use crate::system::{DirEntry, SystemPort, format_size, join_path, parent_path};
use crate::tokens::{motion, radius, text};
use crate::ui::{ButtonKind, ButtonSize, CursorKind, MenuItem, id2};

/// Режим отображения.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum View {
    /// Карточки (`ob-tile`).
    Grid,
    /// Список (`ob-row` с шапкой `ob-thead`).
    List,
}

/// Состояние проводника.
pub struct ExplorerApp {
    /// Текущий путь.
    pub path: String,
    entries: Vec<DirEntry>,
    error: Option<String>,
    selected: Option<usize>,
    back: Vec<String>,
    forward: Vec<String>,
    view: View,
    scroll: f32,
    usage: (u64, u64),
    ctx_on_item: bool,
}

/// Файловая иконка по имени/типу (`fi-*`).
///
/// ```
/// use orbita_ui::{apps::explorer::file_icon, icons::Icon};
/// assert_eq!(file_icon("a.rs", false), Icon::FiCode);
/// assert_eq!(file_icon("x", true), Icon::FiFolder);
/// assert_eq!(file_icon("b.zip", false), Icon::FiZip);
/// ```
pub fn file_icon(name: &str, is_dir: bool) -> Icon {
    if is_dir {
        return Icon::FiFolder;
    }
    let ext = name.rsplit('.').next().unwrap_or("").to_lowercase();
    match ext.as_str() {
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" => Icon::FiImg,
        "zip" | "tar" | "gz" | "7z" | "orbpkg" | "img" => Icon::FiZip,
        "rs" | "sh" | "toml" | "json" | "js" | "py" | "c" | "h" | "css" | "html" => Icon::FiCode,
        _ => Icon::FiDoc,
    }
}

/// Описание типа файла для панели сведений.
fn kind_text(e: &DirEntry) -> &'static str {
    if e.is_dir {
        return "Папка";
    }
    match file_icon(&e.name, false) {
        Icon::FiImg => "Изображение",
        Icon::FiZip => "Архив",
        Icon::FiCode => "Исходный код",
        _ => "Документ",
    }
}

impl ExplorerApp {
    /// Открывает проводник в каталоге `path`.
    pub fn new(path: &str, sys: &mut dyn SystemPort) -> ExplorerApp {
        let mut e = ExplorerApp {
            path: String::from(path),
            entries: Vec::new(),
            error: None,
            selected: None,
            back: Vec::new(),
            forward: Vec::new(),
            view: View::Grid,
            scroll: 0.0,
            usage: (0, 0),
            ctx_on_item: false,
        };
        e.reload(sys);
        e
    }

    fn reload(&mut self, sys: &mut dyn SystemPort) {
        match sys.list_dir(&self.path) {
            Ok(v) => {
                self.entries = v;
                self.error = None;
            }
            Err(err) => {
                self.entries.clear();
                self.error = Some(err);
            }
        }
        self.usage = sys.disk_usage();
        if self.selected.map(|s| s >= self.entries.len()).unwrap_or(false) {
            self.selected = None;
        }
    }

    /// Переходит в каталог (с историей «назад»).
    pub fn go(&mut self, path: &str, sys: &mut dyn SystemPort) {
        if path == self.path {
            return;
        }
        self.back.push(core::mem::replace(&mut self.path, String::from(path)));
        self.forward.clear();
        self.selected = None;
        self.scroll = 0.0;
        self.reload(sys);
    }

    fn open_entry(&mut self, i: usize, cx: &mut AppCx) {
        let e = self.entries[i].clone();
        let full = join_path(&self.path, &e.name);
        if e.is_dir {
            self.go(&full, cx.sys);
        } else {
            cx.requests.push(Request::Open(AppKind::Editor, Some(full)));
        }
    }

    /// Кнопки навигации и «хлебные крошки» в строке заголовка окна.
    pub fn title_bar(&mut self, cx: &mut AppCx, area: Rect) {
        let t = cx.ui.theme;
        let mut x = area.x;
        let nav = [(Icon::IChevL, !self.back.is_empty()), (Icon::IChevR, !self.forward.is_empty())];
        for (n, (icon, enabled)) in nav.iter().enumerate() {
            let r = Rect::new(x, area.y + (area.h - 30.0) * 0.5, 30.0, 30.0);
            if !enabled {
                cx.ui.p.push_opacity(0.45);
            }
            let id = cx.id(if n == 0 { "nav-back" } else { "nav-fwd" });
            if cx.ui.icon_button(id, r, *icon, false).clicked && *enabled {
                if n == 0 {
                    if let Some(p) = self.back.pop() {
                        self.forward.push(core::mem::replace(&mut self.path, p));
                        self.selected = None;
                        self.reload(cx.sys);
                    }
                } else if let Some(p) = self.forward.pop() {
                    self.back.push(core::mem::replace(&mut self.path, p));
                    self.selected = None;
                    self.reload(cx.sys);
                }
            }
            if !enabled {
                cx.ui.p.pop_opacity();
            }
            x += 32.0;
        }
        // `ob-crumbs`.
        let cr = Rect::new(x + 4.0, area.y, area.right() - x - 4.0 - 72.0, area.h);
        cx.ui.p.rect(cr, radius::SM, Paint::Solid(t.stroke_subtle));
        let mut cxp = cr.x + 10.0;
        cx.ui.icon(Icon::IHome, Rect::new(cxp, cr.y + (cr.h - 14.0) * 0.5, 14.0, 14.0), t.ink_faint);
        cxp += 20.0;
        let parts: Vec<String> = self.path.split('/').filter(|s| !s.is_empty()).map(String::from).collect();
        let mut acc = String::new();
        let style = text::LABEL_13;
        cx.ui.push_clip(cr, radius::SM);
        let n = parts.len();
        let mut target = None;
        for (i, part) in parts.iter().enumerate() {
            acc.push('/');
            acc.push_str(part);
            let label = if part == "user" && i == 1 { "Домашняя" } else { part.as_str() };
            let w = cx.ui.p.measure(&style, label) + 10.0;
            let r = Rect::new(cxp, cr.y + 4.0, w, cr.h - 8.0);
            let id = id2(cx.id("crumb"), i as u64);
            let resp = cx.ui.interact(id, r);
            let h = cx.ui.hover_t(id, resp.hovered);
            cx.ui.p.rect(r, radius::XS, Paint::Solid(t.accent_soft.fade(h)));
            let c = if i + 1 == n { t.ink } else { t.ink_muted };
            cx.ui.p.text_in(r, &style, label, c, 0.5);
            if resp.clicked {
                target = Some(acc.clone());
            }
            cxp += w;
            if i + 1 < n {
                cx.ui.icon(Icon::IChevR, Rect::new(cxp, cr.y + (cr.h - 14.0) * 0.5, 14.0, 14.0), t.ink_faint);
                cxp += 16.0;
            }
        }
        cx.ui.pop_clip();
        if let Some(p) = target {
            self.go(&p, cx.sys);
        }
        // Переключатель вида.
        let vx = cr.right() + 6.0;
        for (n, (icon, v)) in [(Icon::IGrid, View::Grid), (Icon::IList, View::List)].iter().enumerate() {
            let r = Rect::new(vx + n as f32 * 32.0, area.y + (area.h - 30.0) * 0.5, 30.0, 30.0);
            let on = self.view == *v;
            if on {
                cx.ui.p.rect(r, radius::SM, Paint::Solid(t.accent_soft));
            }
            let id = id2(cx.id("view"), n as u64);
            if cx.ui.icon_button(id, r, *icon, false).clicked {
                self.view = *v;
            }
        }
    }

    /// Тело проводника.
    pub fn ui(&mut self, cx: &mut AppCx, body: Rect) {
        let t = cx.ui.theme;
        let (side, rest) = body.split_left(212.0);
        let (main_area, status) = rest.split_bottom(30.0);
        let info_w = if self.selected.is_some() && main_area.w > 640.0 { 232.0 } else { 0.0 };
        let (main_area, info) = main_area.split_right(info_w);

        // Боковая панель (`ob-window__side`, sunken 26%).
        cx.ui.p.rect(side, Radii { tl: 0.0, tr: 0.0, br: 0.0, bl: radius::XL }, Paint::Solid(t.surface_sunken.percent(0.26)));
        let mut y = side.y + 8.0;
        let sx = side.x + 12.0;
        cx.ui.p.text(Vec2::new(sx + 8.0, y + 4.0), &text::OVERLINE, "БЫСТРЫЙ ДОСТУП", t.ink_muted);
        y += 24.0;
        let places = [
            (Icon::IHome, "Домашняя", "/home/user"),
            (Icon::IFolder, "Проекты", "/home/user/Проекты"),
            (Icon::IDownload, "Загрузки", "/home/user/Загрузки"),
            (Icon::IImage, "Изображения", "/home/user/Изображения"),
            (Icon::IFile, "Документы", "/home/user/Документы"),
            (Icon::IDrive, "Корень", "/"),
        ];
        let mut go_to = None;
        for (i, (icon, label, path)) in places.iter().enumerate() {
            let r = Rect::new(sx, y, side.w - 24.0, 34.0);
            if cx.ui.nav_item(id2(cx.id("place"), i as u64), r, Some(*icon), label, self.path == *path).clicked {
                go_to = Some(*path);
            }
            y += 36.0;
        }
        y += 8.0;
        cx.ui.p.text(Vec2::new(sx + 8.0, y + 4.0), &text::OVERLINE, "ТЕГИ", t.ink_muted);
        y += 24.0;
        for (i, (label, c)) in [("Рабочие", t.accent_text), ("Архив", t.amb_teal)].iter().enumerate() {
            let r = Rect::new(sx, y, side.w - 24.0, 32.0);
            let id = id2(cx.id("tag"), i as u64);
            let resp = cx.ui.interact(id, r);
            let h = cx.ui.hover_t(id, resp.hovered);
            cx.ui.p.rect(r, radius::SM, Paint::Solid(t.accent_soft.fade(h)));
            cx.ui.p.rect(Rect::new(r.x + 16.0, r.y + 12.0, 8.0, 8.0), 4.0, Paint::Solid(*c));
            cx.ui.p.text_in(Rect::new(r.x + 42.0, r.y, r.w - 42.0, r.h), &text::LABEL_MD, label, t.ink, 0.0);
            y += 34.0;
        }
        // Индикатор диска внизу.
        let (used, total) = self.usage;
        let dy = side.bottom() - 62.0;
        if total > 0 {
            cx.ui.p.text(Vec2::new(sx + 8.0, dy), &text::BODY_SM, "Системный", t.ink_muted);
            let s = format!("занято {} из {}", format_size(used), format_size(total));
            cx.ui.p.text_in(Rect::new(sx + 8.0, dy + 18.0, side.w - 40.0, 18.0), &text::BODY_SM, &s, t.ink_muted, 0.0);
            let m = Rect::new(sx + 8.0, dy + 42.0, side.w - 40.0, 6.0);
            cx.ui.p.rect(m, 3.0, Paint::Solid(t.stroke_default));
            let frac = math::clamp(used as f32 / total as f32, 0.02, 1.0);
            cx.ui.p.rect(Rect::new(m.x, m.y, m.w * frac, 6.0), 3.0, Paint::linear(90.0, t.amb_blue, t.amb_cyan));
        }
        if let Some(p) = go_to {
            self.go(p, cx.sys);
        }

        // Клавиатура: стрелки, Enter, Backspace, Delete.
        if cx.focused && !cx.ui.keys_taken && cx.ui.popup.is_none() {
            let keys = cx.ui.input.keys.clone();
            for k in keys.iter().filter(|k| k.down) {
                let n = self.entries.len();
                match k.key {
                    Key::Right | Key::Down if n > 0 => self.selected = Some(self.selected.map(|s| (s + 1).min(n - 1)).unwrap_or(0)),
                    Key::Left | Key::Up if n > 0 => self.selected = Some(self.selected.map(|s| s.saturating_sub(1)).unwrap_or(0)),
                    Key::Enter => {
                        if let Some(s) = self.selected {
                            self.open_entry(s, cx);
                        }
                    }
                    Key::Backspace => {
                        let p = parent_path(&self.path);
                        self.go(&p, cx.sys);
                    }
                    Key::Delete => self.delete_selected(cx),
                    _ => {}
                }
            }
        }

        // Основная область.
        let inner = main_area.inset_xy(20.0, 12.0);
        if let Some(err) = self.error.clone() {
            cx.ui.p.text_in(inner, &text::BODY_MD, &format!("Каталог недоступен: {err}"), t.danger, 0.5);
        }
        let ctx_id = cx.id("ctx");
        let mut open_idx = None;
        let mut ctx_for: Option<Option<usize>> = None;
        match self.view {
            View::Grid => {
                let tile_w = 104.0;
                let tile_h = 112.0;
                let gap = 12.0;
                let cols = math::max(1.0, math::floor((inner.w + gap) / (tile_w + gap))) as usize;
                let rows = self.entries.len().div_ceil(cols);
                let content_h = rows as f32 * (tile_h + gap);
                let sid = cx.id("grid-scroll");
                let off = cx.ui.scroll(sid, inner, content_h, &mut self.scroll);
                cx.ui.push_clip(inner.inset(-4.0), 0.0);
                for (i, e) in self.entries.iter().enumerate() {
                    let col = i % cols;
                    let row = i / cols;
                    let r = Rect::new(inner.x + col as f32 * (tile_w + gap), inner.y + row as f32 * (tile_h + gap) - off, tile_w, tile_h);
                    if r.bottom() < inner.y - 10.0 || r.y > inner.bottom() + 10.0 {
                        continue;
                    }
                    let id = id2(cx.id("tile"), i as u64);
                    let resp = cx.ui.interact(id, r);
                    let h = cx.ui.hover_t(id, resp.hovered);
                    let sel = self.selected == Some(i);
                    let s = cx.ui.anim(id2(id, 1), if sel { 1.0 } else { 0.0 }, motion::FAST, Ease::Soft);
                    // `ob-tile` v2: карточка `stroke-subtle`, выбор — `accent-soft` + кольцо 1.5px.
                    let bg = t.stroke_subtle.mix(t.accent_soft, math::max(h, s));
                    cx.ui.p.rect_ex(r, Radii::all(radius::LG), Paint::Solid(bg), 1.5, t.accent_line.fade(s));
                    let ic = Rect::new(r.center().x - 24.0, r.y + 14.0, 48.0, 48.0);
                    cx.ui.icon(file_icon(&e.name, e.is_dir), ic, t.ink);
                    let st = if sel { text::BODY_SM.with_face(crate::gfx::text::Face::Sans600) } else { text::BODY_SM };
                    cx.ui.p.text_in(Rect::new(r.x + 6.0, ic.bottom() + 8.0, r.w - 12.0, 18.0), &st, &e.name, t.ink, 0.5);
                    let meta = if e.is_dir { String::from("папка") } else { format_size(e.size) };
                    cx.ui.p.text_in(Rect::new(r.x + 6.0, ic.bottom() + 26.0, r.w - 12.0, 16.0), &text::LABEL_SM, &meta, t.ink_faint, 0.5);
                    if resp.pressed {
                        self.selected = Some(i);
                        if resp.double_clicked {
                            open_idx = Some(i);
                        }
                    }
                    if resp.right_clicked {
                        self.selected = Some(i);
                        ctx_for = Some(Some(i));
                    }
                    if resp.hovered {
                        cx.ui.cursor = CursorKind::Hand;
                    }
                }
                cx.ui.pop_clip();
            }
            View::List => {
                // Шапка `ob-thead`.
                let head = Rect::new(inner.x, inner.y, inner.w, 28.0);
                let c2 = 120.0;
                let c3 = 90.0;
                cx.ui.p.text_in(Rect::new(head.x + 12.0, head.y, 200.0, 28.0), &text::OVERLINE, "ИМЯ", t.ink_muted, 0.0);
                cx.ui.p.text_in(Rect::new(head.right() - c3 - c2 - 12.0, head.y, c2, 28.0), &text::OVERLINE, "ТИП", t.ink_muted, 0.0);
                cx.ui.p.text_in(Rect::new(head.right() - c3 - 12.0, head.y, c3, 28.0), &text::OVERLINE, "РАЗМЕР", t.ink_muted, 1.0);
                let list = Rect::new(inner.x, inner.y + 32.0, inner.w, inner.h - 32.0);
                let content_h = self.entries.len() as f32 * 38.0;
                let sid = cx.id("list-scroll");
                let off = cx.ui.scroll(sid, list, content_h, &mut self.scroll);
                cx.ui.push_clip(list, 0.0);
                for (i, e) in self.entries.iter().enumerate() {
                    let r = Rect::new(list.x, list.y + i as f32 * 38.0 - off, list.w - 8.0, 36.0);
                    if r.bottom() < list.y || r.y > list.bottom() {
                        continue;
                    }
                    let id = id2(cx.id("row"), i as u64);
                    let resp = cx.ui.interact(id, r);
                    let h = cx.ui.hover_t(id, resp.hovered);
                    let sel = self.selected == Some(i);
                    let focus_sel = sel && cx.focused;
                    let bg = if focus_sel { t.accent } else { t.accent_soft.fade(math::max(h, if sel { 1.0 } else { 0.0 })) };
                    cx.ui.p.rect(r, if sel { radius::LG } else { radius::SM }, Paint::Solid(bg));
                    let fg = if focus_sel { t.on_accent } else { t.ink };
                    let mg = if focus_sel { t.on_accent } else { t.ink_muted };
                    cx.ui.icon(file_icon(&e.name, e.is_dir), Rect::new(r.x + 10.0, r.y + 6.0, 24.0, 24.0), fg);
                    cx.ui.p.text_in(Rect::new(r.x + 44.0, r.y, r.w - c2 - c3 - 70.0, r.h), &text::BODY_MD, &e.name, fg, 0.0);
                    cx.ui.p.text_in(Rect::new(r.right() - c3 - c2 - 4.0, r.y, c2, r.h), &text::BODY_SM, kind_text(e), mg, 0.0);
                    let size = if e.is_dir { String::from("—") } else { format_size(e.size) };
                    cx.ui.p.text_in(Rect::new(r.right() - c3 - 4.0, r.y, c3, r.h), &text::MONO_SM, &size, mg, 1.0);
                    if resp.pressed {
                        self.selected = Some(i);
                        if resp.double_clicked {
                            open_idx = Some(i);
                        }
                    }
                    if resp.right_clicked {
                        self.selected = Some(i);
                        ctx_for = Some(Some(i));
                    }
                }
                cx.ui.pop_clip();
            }
        }
        // Клик/правый клик по пустому месту.
        let bg_id = cx.id("bg");
        let bg = cx.ui.interact(bg_id, inner);
        if bg.pressed && cx.ui.hot == Some(bg_id) {
            self.selected = None;
        }
        if bg.right_clicked && ctx_for.is_none() {
            ctx_for = Some(None);
        }
        if let Some(target) = ctx_for {
            let items = match target {
                Some(_) => alloc::vec![
                    MenuItem::new("Открыть", Some(Icon::IFolder)).kbd("Enter"),
                    MenuItem::new("Открыть в редакторе", Some(Icon::ICode)),
                    MenuItem::new("Копировать путь", Some(Icon::ICopy)),
                    MenuItem::sep(),
                    MenuItem::new("Удалить", Some(Icon::ITrash)).kbd("Del").danger(),
                ],
                None => alloc::vec![
                    MenuItem::new("Новая папка", Some(Icon::IPlus)),
                    MenuItem::new("Новый файл", Some(Icon::IFile)),
                    MenuItem::new("Обновить", Some(Icon::IRestore)),
                    MenuItem::new("Открыть терминал здесь", Some(Icon::ITerminal)),
                ],
            };
            let m = cx.ui.mouse;
            cx.ui.open_popup(ctx_id, m, 230.0, items);
            self.ctx_on_item = target.is_some();
        }
        if let Some(choice) = cx.ui.take_popup_result(ctx_id) {
            self.context_action(choice, cx);
        }
        if let Some(i) = open_idx {
            self.open_entry(i, cx);
        }

        // Панель сведений.
        if info_w > 0.0 {
            if let Some(i) = self.selected {
                if let Some(e) = self.entries.get(i).cloned() {
                    let pane = info.inset_xy(0.0, 12.0).split_left(info.w - 16.0).0;
                    cx.ui.p.rect(pane, radius::LG, Paint::Solid(t.stroke_subtle));
                    let ic = Rect::new(pane.center().x - 36.0, pane.y + 24.0, 72.0, 72.0);
                    cx.ui.icon(file_icon(&e.name, e.is_dir), ic, t.ink);
                    cx.ui.p.text_in(Rect::new(pane.x + 12.0, ic.bottom() + 12.0, pane.w - 24.0, 24.0), &text::TITLE_SM, &e.name, t.ink, 0.5);
                    let mut yy = ic.bottom() + 48.0;
                    let rows = [
                        ("Тип", String::from(kind_text(&e))),
                        ("Размер", if e.is_dir { String::from("—") } else { format_size(e.size) }),
                        ("Путь", crate::apps::terminal::short_path(&join_path(&self.path, &e.name))),
                    ];
                    for (k, v) in rows.iter() {
                        cx.ui.p.text(Vec2::new(pane.x + 16.0, yy), &text::BODY_SM, k, t.ink_muted);
                        cx.ui.p.text_in(Rect::new(pane.x + 80.0, yy, pane.w - 96.0, 18.0), &text::MONO_SM, v, t.ink, 1.0);
                        yy += 26.0;
                    }
                    let bw = pane.w - 32.0;
                    let label = if e.is_dir { "Открыть" } else { "Открыть в редакторе" };
                    if cx.ui.button(cx.id("info-open"), Rect::new(pane.x + 16.0, yy + 8.0, bw, 36.0), label, ButtonKind::Primary, ButtonSize::Md, None).clicked {
                        self.open_entry(i, cx);
                    }
                }
            }
        }

        // Строка состояния (`ob-window__status`, моноширинная).
        let sel = match self.selected {
            Some(_) => "выбрано 1",
            None => "ничего не выбрано",
        };
        let s = format!("{} объектов · {}", self.entries.len(), sel);
        cx.ui.p.text_in(Rect::new(status.x + 16.0, status.y, status.w - 32.0, status.h), &text::MONO_SM, &s, t.ink_muted, 0.0);
        cx.ui.p.text_in(Rect::new(status.x + 16.0, status.y, status.w - 32.0, status.h), &text::MONO_SM, &self.path, t.ink_faint, 1.0);
    }

    fn delete_selected(&mut self, cx: &mut AppCx) {
        if let Some(i) = self.selected {
            let e = self.entries[i].clone();
            let full = join_path(&self.path, &e.name);
            match cx.sys.remove(&full) {
                Ok(()) => cx.notify(Notice::new("Удалено", &e.name, Status::Success)),
                Err(err) => cx.notify(Notice::new("Не удалось удалить", &err, Status::Danger)),
            }
            self.selected = None;
            self.reload(cx.sys);
        }
    }

    fn context_action(&mut self, choice: usize, cx: &mut AppCx) {
        if self.ctx_on_item {
            match choice {
                0 => {
                    if let Some(i) = self.selected {
                        self.open_entry(i, cx);
                    }
                }
                1 => {
                    if let Some(i) = self.selected {
                        let p = join_path(&self.path, &self.entries[i].name);
                        cx.requests.push(Request::Open(AppKind::Editor, Some(p)));
                    }
                }
                2 => {
                    if let Some(i) = self.selected {
                        cx.ui.clipboard = join_path(&self.path, &self.entries[i].name);
                    }
                }
                4 => self.delete_selected(cx),
                _ => {}
            }
        } else {
            match choice {
                0 | 1 => {
                    let base = if choice == 0 { "Новая папка" } else { "Новый файл.txt" };
                    let mut name = String::from(base);
                    let mut n = 2;
                    while self.entries.iter().any(|e| e.name == name) {
                        name = format!("{base} {n}");
                        n += 1;
                    }
                    let full = join_path(&self.path, &name);
                    let res = if choice == 0 { cx.sys.mkdir(&full) } else { cx.sys.write_file(&full, b"") };
                    if let Err(e) = res {
                        cx.notify(Notice::new("Ошибка", &e, Status::Danger));
                    }
                    self.reload(cx.sys);
                    self.selected = self.entries.iter().position(|e| e.name == name);
                }
                2 => self.reload(cx.sys),
                3 => {
                    let _ = cx.sys.exec(&format!("cd {}", self.path));
                    cx.requests.push(Request::Open(AppKind::Terminal, None));
                }
                _ => {}
            }
        }
    }
}
