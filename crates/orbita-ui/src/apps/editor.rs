//! Редактор: многострочный текст с номерами строк и сохранением `Ctrl+S`.

extern crate alloc;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use super::AppCx;
use crate::geom::{Radii, Rect, Vec2};
use crate::gfx::Paint;
use crate::input::Key;
use crate::math;
use crate::shell::{Notice, Status};
use crate::system::SystemPort;
use crate::tokens::{radius, text};
use crate::ui::{ButtonKind, ButtonSize, CursorKind, FloatExt};

/// Состояние редактора.
pub struct EditorApp {
    /// Путь файла (`None` — новый документ).
    pub path: Option<String>,
    /// Строки текста.
    pub lines: Vec<String>,
    /// Курсор: (строка, символ).
    pub cur: (usize, usize),
    dirty: bool,
    scroll: f32,
    message: Option<String>,
}

impl EditorApp {
    /// Открывает файл (или пустой документ).
    pub fn new(path: Option<&str>, sys: &mut dyn SystemPort) -> EditorApp {
        let mut message = None;
        let text = match path {
            Some(p) => match sys.read_file(p) {
                Ok(b) => String::from_utf8_lossy(&b).into_owned(),
                Err(e) => {
                    message = Some(format!("новый файл ({e})"));
                    String::new()
                }
            },
            None => String::new(),
        };
        let mut lines: Vec<String> = text.split('\n').map(String::from).collect();
        if lines.is_empty() {
            lines.push(String::new());
        }
        EditorApp { path: path.map(String::from), lines, cur: (0, 0), dirty: false, scroll: 0.0, message }
    }

    /// Заголовок окна: имя файла и признак изменений.
    pub fn title(&self) -> String {
        let name = self.path.as_deref().and_then(|p| p.rsplit('/').next()).unwrap_or("Без имени");
        format!("{}{} — Редактор", name, if self.dirty { " •" } else { "" })
    }

    fn line_len(&self, l: usize) -> usize {
        self.lines[l].chars().count()
    }

    fn byte(&self, l: usize, c: usize) -> usize {
        self.lines[l].char_indices().nth(c).map(|(i, _)| i).unwrap_or(self.lines[l].len())
    }

    /// Вставляет текст в позицию курсора (поддерживает `\n`).
    ///
    /// ```
    /// use orbita_ui::{apps::editor::EditorApp, system::NullSystem};
    /// let mut e = EditorApp::new(None, &mut NullSystem);
    /// e.insert("ab\ncd");
    /// assert_eq!(e.lines, vec!["ab".to_string(), "cd".to_string()]);
    /// assert_eq!(e.cur, (1, 2));
    /// ```
    pub fn insert(&mut self, s: &str) {
        for ch in s.chars() {
            if ch == '\n' || ch == '\r' {
                let (l, c) = self.cur;
                let b = self.byte(l, c);
                let tail = self.lines[l].split_off(b);
                self.lines.insert(l + 1, tail);
                self.cur = (l + 1, 0);
            } else if !ch.is_control() || ch == '\t' {
                let (l, c) = self.cur;
                let b = self.byte(l, c);
                if ch == '\t' {
                    self.lines[l].insert_str(b, "    ");
                    self.cur.1 += 4;
                } else {
                    self.lines[l].insert(b, ch);
                    self.cur.1 += 1;
                }
            }
        }
        self.dirty = true;
    }

    fn backspace(&mut self) {
        let (l, c) = self.cur;
        if c > 0 {
            let b0 = self.byte(l, c - 1);
            let b1 = self.byte(l, c);
            self.lines[l].replace_range(b0..b1, "");
            self.cur.1 -= 1;
        } else if l > 0 {
            let line = self.lines.remove(l);
            let len = self.line_len(l - 1);
            self.lines[l - 1].push_str(&line);
            self.cur = (l - 1, len);
        }
        self.dirty = true;
    }

    fn delete(&mut self) {
        let (l, c) = self.cur;
        if c < self.line_len(l) {
            let b0 = self.byte(l, c);
            let b1 = self.byte(l, c + 1);
            self.lines[l].replace_range(b0..b1, "");
        } else if l + 1 < self.lines.len() {
            let next = self.lines.remove(l + 1);
            self.lines[l].push_str(&next);
        }
        self.dirty = true;
    }

    fn save(&mut self, cx: &mut AppCx) {
        let path = match &self.path {
            Some(p) => p.clone(),
            None => {
                let p = String::from("/home/user/Документы/без имени.txt");
                self.path = Some(p.clone());
                p
            }
        };
        let text = self.lines.join("\n");
        match cx.sys.write_file(&path, text.as_bytes()) {
            Ok(()) => {
                self.dirty = false;
                self.message = Some(String::from("сохранено"));
                cx.notify(Notice::new("Файл сохранён", &path, Status::Success));
            }
            Err(e) => {
                self.message = Some(format!("error: {e}"));
                cx.notify(Notice::new("Не удалось сохранить", &e, Status::Danger));
            }
        }
    }

    /// Рисует редактор.
    pub fn ui(&mut self, cx: &mut AppCx, body: Rect) {
        let t = cx.ui.theme;
        let style = text::MONO_MD;
        let (main, status) = body.split_bottom(30.0);
        // Панель инструментов.
        let tb = Rect::new(main.x + 16.0, main.y + 2.0, main.w - 32.0, 36.0);
        let bw = cx.ui.button_width("Сохранить", ButtonSize::Sm, false);
        if cx.ui.button(cx.id("save"), Rect::new(tb.right() - bw, tb.y + 4.0, bw, 28.0), "Сохранить", ButtonKind::Primary, ButtonSize::Sm, None).clicked {
            self.save(cx);
        }
        let path = self.path.clone().unwrap_or_else(|| String::from("новый документ"));
        cx.ui.p.text_in(Rect::new(tb.x, tb.y, tb.w - bw - 16.0, tb.h), &text::MONO_SM, &crate::apps::terminal::short_path(&path), t.ink_muted, 0.0);

        let area = Rect::new(main.x + 12.0, tb.bottom() + 6.0, main.w - 24.0, main.bottom() - tb.bottom() - 12.0);
        cx.ui.p.rect(area, radius::LG, Paint::Solid(t.surface_sunken.percent(0.5)));
        let gutter = 52.0;
        let text_area = Rect::new(area.x + gutter, area.y + 8.0, area.w - gutter - 12.0, area.h - 16.0);
        let line_h = style.line;
        let char_w = cx.ui.p.measure(&style, "M");

        // Клавиатура.
        if cx.focused && !cx.ui.keys_taken && cx.ui.popup.is_none() {
            let input = cx.ui.input.clone();
            for k in input.keys.iter().filter(|k| k.down) {
                let (l, c) = self.cur;
                match k.key {
                    Key::Char('s') if k.mods.ctrl => self.save(cx),
                    Key::Enter => self.insert("\n"),
                    Key::Tab => self.insert("\t"),
                    Key::Backspace => self.backspace(),
                    Key::Delete => self.delete(),
                    Key::Left => {
                        if c > 0 {
                            self.cur.1 -= 1;
                        } else if l > 0 {
                            self.cur = (l - 1, self.line_len(l - 1));
                        }
                    }
                    Key::Right => {
                        if c < self.line_len(l) {
                            self.cur.1 += 1;
                        } else if l + 1 < self.lines.len() {
                            self.cur = (l + 1, 0);
                        }
                    }
                    Key::Up if l > 0 => self.cur = (l - 1, c.min(self.line_len(l - 1))),
                    Key::Down if l + 1 < self.lines.len() => self.cur = (l + 1, c.min(self.line_len(l + 1))),
                    Key::Home => self.cur.1 = 0,
                    Key::End => self.cur.1 = self.line_len(l),
                    Key::Char('v') if k.mods.ctrl => {
                        let clip = cx.ui.clipboard.clone();
                        self.insert(&clip);
                    }
                    Key::Char('c') if k.mods.ctrl => {
                        cx.ui.clipboard = self.lines[l].clone();
                    }
                    _ => {}
                }
            }
            if !input.text.is_empty() && !input.mods.ctrl {
                let typed: String = input.text.chars().filter(|c| !c.is_control()).collect();
                if !typed.is_empty() {
                    self.insert(&typed);
                }
            }
            cx.ui.keys_taken = true;
        }

        // Клик ставит курсор.
        let tid = cx.id("text");
        let resp = cx.ui.interact(tid, area);
        if resp.hovered {
            cx.ui.cursor = CursorKind::Text;
        }
        let content_h = self.lines.len() as f32 * line_h + 8.0;
        let sid = cx.id("scroll");
        // Прокрутка к курсору.
        let cy = self.cur.0 as f32 * line_h;
        if cy < self.scroll {
            self.scroll = cy;
        } else if cy + line_h > self.scroll + text_area.h {
            self.scroll = cy + line_h - text_area.h;
        }
        let off = cx.ui.scroll(sid, text_area, content_h, &mut self.scroll);
        if resp.pressed {
            let m = cx.ui.mouse;
            let l = math::clamp(((m.y - text_area.y + off) / line_h) as f32, 0.0, (self.lines.len() - 1) as f32) as usize;
            let c = math::max(0.0, math::round((m.x - text_area.x) / char_w)) as usize;
            self.cur = (l, c.min(self.line_len(l)));
        }

        cx.ui.push_clip(area, radius::LG);
        let first = (off / line_h) as usize;
        let visible = (text_area.h / line_h) as usize + 2;
        for (i, line) in self.lines.iter().enumerate().skip(first).take(visible) {
            let y = text_area.y + i as f32 * line_h - off;
            let is_cur = i == self.cur.0;
            if is_cur && cx.focused {
                cx.ui.p.rect(Rect::new(area.x, y, area.w, line_h), Radii::ZERO, Paint::Solid(t.accent_soft.percent(0.45)));
            }
            let num = format!("{}", i + 1);
            let nc = if is_cur { t.ink } else { t.ink_faint };
            cx.ui.p.text_in(Rect::new(area.x, y, gutter - 14.0, line_h), &text::MONO_SM, &num, nc, 1.0);
            cx.ui.p.text(Vec2::new(text_area.x, y), &style, line, t.ink);
        }
        if cx.focused {
            let blink = (cx.ui.now / 1.1).fract_f() < 0.56 || !cx.ui.look.animations;
            cx.ui.anims.mark_active();
            if blink {
                let x = text_area.x + self.cur.1 as f32 * char_w;
                let y = text_area.y + self.cur.0 as f32 * line_h - off;
                cx.ui.p.rect(Rect::new(x, y + 2.0, 2.0, line_h - 4.0), 1.0, Paint::Solid(t.accent_text));
            }
        }
        cx.ui.pop_clip();

        // Строка состояния.
        let st = format!(
            "строка {}, столбец {} · {} строк · UTF-8{}",
            self.cur.0 + 1,
            self.cur.1 + 1,
            self.lines.len(),
            match &self.message {
                Some(m) => format!(" · {m}"),
                None => String::new(),
            }
        );
        cx.ui.p.text_in(Rect::new(status.x + 16.0, status.y, status.w - 32.0, status.h), &text::MONO_SM, &st, t.ink_muted, 0.0);
    }
}
