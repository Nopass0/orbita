//! Runtime seeding: shell volume image, runtime inventory files, and live state sync.


extern crate alloc;

use alloc::format;
use core::fmt::Write;
use core::sync::atomic::Ordering;
use orbita_core::{
    AppLaunchState, BuiltinServiceState, DesktopChromeState, DesktopPointerState, DesktopSessionState, DesktopWorkspaceState,
    GraphicsBackend, RuntimeEventBuffer, RuntimeServiceHealth, ServiceRuntimeRecord, builtin_apps,
    builtin_apps_manifest,
    builtin_services, builtin_services_manifest, runtime_services_manifest,
};
use orbita_fs::MemoryVolume;
use orbita_hw::PciInventory;
use orbita_std::String;
use crate::config::*;
use crate::console::*;
use crate::ui::*;
use crate::{KEYBOARD_IRQ_COUNT, TIMER_IRQ_COUNT};

pub(crate) fn read_fs_text(fs: &mut MemoryVolume, path: &str, fallback: &str) -> String {
    match fs.read_file_path(path) {
        Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
        Err(_) => String::from(fallback),
    }
}

pub(crate) fn seed_runtime_inventory(
    fs: &mut MemoryVolume,
    pci_inventory: &PciInventory,
    gpu_identity: Option<&str>,
) -> Result<(), orbita_fs::FsError> {
    fs.create_dir_all("/proc")?;

    let mut devices = String::new();
    let _ = writeln!(&mut devices, "{}", pci_inventory.inventory_summary());
    for line in pci_inventory.device_lines() {
        let _ = writeln!(&mut devices, "{line}");
    }
    fs.create_file_path("/proc/pci.txt", devices.as_bytes())?;

    let mut gpu = String::new();
    let _ = writeln!(&mut gpu, "{}", pci_inventory.gpu_summary());
    if let Some(identity) = gpu_identity {
        let _ = writeln!(&mut gpu, "framebuffer=UEFI GOP");
        let _ = writeln!(&mut gpu, "active={identity}");
    } else {
        let _ = writeln!(&mut gpu, "framebuffer=UEFI GOP");
        let _ = writeln!(&mut gpu, "active=uefi-gop-only");
    }
    for device in pci_inventory.gpu_devices() {
        let _ = writeln!(&mut gpu, "candidate={}", device.summary_line());
    }
    fs.create_file_path("/proc/gpu.txt", gpu.as_bytes())?;

    Ok(())
}

pub(crate) fn seed_shell_volume(fs: &mut MemoryVolume) -> Result<(), orbita_fs::FsError> {
    fs.create_dir_all("/etc")?;
    fs.create_dir_all("/home/user")?;
    fs.create_dir_all("/system/apps")?;
    fs.create_dir_all("/system/services")?;
    fs.create_dir_all("/system/manifest")?;
    fs.create_dir_all("/run/desktop")?;
    fs.create_dir_all("/run/graphics")?;
    fs.create_dir_all("/run/network")?;
    fs.create_dir_all("/run/toolchains")?;
    fs.create_dir_all("/run/services")?;
    fs.create_dir_all("/usr/bin")?;
    fs.create_dir_all("/usr/lib/python3/site-packages")?;
    fs.create_dir_all("/usr/lib/node_modules")?;
    fs.create_dir_all("/opt/toolchains")?;
    fs.create_dir_all("/opt/toolchains/bin")?;
    fs.create_dir_all("/var/lib/orbita/pkg")?;
    fs.create_dir_all("/var/log")?;
    fs.create_file_path(
        "/readme.txt",
        b"Orbita OS memory volume.\nUse help, pipes, redirects, env vars, and sh /etc/profile.sh.\nInstall Linux-like compatibility packages with pkg install or apt install.\nSee /system/manifest/apps.toml and /etc/toolchains.toml.\n",
    )?;
    fs.create_file_path("/etc/motd", b"Welcome to Orbita OS.\n")?;
    fs.create_file_path(
        "/etc/toolchains.toml",
        b"[console]\nmode = \"rendered\"\nterminal = \"framebuffer-shell\"\nqemu = \"supported\"\ninstall = \"linux-like-compat\"\n\n[toolchains]\npython = \"available\"\npip = \"available\"\nnode = \"available\"\nnpm = \"available\"\nrust = \"available\"\nc = \"available\"\ncpp = \"available\"\nclang = \"available\"\nmake = \"available\"\nbuild_essential = \"available\"\n\n[packages]\ninstalled = \"(none)\"\n",
    )?;
    fs.create_file_path(
        "/etc/network.toml",
        b"[network]\nstack = \"planned\"\ndns = \"planned\"\nhttp = \"planned\"\n",
    )?;
    let apps_manifest = builtin_apps_manifest();
    fs.create_file_path("/system/manifest/apps.toml", apps_manifest.as_bytes())?;
    for app in builtin_apps() {
        let descriptor = app.descriptor_body();
        fs.create_file_path(app.descriptor(), descriptor.as_bytes())?;
    }
    let services_manifest = builtin_services_manifest();
    fs.create_file_path("/system/manifest/services.toml", services_manifest.as_bytes())?;
    for service in builtin_services() {
        let descriptor = service.descriptor_body();
        let path = service.descriptor_path();
        fs.create_file_path(path.as_str(), descriptor.as_bytes())?;
    }
    fs.create_file_path(
        "/etc/profile.sh",
        b"export GREETING=Orbita\nmkdir /tmp\nwrite /tmp/hello.txt \"$GREETING shell runtime\"\ncat /tmp/hello.txt\npkg update\npkg install python3 nodejs rust build-essential\npython -c \"print('hello from orbita')\"\nnode -e \"console.log('hello from orbita')\"\ncat /system/manifest/apps.toml\ncat /system/manifest/services.toml\n",
    )?;
    // Scripting-language demo: runs at every boot via `sh /etc/demo.sh`
    // (see docs/scripting.md) — proves if/for/test/&& in the live OS.
    fs.create_file_path(
        "/etc/demo.sh",
        b"#!/bin/sh\n# Orbita scripting language demo (docs/scripting.md)\nfor d in /etc /home /usr\ndo\n  if test -d $d\n  then\n    echo \"script dir: $d\"\n  fi\ndone\nif test -f /etc/orbita.conf\nthen\n  echo \"script: if ok (config found)\"\nelse\n  echo \"script: config missing\"\nfi\ntest -d /etc && echo \"script: and-ok\" || echo \"script: and-fail\"\ni=0\nwhile test $i -lt 3\ndo\n  i=$((i+1))\ndone\necho \"script: arith ok (i=$i)\"\necho \"script: subst ok ($(echo live))\"\nsquare() {\n  echo \"script: fn ok ($1*$1=$(( $1 * $1 )))\"\n}\nsquare 6\ncase host in\n  host*) echo \"script: case ok\" ;;\n  *) echo \"script: case miss\" ;;\nesac\nn=0\nuntil test $n -ge 2\ndo\n  n=$((n+1))\ndone\necho \"script: until ok (n=$n)\"\n",
    )?;
    fs.create_file_path(
        "/var/lib/orbita/pkg/available.txt",
        b"# Orbita package index\npython3|3.12.0-compat|runtime|Python 3 compatibility runtime with python/pip entrypoints|python,python3,pip,pip3|\nnodejs|22.0.0-compat|runtime|Node.js compatibility runtime with npm/npx entrypoints|node,nodejs,npm,npx|\nrust|1.80.0-compat|toolchain|Rust compatibility toolchain with cargo/rustc/rustfmt|rust,cargo,rustc,rustfmt|\ngcc|14.2.0-compat|compiler|GNU C compatibility compiler with cc entrypoint|gcc,cc|\ngxx|14.2.0-compat|compiler|GNU C++ compatibility compiler with g++/c++ entrypoints|g++,c++|gcc\nclang|18.1.0-compat|compiler|LLVM/Clang compatibility compiler with clang/clang++ entrypoints|clang,clang++|\nmake|4.4-compat|build|GNU make compatibility runner|make|\nbuild-essential|1.0-compat|meta|Meta-package for Linux-like C/C++ build environments|build-essential|gcc,gxx,make\n",
    )?;
    fs.create_file_path("/var/lib/orbita/pkg/installed.txt", b"")?;
    fs.create_file_path("/var/lib/orbita/pkg/pip-installed.txt", b"")?;
    fs.create_file_path("/var/lib/orbita/pkg/npm-installed.txt", b"")?;
    fs.create_file_path("/run/services/status.toml", b"")?;
    fs.create_file_path("/run/graphics/status.toml", b"")?;
    fs.create_file_path("/run/network/status.toml", b"")?;
    fs.create_file_path("/run/toolchains/status.toml", b"")?;
    fs.create_file_path("/run/desktop/session.toml", b"")?;
    fs.create_file_path("/run/events.log", b"Orbita runtime log\n")?;
    Ok(())
}

pub(crate) fn sync_runtime_state(
    fs: &mut MemoryVolume,
    console: &BootConsole,
    app_launch: &AppLaunchState,
    chrome: &DesktopChromeState,
    workspace: &DesktopWorkspaceState,
    pointer: &DesktopPointerState,
    frame_counter: u32,
    runtime_events: &RuntimeEventBuffer,
    net_live: Option<(u64, u64, u64, u64, usize)>,
) -> Result<(), orbita_fs::FsError> {
    let timer_ticks = TIMER_IRQ_COUNT.load(Ordering::Relaxed);
    let keyboard_irqs = KEYBOARD_IRQ_COUNT.load(Ordering::Relaxed);
    let network_lab_ready = config_contains(fs, "/etc/network.toml", "stack = \"lab-ready\"");
    let python_installed = package_installed(fs, "python3");
    let node_installed = package_installed(fs, "nodejs");
    let rust_installed = package_installed(fs, "rust");
    let gcc_installed = package_installed(fs, "gcc");
    let gxx_installed = package_installed(fs, "gxx");
    let clang_installed = package_installed(fs, "clang");
    let make_installed = package_installed(fs, "make");
    let build_essential_installed = package_installed(fs, "build-essential");
    let runtime = [
        ServiceRuntimeRecord {
            id: "desktop",
            state: BuiltinServiceState::Active,
            health: RuntimeServiceHealth::Healthy,
        },
        ServiceRuntimeRecord {
            id: "shell",
            state: BuiltinServiceState::Active,
            health: if console.status.contains("failed") || console.status.contains("error") {
                RuntimeServiceHealth::Degraded
            } else {
                RuntimeServiceHealth::Healthy
            },
        },
        ServiceRuntimeRecord {
            id: "inventory",
            state: BuiltinServiceState::Active,
            health: RuntimeServiceHealth::Healthy,
        },
        ServiceRuntimeRecord {
            id: "network",
            state: if network_lab_ready {
                BuiltinServiceState::Active
            } else {
                BuiltinServiceState::Planned
            },
            health: if network_lab_ready {
                RuntimeServiceHealth::Healthy
            } else {
                RuntimeServiceHealth::Planned
            },
        },
    ];
    let manifest = runtime_services_manifest(&runtime);
    fs.create_file_path("/run/services/status.toml", manifest.as_bytes())?;
    let network_status = match net_live {
        Some((rx_frames, rx_bytes, tx_frames, tx_bytes, arp_entries)) => format!(
            "stack=live-e1000\nrx_frames={}\nrx_bytes={}\ntx_frames={}\ntx_bytes={}\narp_entries={}\nhealth=healthy\n",
            rx_frames, rx_bytes, tx_frames, tx_bytes, arp_entries
        ),
        None => format!(
            "stack={}\nhealth=no-live-nic\n",
            if network_lab_ready { "lab-ready" } else { "planned" }
        ),
    };
    fs.create_file_path("/run/network/status.toml", network_status.as_bytes())?;
    let graphics_status = format!(
        "renderer={}\napi={}\nvsync=not-available\nvulkan={}\npresent={}\nframes_in_flight={}\n",
        workspace.graphics_backend().label(),
        workspace.graphics_backend().api(),
        if workspace.graphics_backend() == GraphicsBackend::Vulkan {
            "active"
        } else {
            "inactive"
        },
        workspace.graphics_backend().present_mode(),
        effective_backend_info(workspace.graphics_backend()).swapchain_len
    );
    fs.create_file_path("/run/graphics/status.toml", graphics_status.as_bytes())?;
    let toolchain_status = format!(
        "python={}\npip={}\nnode={}\nnpm={}\nrust={}\nc={}\ncpp={}\nclang={}\nmake={}\nbuild_essential={}\n",
        if python_installed { "installed-compat" } else { "available" },
        if python_installed { "installed-compat" } else { "available" },
        if node_installed { "installed-compat" } else { "available" },
        if node_installed { "installed-compat" } else { "available" },
        if rust_installed { "installed-compat" } else { "available" },
        if gcc_installed { "installed-compat" } else { "available" },
        if gxx_installed { "installed-compat" } else { "available" },
        if clang_installed { "installed-compat" } else { "available" },
        if make_installed { "installed-compat" } else { "available" },
        if build_essential_installed { "installed-compat" } else { "available" },
    );
    fs.create_file_path("/run/toolchains/status.toml", toolchain_status.as_bytes())?;

    let session = DesktopSessionState {
        frame_counter,
        active_app: app_launch.active_app().id,
        status: console.status.as_str(),
        graphics_backend: workspace.graphics_backend().label(),
        graphics_api: workspace.graphics_backend().api(),
        present_mode: workspace.graphics_backend().present_mode(),
        frames_in_flight: effective_backend_info(workspace.graphics_backend()).swapchain_len,
        cursor_visible: console.cursor_visible,
        pointer_x: pointer.x,
        pointer_y: pointer.y,
        focus_target: pointer.focus_target().label(),
        chrome_panel: chrome.active_panel().label(),
        chrome_query: chrome.query(),
        chrome_selection: chrome.selection(),
        files_selection: workspace.files_selection(),
        settings_section: workspace.settings_section().label(),
    };
    let session_manifest = session.manifest();
    fs.create_file_path("/run/desktop/session.toml", session_manifest.as_bytes())?;

    let event_log = format!(
        "timer_irqs={}\nkeyboard_irqs={}\n\n{}",
        timer_ticks,
        keyboard_irqs,
        runtime_events.render()
    );
    fs.create_file_path("/run/events.log", event_log.as_bytes())?;
    Ok(())
}

pub(crate) fn package_installed(fs: &mut MemoryVolume, package: &str) -> bool {
    fs.read_file_path("/var/lib/orbita/pkg/installed.txt")
        .map(|bytes| {
            String::from_utf8_lossy(&bytes)
                .lines()
                .map(str::trim)
                .any(|line| line == package)
        })
        .unwrap_or(false)
}
