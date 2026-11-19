//! Терминал (`ob-term`): настоящий shell Orbita OS в окне дизайн-системы.
//!
//! Команды выполняет [`SystemPort::exec`](crate::system::SystemPort::exec) —
//! тот же интерпретатор, что и в консоли ядра (пайпы, перенаправления,
//! переменные, `if/for/while`, `sh script.sh`, `pkg`, `ping`). Дополнительно
//! терминал понимает команды оболочки: `theme`, `wallpaper`, `accent`,
//! `blur`, `anim`, `fps`, `vsync`, `open <приложение>`, `notify <текст>`,
//! `exit`.

extern crate alloc;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use super::{AppCx, AppKind, Request};
use crate::color::Color;
use crate::geom::{Rect, Vec2};
use crate::gfx::Paint;
use crate::input::Key;
use crate::math;
use crate::shell::{Desktop, Notice, Status};
use crate::system::SystemPort;
use crate::tokens::{radius, text};
use crate::ui::{FloatExt, TextEdit};

/// Цвет строки вывода (ANSI-палитра дизайн-системы).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    /// Обычный текст `term-fg`.
    Normal,
    /// Приглушённый `term-dim`.
    Dim,
    /// Ошибка `term-red` (строка содержит `error`).
    Error,
    /// Успех `term-green` (`ok`/`done`).
    Ok,
    /// Предупреждение `term-yellow`.
    Warn,
    /// Строка-приглашение с командой.
    Prompt,
}

/// Строка буфера терминала.
#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    /// Текст.
    pub text: String,
    /// Тон.
    pub tone: Tone,
    /// Для приглашения: каталог на момент команды.
    pub cwd: String,
}

/// Классифицирует строку вывода по ключевым словам (статус всегда словом).
///
/// ```
/// use orbita_ui::apps::terminal::{classify, Tone};
/// assert_eq!(classify("error: not found"), Tone::Error);
/// assert_eq!(classify("build ok"), Tone::Ok);
/// assert_eq!(classify("plain"), Tone::Normal);
/// ```
pub fn classify(s: &str) -> Tone {
    let l = s.to_lowercase();
    if l.contains("error") || l.contains("ошибка") || l.contains("fail") || l.contains("not found") || l.contains("unknown") {
        Tone::Error
    } else if l.contains("warn") || l.contains("внимание") {
        Tone::Warn
    } else if l.contains(" ok") || l.starts_with("ok") || l.contains("done") || l.contains("готово") {
        Tone::Ok
    } else {
        Tone::Normal
    }
}

/// Состояние терминала.
pub struct TerminalApp {
    /// Буфер вывода.
    pub lines: Vec<Line>,
    /// Строка ввода.
    pub input: TextEdit,
    history: Vec<String>,
    hist_pos: Option<usize>,
    scroll: f32,
    follow: bool,
    cwd: String,
}

impl TerminalApp {
    /// Новый терминал с приветствием.
    pub fn new(sys: &mut dyn SystemPort) -> TerminalApp {
        let info = sys.info();
        let mut lines = Vec::new();
        let mut push = |t: &str, tone: Tone| lines.push(Line { text: String::from(t), tone, cwd: String::new() });
        push(&format!("{} — {}", info.os, info.machine), Tone::Dim);
        push("Введите help — список команд shell'а. Команды оболочки: theme, wallpaper, accent, blur, fps, vsync, open, notify.", Tone::Dim);
        push("", Tone::Normal);
        TerminalApp {
            lines,
            input: TextEdit::default(),
            history: Vec::new(),
            hist_pos: None,
            scroll: 0.0,
            follow: true,
            cwd: sys.cwd(),
        }
    }

    /// Заголовок окна: `user@orbita — ~/путь`.
    pub fn title(&self) -> String {
        format!("orbita — {}", short_path(&self.cwd))
    }

    /// Выполняет строку (вызывается по Enter; публично — для тестов и скриптов).
    pub fn run(&mut self, line: &str, cx_sys: &mut dyn SystemPort, settings: &mut crate::shell::Settings, requests: &mut Vec<Request>) {
        self.lines.push(Line { text: String::from(line), tone: Tone::Prompt, cwd: self.cwd.clone() });
        let trimmed = line.trim();
        if !trimmed.is_empty() {
            self.history.push(String::from(trimmed));
        }
        self.hist_pos = None;
        self.follow = true;
        if trimmed.is_empty() {
            return;
        }
        // Команды оболочки рабочего стола.
        if let Some(out) = Desktop::shell_command(settings, trimmed) {
            for l in out {
                self.lines.push(Line { tone: Tone::Ok, text: l, cwd: String::new() });
            }
            return;
        }
        let mut parts = trimmed.splitn(2, ' ');
        let cmd = parts.next().unwrap_or("");
        let rest = parts.next().unwrap_or("").trim();
        match cmd {
            "exit" => {
                requests.push(Request::CloseSelf);
                return;
            }
            "open" => {
                match AppKind::from_key(rest.split(' ').next().unwrap_or("")) {
                    Some(k) => {
                        let arg = rest.splitn(2, ' ').nth(1).map(String::from);
                        requests.push(Request::Open(k, arg));
                        self.lines.push(Line { text: format!("ok: открыто «{}»", k.title()), tone: Tone::Ok, cwd: String::new() });
                    }
                    None => self.lines.push(Line {
                        text: String::from("error: неизвестное приложение (files, terminal, editor, monitor, settings, about)"),
                        tone: Tone::Error,
                        cwd: String::new(),
                    }),
                }
                return;
            }
            "notify" => {
                let text = if rest.is_empty() { "Тестовое уведомление" } else { rest };
                requests.push(Request::Notify(Notice::new("Терминал", text, Status::Info)));
                return;
            }
            "edit" | "nano" | "vim" => {
                let path = if rest.starts_with('/') { String::from(rest) } else { crate::system::join_path(&self.cwd, rest) };
                requests.push(Request::Open(AppKind::Editor, Some(path)));
                return;
            }
            _ => {}
        }
        let out = cx_sys.exec(trimmed);
        if out.clear {
            self.lines.clear();
        }
        for l in out.lines {
            let tone = classify(&l);
            self.lines.push(Line { text: l, tone, cwd: String::new() });
        }
        if out.status != 0 && !out.clear {
            self.lines.push(Line { text: format!("exit {}", out.status), tone: Tone::Dim, cwd: String::new() });
        }
        self.cwd = cx_sys.cwd();
        if self.lines.len() > 2000 {
            let cut = self.lines.len() - 2000;
            self.lines.drain(..cut);
        }
    }

    /// Рисует терминал.
    pub fn ui(&mut self, cx: &mut AppCx, body: Rect) {
        let t = cx.ui.theme;
        let style = text::MONO_MD;
        // `.ob-term`: padding 8 20 20, без жёстких границ с окном.
        let area = Rect::new(body.x + 20.0, body.y + 4.0, body.w - 40.0, body.h - 24.0);
        let line_h = style.line;
        // Ввод.
        if cx.focused && !cx.ui.keys_taken && cx.ui.popup.is_none() {
            let input = cx.ui.input.clone();
            for k in &input.keys {
                if !k.down {
                    continue;
                }
                match k.key {
                    Key::Up if !self.history.is_empty() => {
                        let p = match self.hist_pos {
                            None => self.history.len() - 1,
                            Some(p) => p.saturating_sub(1),
                        };
                        self.hist_pos = Some(p);
                        self.input.set(&self.history[p]);
                    }
                    Key::Down => {
                        if let Some(p) = self.hist_pos {
                            if p + 1 < self.history.len() {
                                self.hist_pos = Some(p + 1);
                                self.input.set(&self.history[p + 1]);
                            } else {
                                self.hist_pos = None;
                                self.input.set("");
                            }
                        }
                    }
                    Key::Char('l') if k.mods.ctrl => self.lines.clear(),
                    Key::Char('c') if k.mods.ctrl && self.input.cursor == self.input.anchor => {
                        let l = format!("{}^C", self.input.text);
                        self.lines.push(Line { text: l, tone: Tone::Prompt, cwd: self.cwd.clone() });
                        self.input.set("");
                    }
                    Key::PageUp => {
                        self.scroll -= area.h * 0.8;
                        self.follow = false;
                    }
                    Key::PageDown => self.scroll += area.h * 0.8,
                    Key::Enter => {
                        let line = core::mem::take(&mut self.input.text);
                        self.input.set("");
                        let (sys, settings, reqs) = (&mut *cx.sys, &mut *cx.settings, &mut *cx.requests);
                        self.run(&line, sys, settings, reqs);
                    }
                    _ => {}
                }
            }
            let mut clip = core::mem::take(&mut cx.ui.clipboard);
            let mut filtered = input.clone();
            filtered.keys.retain(|k| !matches!(k.key, Key::Up | Key::Down | Key::Enter | Key::Char('l')));
            self.input.apply(&filtered, &mut clip);
            cx.ui.clipboard = clip;
            cx.ui.keys_taken = true;
        }

        // Раскладка: строки → визуальные строки с переносом по ширине.
        let char_w = cx.ui.p.measure(&style, "M");
        let cols = math::max(10.0, math::floor(area.w / char_w)) as usize;
        let prompt_text = prompt(&self.cwd);
        let mut rows: Vec<(usize, usize, usize)> = Vec::new(); // (line, start char, end char)
        for (i, l) in self.lines.iter().enumerate() {
            let full_len = l.text.chars().count() + if l.tone == Tone::Prompt { prompt(&l.cwd).chars().count() } else { 0 };
            if full_len == 0 {
                rows.push((i, 0, 0));
                continue;
            }
            let mut s = 0;
            while s < full_len {
                let e = (s + cols).min(full_len);
                rows.push((i, s, e));
                s = e;
            }
        }
        let input_len = prompt_text.chars().count() + self.input.text.chars().count();
        let input_rows = input_len / cols + 1;
        let content_h = (rows.len() + input_rows) as f32 * line_h;
        if self.follow {
            self.scroll = content_h;
        }
        let max = math::max(0.0, content_h - area.h);
        let before = self.scroll;
        let id = cx.id("term-scroll");
        let off = cx.ui.scroll(id, area, content_h, &mut self.scroll);
        if cx.ui.input.wheel.y < 0.0 {
            self.follow = false;
        }
        if self.scroll >= max - 1.0 {
            self.follow = true;
        }
        let _ = before;

        cx.ui.push_clip(Rect::new(area.x - 6.0, area.y - 2.0, area.w + 12.0, area.h + 4.0), 0.0);
        let first = (off / line_h) as usize;
        let visible = (area.h / line_h) as usize + 2;
        for (vi, (li, s, e)) in rows.iter().enumerate().skip(first).take(visible) {
            let y = area.y + vi as f32 * line_h - off;
            let l = &self.lines[*li];
            if l.tone == Tone::Prompt {
                let p = prompt(&l.cwd);
                let full: String = p.chars().chain(l.text.chars()).collect();
                draw_prompt_row(cx, &full, &p, *s, *e, Vec2::new(area.x, y));
            } else {
                let seg: String = l.text.chars().skip(*s).take(e - s).collect();
                let c = match l.tone {
                    Tone::Normal | Tone::Prompt => t.term_fg,
                    Tone::Dim => t.term_dim,
                    Tone::Error => t.term_red,
                    Tone::Ok => t.term_green,
                    Tone::Warn => t.term_yellow,
                };
                cx.ui.p.text(Vec2::new(area.x, y), &style, &seg, c);
            }
        }
        // Строка ввода с приглашением и курсором-блоком.
        let full: String = prompt_text.chars().chain(self.input.text.chars()).collect();
        let base_row = rows.len();
        let plen = prompt_text.chars().count();
        for r in 0..input_rows {
            let y = area.y + (base_row + r) as f32 * line_h - off;
            let s = r * cols;
            let e = ((r + 1) * cols).min(full.chars().count());
            if s <= e {
                draw_prompt_row(cx, &full, &prompt_text, s, e, Vec2::new(area.x, y));
            }
        }
        let cur = plen + self.input.cursor;
        let cy = area.y + (base_row + cur / cols) as f32 * line_h - off;
        let cx_ = area.x + (cur % cols) as f32 * char_w;
        let (a, b) = (self.input.cursor.min(self.input.anchor), self.input.cursor.max(self.input.anchor));
        if a != b {
            let sx = area.x + ((plen + a) % cols) as f32 * char_w;
            cx.ui.p.rect(Rect::new(sx, cy, (b - a) as f32 * char_w, line_h), 2.0, Paint::Solid(t.term_selection));
        }
        if cx.focused {
            let blink = (cx.ui.now / 1.1).fract_f() < 0.56 || !cx.ui.look.animations;
            cx.ui.anims.mark_active();
            if blink {
                cx.ui.p.rect(Rect::new(cx_, cy + 3.0, char_w, line_h - 5.0), 1.0, Paint::Solid(t.term_cursor));
                if let Some(ch) = self.input.text.chars().nth(self.input.cursor) {
                    let mut b = [0u8; 4];
                    cx.ui.p.text(Vec2::new(cx_, cy), &style, ch.encode_utf8(&mut b), t.term_bg);
                }
            }
        } else {
            cx.ui.p.stroke_rect(Rect::new(cx_, cy + 3.0, char_w, line_h - 5.0), 1.0, 1.5, t.term_cursor);
        }
        cx.ui.pop_clip();
        let _ = radius::XS;
        let _ = Color::WHITE;
    }
}

/// Приглашение `orbita@node:~/путь $ `.
fn prompt(cwd: &str) -> String {
    format!("orbita@node:{} $ ", short_path(cwd))
}

/// Сокращает домашний каталог до `~`.
///
/// ```
/// assert_eq!(orbita_ui::apps::terminal::short_path("/home/user/Проекты"), "~/Проекты");
/// assert_eq!(orbita_ui::apps::terminal::short_path("/etc"), "/etc");
/// ```
pub fn short_path(p: &str) -> String {
    if let Some(rest) = p.strip_prefix("/home/user") {
        format!("~{rest}")
    } else {
        String::from(p)
    }
}

/// Рисует фрагмент `[s, e)` строки «приглашение + команда» с подсветкой:
/// пользователь — зелёный, путь — синий, `$` — приглушённый.
fn draw_prompt_row(cx: &mut AppCx, full: &str, prompt: &str, s: usize, e: usize, pos: Vec2) {
    let t = cx.ui.theme;
    let style = text::MONO_MD;
    let char_w = cx.ui.p.measure(&style, "M");
    let user_end = prompt.find(':').map(|b| prompt[..b].chars().count()).unwrap_or(0);
    let path_end = prompt.chars().count().saturating_sub(3);
    let plen = prompt.chars().count();
    let spans = [
        (0, user_end, t.term_green),
        (user_end, user_end + 1, t.term_dim),
        (user_end + 1, path_end, t.term_blue),
        (path_end, plen, t.term_dim),
        (plen, usize::MAX, t.term_fg),
    ];
    for (a, b, c) in spans {
        let a2 = a.max(s);
        let b2 = b.min(e);
        if a2 >= b2 {
            continue;
        }
        let seg: String = full.chars().skip(a2).take(b2 - a2).collect();
        cx.ui.p.text(Vec2::new(pos.x + (a2 - s) as f32 * char_w, pos.y), &style, &seg, c);
    }
}

#[cfg(all(test, feature = "system"))]
mod tests {
    use super::*;
    use crate::shell::Settings;
    use crate::system::ShellPort;

    #[test]
    fn runs_real_shell_commands() {
        let mut sys = ShellPort::demo("test", "gpu");
        let mut term = TerminalApp::new(&mut sys);
        let mut settings = Settings::default();
        let mut reqs = Vec::new();
        term.run("echo один | cat", &mut sys, &mut settings, &mut reqs);
        assert!(term.lines.iter().any(|l| l.text == "один"));
        term.run("theme light", &mut sys, &mut settings, &mut reqs);
        assert_eq!(settings.mode, crate::tokens::ThemeMode::Light);
        term.run("open files", &mut sys, &mut settings, &mut reqs);
        assert!(matches!(reqs.last(), Some(Request::Open(AppKind::Files, _))));
    }
}
