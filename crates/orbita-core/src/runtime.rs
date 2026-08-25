extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

use crate::builtin_apps;

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum DesktopFocusTarget {
    Desktop,
    MainWindow,
    SystemWindow,
    PreviewWindow,
    DockStart,
    DockSearch,
    DockTray,
    DockApp(usize),
}

impl DesktopFocusTarget {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Desktop => "desktop",
            Self::MainWindow => "main-window",
            Self::SystemWindow => "system-window",
            Self::PreviewWindow => "preview-window",
            Self::DockStart => "dock-start",
            Self::DockSearch => "dock-search",
            Self::DockTray => "dock-tray",
            Self::DockApp(_) => "dock-app",
        }
    }
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum DesktopChromePanel {
    None,
    Start,
    Search,
    Tray,
}

impl DesktopChromePanel {
    pub const fn label(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Start => "start",
            Self::Search => "search",
            Self::Tray => "tray",
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct DesktopChromeState {
    panel: DesktopChromePanel,
    selection: usize,
    query: String,
}

impl DesktopChromeState {
    pub fn new() -> Self {
        Self {
            panel: DesktopChromePanel::None,
            selection: 0,
            query: String::new(),
        }
    }

    pub const fn active_panel(&self) -> DesktopChromePanel {
        self.panel
    }

    pub fn activate(&mut self, panel: DesktopChromePanel) {
        if self.panel != panel {
            self.selection = 0;
            self.query.clear();
        }
        self.panel = panel;
    }

    pub fn clear(&mut self) {
        self.panel = DesktopChromePanel::None;
        self.selection = 0;
        self.query.clear();
    }

    pub const fn selection(&self) -> usize {
        self.selection
    }

    pub fn cycle_selection(&mut self, count: usize) {
        if count > 0 {
            self.selection = (self.selection + 1) % count;
        } else {
            self.selection = 0;
        }
    }

    pub fn push_query(&mut self, ch: char) {
        if self.query.len() < 32 {
            self.query.push(ch);
            self.selection = 0;
        }
    }

    pub fn pop_query(&mut self) {
        self.query.pop();
        self.selection = 0;
    }

    pub fn query(&self) -> &str {
        self.query.as_str()
    }

    pub fn selected_app_index(&self) -> Option<usize> {
        match self.panel {
            DesktopChromePanel::Start => {
                let mut matches_seen = 0usize;
                let needle = self.query.as_str();
                for (index, app) in builtin_apps().iter().enumerate() {
                    let is_match = needle.is_empty()
                        || app.id.contains(needle)
                        || app.name.to_ascii_lowercase().contains(&needle.to_ascii_lowercase());
                    if is_match {
                        if matches_seen == self.selection {
                            return Some(index);
                        }
                        matches_seen += 1;
                    }
                }
                None
            }
            DesktopChromePanel::Search => {
                let mut matches_seen = 0usize;
                let needle = self.query.as_str();
                for (index, app) in builtin_apps().iter().enumerate() {
                    let is_match = needle.is_empty()
                        || app.id.contains(needle)
                        || app.name.to_ascii_lowercase().contains(&needle.to_ascii_lowercase());
                    if is_match {
                        if matches_seen == self.selection {
                            return Some(index);
                        }
                        matches_seen += 1;
                    }
                }
                None
            }
            DesktopChromePanel::None | DesktopChromePanel::Tray => None,
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct DesktopSessionState<'a> {
    pub frame_counter: u32,
    pub active_app: &'a str,
    pub status: &'a str,
    pub graphics_backend: &'a str,
    pub graphics_api: &'a str,
    pub present_mode: &'a str,
    pub frames_in_flight: usize,
    pub cursor_visible: bool,
    pub pointer_x: usize,
    pub pointer_y: usize,
    pub focus_target: &'a str,
    pub chrome_panel: &'a str,
    pub chrome_query: &'a str,
    pub chrome_selection: usize,
    pub files_selection: usize,
    pub settings_section: &'a str,
}

impl<'a> DesktopSessionState<'a> {
    pub fn manifest(&self) -> String {
        alloc::format!(
            "frame={}\nactive_app={}\nstatus={}\ngraphics_backend={}\ngraphics_api={}\npresent_mode={}\nframes_in_flight={}\ncursor_visible={}\npointer_x={}\npointer_y={}\nfocus_target={}\nchrome_panel={}\nchrome_query={}\nchrome_selection={}\nfiles_selection={}\nsettings_section={}\n",
            self.frame_counter,
            self.active_app,
            self.status,
            self.graphics_backend,
            self.graphics_api,
            self.present_mode,
            self.frames_in_flight,
            self.cursor_visible,
            self.pointer_x,
            self.pointer_y,
            self.focus_target,
            self.chrome_panel,
            self.chrome_query,
            self.chrome_selection,
            self.files_selection,
            self.settings_section
        )
    }
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum SettingsSection {
    Toolchains,
    Network,
    Services,
}

impl SettingsSection {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Toolchains => "toolchains",
            Self::Network => "network",
            Self::Services => "services",
        }
    }

    pub const fn next(self) -> Self {
        match self {
            Self::Toolchains => Self::Network,
            Self::Network => Self::Services,
            Self::Services => Self::Toolchains,
        }
    }
}

/// User-selectable graphics backend preference.
///
/// The string returned by [`GraphicsBackend::label`] is the registry key
/// into `orbita_video::create_backend`; an engine that is not registered
/// by a driver falls back to the software framebuffer at creation time.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum GraphicsBackend {
    SoftwareFramebuffer,
    Vulkan,
}

impl GraphicsBackend {
    /// Registry name of the backend (matches `orbita_video` registry keys).
    pub const fn label(self) -> &'static str {
        match self {
            Self::SoftwareFramebuffer => "software-framebuffer",
            Self::Vulkan => "vulkan",
        }
    }

    pub const fn next(self) -> Self {
        match self {
            Self::SoftwareFramebuffer => Self::Vulkan,
            Self::Vulkan => Self::SoftwareFramebuffer,
        }
    }

    /// Presentation style advertised when this backend is available.
    pub const fn present_mode(self) -> &'static str {
        match self {
            Self::SoftwareFramebuffer => "double-buffered-dirty-rect",
            Self::Vulkan => "mailbox",
        }
    }

    /// Rendering API advertised when this backend is available.
    pub const fn api(self) -> &'static str {
        match self {
            Self::SoftwareFramebuffer => "cpu-raster",
            Self::Vulkan => "vulkan",
        }
    }
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct DesktopWorkspaceState {
    files_selection: usize,
    settings_section: SettingsSection,
    file_action_counter: usize,
    graphics_backend: GraphicsBackend,
}

impl DesktopWorkspaceState {
    pub const fn new() -> Self {
        Self {
            files_selection: 0,
            settings_section: SettingsSection::Toolchains,
            file_action_counter: 0,
            graphics_backend: GraphicsBackend::SoftwareFramebuffer,
        }
    }

    pub const fn files_selection(&self) -> usize {
        self.files_selection
    }

    pub const fn settings_section(&self) -> SettingsSection {
        self.settings_section
    }

    pub const fn graphics_backend(&self) -> GraphicsBackend {
        self.graphics_backend
    }

    /// Override the backend preference (e.g. from `/etc/orbita.conf`).
    pub fn set_graphics_backend(&mut self, backend: GraphicsBackend) {
        self.graphics_backend = backend;
    }

    pub fn cycle_files(&mut self, count: usize) {
        if count > 0 {
            self.files_selection = (self.files_selection + 1) % count;
        } else {
            self.files_selection = 0;
        }
    }

    pub fn set_files_selection(&mut self, index: usize) {
        self.files_selection = index;
    }

    pub fn cycle_settings(&mut self) {
        self.settings_section = self.settings_section.next();
    }

    pub fn next_file_action_id(&mut self) -> usize {
        let next = self.file_action_counter;
        self.file_action_counter = self.file_action_counter.saturating_add(1);
        next
    }

    pub fn cycle_graphics_backend(&mut self) {
        self.graphics_backend = self.graphics_backend.next();
    }
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct DesktopPointerState {
    pub x: usize,
    pub y: usize,
    focus_target: DesktopFocusTarget,
}

impl DesktopPointerState {
    pub const fn new(x: usize, y: usize) -> Self {
        Self {
            x,
            y,
            focus_target: DesktopFocusTarget::MainWindow,
        }
    }

    pub fn move_by(&mut self, dx: isize, dy: isize, width: usize, height: usize) {
        let next_x = if dx.is_negative() {
            self.x.saturating_sub(dx.unsigned_abs())
        } else {
            self.x.saturating_add(dx as usize)
        };
        let next_y = if dy.is_negative() {
            self.y.saturating_sub(dy.unsigned_abs())
        } else {
            self.y.saturating_add(dy as usize)
        };
        self.x = next_x.min(width.saturating_sub(1));
        self.y = next_y.min(height.saturating_sub(1));
    }

    pub const fn focus_target(&self) -> DesktopFocusTarget {
        self.focus_target
    }

    pub fn set_focus_target(&mut self, focus_target: DesktopFocusTarget) {
        self.focus_target = focus_target;
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct RuntimeEventBuffer {
    capacity: usize,
    entries: Vec<String>,
}

impl RuntimeEventBuffer {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            entries: Vec::new(),
        }
    }

    pub fn push(&mut self, event: impl Into<String>) {
        self.entries.push(event.into());
        if self.entries.len() > self.capacity {
            let remove = self.entries.len() - self.capacity;
            self.entries.drain(0..remove);
        }
    }

    pub fn render(&self) -> String {
        if self.entries.is_empty() {
            return String::from("no runtime events\n");
        }
        let mut out = String::new();
        for entry in &self.entries {
            out.push_str(entry.as_str());
            out.push('\n');
        }
        out
    }
}
