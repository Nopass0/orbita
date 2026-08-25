//! Boot console model, terminal state machine, and PS/2 keyboard decoding.


extern crate alloc;

use alloc::vec::Vec;
use core::fmt::Write;
use orbita_std::String;

pub(crate) struct BootConsole {
    pub(crate) lines: Vec<String>,
    pub(crate) input: String,
    pub(crate) status: String,
    pub(crate) cwd: String,
    pub(crate) hostname: String,
    pub(crate) cursor_visible: bool,
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub(crate) enum RedrawKind {
    None,
    PromptOnly,
    Full,
}

impl BootConsole {
    pub(crate) fn new() -> Self {
        Self {
            lines: Vec::new(),
            input: String::new(),
            status: String::from("ready"),
            cwd: String::from("/"),
            hostname: String::from("orbita"),
            cursor_visible: true,
        }
    }

    pub(crate) fn clear(&mut self) {
        self.lines.clear();
    }

    pub(crate) fn push_line(&mut self, line: &str) {
        self.lines.push(String::from(line));
        self.trim();
    }

    pub(crate) fn push_line_fmt(&mut self, args: core::fmt::Arguments<'_>) {
        let mut line = String::new();
        let _ = line.write_fmt(args);
        self.lines.push(line);
        self.trim();
    }

    pub(crate) fn set_status(&mut self, status: &str) {
        self.status.clear();
        self.status.push_str(status);
    }

    pub(crate) fn render_history(&self, max_lines: usize) -> String {
        let mut out = String::new();
        let start = self.lines.len().saturating_sub(max_lines);
        for line in &self.lines[start..] {
            let _ = writeln!(&mut out, "{line}");
        }
        out
    }

    pub(crate) fn trim(&mut self) {
        const MAX_LINES: usize = 256;
        if self.lines.len() > MAX_LINES {
            let remove = self.lines.len() - MAX_LINES;
            self.lines.drain(0..remove);
        }
    }

    pub(crate) fn prompt_text(&self) -> String {
        let mut prompt = String::new();
        let _ = write!(&mut prompt, "{}:{}# ", self.hostname, self.cwd);
        prompt.push_str(&self.input);
        prompt
    }

    #[allow(dead_code)]
    const fn line_height(&self) -> usize {
        10
    }

    #[allow(dead_code)]
    pub(crate) fn visible_line_count(&self, max_lines: usize) -> usize {
        core::cmp::min(self.lines.len(), max_lines)
    }
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub(crate) enum KeyAction {
    Char(char),
    Backspace,
    Enter,
    NextApp,
    LaunchApp(usize),
    PointerMove(isize, isize),
    PointerActivate,
}

pub(crate) struct Ps2KeyboardDecoder {
    extended: bool,
    shift: bool,
}

impl Ps2KeyboardDecoder {
    pub(crate) const fn new() -> Self {
        Self {
            extended: false,
            shift: false,
        }
    }

    pub(crate) fn feed(&mut self, scancode: u8) -> Option<KeyAction> {
        if scancode == 0xE0 {
            self.extended = true;
            return None;
        }

        let released = scancode & 0x80 != 0;
        let code = scancode & 0x7F;

        match code {
            0x2A | 0x36 => {
                self.shift = !released;
                self.extended = false;
                return None;
            }
            _ => {}
        }

        if released {
            self.extended = false;
            return None;
        }

        let action = if self.extended {
            match code {
                0x48 => Some(KeyAction::PointerMove(0, -18)),
                0x50 => Some(KeyAction::PointerMove(0, 18)),
                0x4B => Some(KeyAction::PointerMove(-18, 0)),
                0x4D => Some(KeyAction::PointerMove(18, 0)),
                _ => None,
            }
        } else {
            match code {
            0x0E => Some(KeyAction::Backspace),
            0x0F => Some(KeyAction::NextApp),
            0x1C => Some(KeyAction::Enter),
            0x38 => Some(KeyAction::PointerActivate),
            0x3B => Some(KeyAction::LaunchApp(0)),
            0x3C => Some(KeyAction::LaunchApp(1)),
            0x3D => Some(KeyAction::LaunchApp(2)),
            0x3E => Some(KeyAction::LaunchApp(3)),
            _ => map_scancode_set1(code, self.shift).map(KeyAction::Char),
            }
        };
        self.extended = false;
        action
    }
}

pub(crate) fn map_scancode_set1(code: u8, shift: bool) -> Option<char> {
    let shifted_digit = match code {
        0x02 => '!',
        0x03 => '@',
        0x04 => '#',
        0x05 => '$',
        0x06 => '%',
        0x07 => '^',
        0x08 => '&',
        0x09 => '*',
        0x0A => '(',
        0x0B => ')',
        _ => '\0',
    };

    let ch = match code {
        0x02..=0x0B if shift => shifted_digit,
        0x02 => '1',
        0x03 => '2',
        0x04 => '3',
        0x05 => '4',
        0x06 => '5',
        0x07 => '6',
        0x08 => '7',
        0x09 => '8',
        0x0A => '9',
        0x0B => '0',
        0x0C => if shift { '_' } else { '-' },
        0x0D => if shift { '+' } else { '=' },
        0x10 => if shift { 'Q' } else { 'q' },
        0x11 => if shift { 'W' } else { 'w' },
        0x12 => if shift { 'E' } else { 'e' },
        0x13 => if shift { 'R' } else { 'r' },
        0x14 => if shift { 'T' } else { 't' },
        0x15 => if shift { 'Y' } else { 'y' },
        0x16 => if shift { 'U' } else { 'u' },
        0x17 => if shift { 'I' } else { 'i' },
        0x18 => if shift { 'O' } else { 'o' },
        0x19 => if shift { 'P' } else { 'p' },
        0x1A => if shift { '{' } else { '[' },
        0x1B => if shift { '}' } else { ']' },
        0x1E => if shift { 'A' } else { 'a' },
        0x1F => if shift { 'S' } else { 's' },
        0x20 => if shift { 'D' } else { 'd' },
        0x21 => if shift { 'F' } else { 'f' },
        0x22 => if shift { 'G' } else { 'g' },
        0x23 => if shift { 'H' } else { 'h' },
        0x24 => if shift { 'J' } else { 'j' },
        0x25 => if shift { 'K' } else { 'k' },
        0x26 => if shift { 'L' } else { 'l' },
        0x27 => if shift { ':' } else { ';' },
        0x28 => if shift { '"' } else { '\'' },
        0x29 => if shift { '~' } else { '`' },
        0x2B => if shift { '|' } else { '\\' },
        0x2C => if shift { 'Z' } else { 'z' },
        0x2D => if shift { 'X' } else { 'x' },
        0x2E => if shift { 'C' } else { 'c' },
        0x2F => if shift { 'V' } else { 'v' },
        0x30 => if shift { 'B' } else { 'b' },
        0x31 => if shift { 'N' } else { 'n' },
        0x32 => if shift { 'M' } else { 'm' },
        0x33 => if shift { '<' } else { ',' },
        0x34 => if shift { '>' } else { '.' },
        0x35 => if shift { '?' } else { '/' },
        0x39 => ' ',
        _ => return None,
    };
    Some(ch)
}
