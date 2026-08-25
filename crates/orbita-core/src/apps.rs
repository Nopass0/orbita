extern crate alloc;

use alloc::format;
use alloc::string::String;

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum BuiltinAppIcon {
    Planet,
    Terminal,
    Folder,
    Settings,
    Monitor,
}

impl BuiltinAppIcon {
    pub const fn id(self) -> &'static str {
        match self {
            Self::Planet => "planet",
            Self::Terminal => "terminal",
            Self::Folder => "folder",
            Self::Settings => "settings",
            Self::Monitor => "monitor",
        }
    }
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct BuiltinApp {
    pub id: &'static str,
    pub name: &'static str,
    pub entry: &'static str,
    pub module: &'static str,
    pub icon: BuiltinAppIcon,
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct AppLaunchState {
    active: usize,
}

impl AppLaunchState {
    pub const fn new() -> Self {
        Self { active: 0 }
    }

    pub const fn active_index(&self) -> usize {
        self.active
    }

    pub fn active_app(&self) -> &'static BuiltinApp {
        &BUILTIN_APPS[self.active.min(BUILTIN_APPS.len().saturating_sub(1))]
    }

    pub fn activate_by_index(&mut self, index: usize) -> bool {
        if index < BUILTIN_APPS.len() {
            self.active = index;
            true
        } else {
            false
        }
    }

    pub fn activate_by_id(&mut self, id: &str) -> bool {
        if let Some((index, _)) = BUILTIN_APPS.iter().enumerate().find(|(_, app)| app.id == id) {
            self.active = index;
            true
        } else {
            false
        }
    }

    pub fn cycle_next(&mut self) {
        if !BUILTIN_APPS.is_empty() {
            self.active = (self.active + 1) % BUILTIN_APPS.len();
        }
    }
}

impl BuiltinApp {
    pub const fn descriptor(self) -> &'static str {
        self.entry
    }

    pub fn manifest_entry(self) -> String {
        format!(
            "[[apps]]\nid = \"{}\"\nname = \"{}\"\nentry = \"{}\"\nmodule = \"{}\"\nicon = \"{}\"\n",
            self.id,
            self.name,
            self.entry,
            self.module,
            self.icon.id()
        )
    }

    pub fn descriptor_body(self) -> String {
        format!(
            "id={}\nname={}\nkind=builtin\nmodule={}\nicon={}\n",
            self.id,
            self.name,
            self.module,
            self.icon.id()
        )
    }
}

pub const BUILTIN_APPS: &[BuiltinApp] = &[
    BuiltinApp {
        id: "terminal",
        name: "Orbita Terminal",
        entry: "/system/apps/terminal.app",
        module: "orbita-shell",
        icon: BuiltinAppIcon::Terminal,
    },
    BuiltinApp {
        id: "files",
        name: "Orbita Files",
        entry: "/system/apps/files.app",
        module: "orbita-fs",
        icon: BuiltinAppIcon::Folder,
    },
    BuiltinApp {
        id: "settings",
        name: "Orbita Settings",
        entry: "/system/apps/settings.app",
        module: "orbita-desktop",
        icon: BuiltinAppIcon::Settings,
    },
    BuiltinApp {
        id: "monitor",
        name: "Orbita Monitor",
        entry: "/system/apps/monitor.app",
        module: "orbita-desktop",
        icon: BuiltinAppIcon::Monitor,
    },
];

pub fn builtin_apps() -> &'static [BuiltinApp] {
    BUILTIN_APPS
}

pub fn builtin_apps_manifest() -> String {
    let mut manifest = String::new();
    for app in BUILTIN_APPS {
        manifest.push_str(app.manifest_entry().as_str());
        manifest.push('\n');
    }
    manifest
}
