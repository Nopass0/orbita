extern crate alloc;

use alloc::format;
use alloc::string::String;

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum BuiltinServiceState {
    Active,
    Planned,
}

impl BuiltinServiceState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Planned => "planned",
        }
    }
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct BuiltinService {
    pub id: &'static str,
    pub name: &'static str,
    pub module: &'static str,
    pub state: BuiltinServiceState,
    pub description: &'static str,
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum RuntimeServiceHealth {
    Healthy,
    Degraded,
    Planned,
}

impl RuntimeServiceHealth {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Healthy => "healthy",
            Self::Degraded => "degraded",
            Self::Planned => "planned",
        }
    }
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct ServiceRuntimeRecord {
    pub id: &'static str,
    pub state: BuiltinServiceState,
    pub health: RuntimeServiceHealth,
}

impl ServiceRuntimeRecord {
    pub fn manifest_entry(self) -> String {
        format!(
            "[[runtime_services]]\nid = \"{}\"\nstate = \"{}\"\nhealth = \"{}\"\n",
            self.id,
            self.state.as_str(),
            self.health.as_str()
        )
    }
}

impl BuiltinService {
    pub fn manifest_entry(self) -> String {
        format!(
            "[[services]]\nid = \"{}\"\nname = \"{}\"\nmodule = \"{}\"\nstate = \"{}\"\ndescription = \"{}\"\n",
            self.id,
            self.name,
            self.module,
            self.state.as_str(),
            self.description
        )
    }

    pub fn descriptor_path(self) -> String {
        format!("/system/services/{}.svc", self.id)
    }

    pub fn descriptor_body(self) -> String {
        format!(
            "id={}\nname={}\nmodule={}\nstate={}\ndescription={}\n",
            self.id,
            self.name,
            self.module,
            self.state.as_str(),
            self.description
        )
    }
}

pub const BUILTIN_SERVICES: &[BuiltinService] = &[
    BuiltinService {
        id: "desktop",
        name: "Orbita Desktop",
        module: "orbita-desktop",
        state: BuiltinServiceState::Active,
        description: "desktop compositor and launcher",
    },
    BuiltinService {
        id: "shell",
        name: "Orbita Shell",
        module: "orbita-shell",
        state: BuiltinServiceState::Active,
        description: "interactive shell runtime",
    },
    BuiltinService {
        id: "inventory",
        name: "Device Inventory",
        module: "orbita-hw",
        state: BuiltinServiceState::Active,
        description: "hardware discovery and reporting",
    },
    BuiltinService {
        id: "network",
        name: "Network Stack",
        module: "orbita-net",
        state: BuiltinServiceState::Planned,
        description: "future dns and http runtime",
    },
];

pub fn builtin_services() -> &'static [BuiltinService] {
    BUILTIN_SERVICES
}

pub fn builtin_services_manifest() -> String {
    let mut manifest = String::new();
    for service in BUILTIN_SERVICES {
        manifest.push_str(service.manifest_entry().as_str());
        manifest.push('\n');
    }
    manifest
}

pub fn runtime_services_manifest(records: &[ServiceRuntimeRecord]) -> String {
    let mut manifest = String::new();
    for record in records {
        manifest.push_str(record.manifest_entry().as_str());
        manifest.push('\n');
    }
    manifest
}
