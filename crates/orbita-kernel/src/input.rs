//! Interactive input handling: keyboard actions, chrome/workspace navigation, file shortcuts.


extern crate alloc;

use alloc::format;
use core::fmt::Write;
use orbita_core::{
    AppLaunchState, DesktopChromePanel, DesktopChromeState,
    DesktopFocusTarget, DesktopPointerState, DesktopWorkspaceState, RuntimeEventBuffer, builtin_apps,
};
use orbita_fs::MemoryVolume;
use orbita_net::NetworkStack;
use orbita_process::ProcessEngine;
use orbita_shell::{ShellEnvironment, ShellRuntime};
use orbita_std::String;
use crate::config::*;
use crate::console::*;
use crate::hosts::*;
use crate::ui::*;

pub(crate) fn handle_console_action(
    console: &mut BootConsole,
    fs: &mut MemoryVolume,
    process_engine: &mut Option<ProcessEngine>,
    net_stack: &mut NetworkStack,
    live_nic: &mut Option<orbita_hw::E1000>,
    shell_runtime: &ShellRuntime,
    shell_env: &mut ShellEnvironment,
    app_launch: &mut AppLaunchState,
    chrome: &mut DesktopChromeState,
    workspace: &mut DesktopWorkspaceState,
    pointer: &mut DesktopPointerState,
    framebuffer_width: usize,
    framebuffer_height: usize,
    runtime_events: &mut RuntimeEventBuffer,
    action: KeyAction,
) -> RedrawKind {
    match action {
        KeyAction::Char(ch) => {
            if chrome.active_panel() == DesktopChromePanel::None
                && app_launch.active_app().id == "files"
                && pointer.focus_target() == DesktopFocusTarget::MainWindow
            {
                if let Some(redraw) = handle_files_shortcut(console, fs, shell_env, workspace, runtime_events, ch) {
                    return redraw;
                }
            }
            if chrome.active_panel() == DesktopChromePanel::None
                && app_launch.active_app().id == "settings"
                && pointer.focus_target() == DesktopFocusTarget::MainWindow
            {
                if let Some(redraw) = handle_settings_shortcut(console, fs, workspace, runtime_events, ch) {
                    return redraw;
                }
            }
            if chrome.active_panel() == DesktopChromePanel::None
                && app_launch.active_app().id == "monitor"
                && pointer.focus_target() == DesktopFocusTarget::MainWindow
            {
                if let Some(redraw) = handle_monitor_shortcut(console, fs, runtime_events, ch) {
                    return redraw;
                }
            }
            match chrome.active_panel() {
                DesktopChromePanel::Start | DesktopChromePanel::Search => {
                    if ch.is_ascii_graphic() || ch == ' ' {
                        chrome.push_query(ch.to_ascii_lowercase());
                        console.set_status(chrome_status(chrome).as_str());
                        runtime_events.push(format!("chrome: query {}", chrome.query()));
                        return RedrawKind::Full;
                    }
                }
                DesktopChromePanel::None | DesktopChromePanel::Tray => {}
            }
            console.input.push(ch);
            console.cursor_visible = true;
            RedrawKind::PromptOnly
        }
        KeyAction::Backspace => {
            match chrome.active_panel() {
                DesktopChromePanel::Start | DesktopChromePanel::Search => {
                    chrome.pop_query();
                    console.set_status(chrome_status(chrome).as_str());
                    runtime_events.push("chrome: backspace");
                    return RedrawKind::Full;
                }
                DesktopChromePanel::None | DesktopChromePanel::Tray => {}
            }
            console.input.pop();
            console.cursor_visible = true;
            RedrawKind::PromptOnly
        }
        KeyAction::Enter => {
            if let Some(redraw) =
                activate_chrome_selection(console, shell_env, app_launch, chrome, pointer, runtime_events)
            {
                return redraw;
            }
            if let Some(redraw) = activate_workspace_selection(console, fs, shell_env, app_launch, workspace, runtime_events) {
                return redraw;
            }
            if console.input.trim().is_empty() {
                return RedrawKind::None;
            }
            chrome.clear();
            let command = core::mem::take(&mut console.input);
            console.cursor_visible = true;
            let host = console.hostname.clone();
            console.push_line_fmt(format_args!("{}:{}# {command}", host, shell_env.cwd()));
            let mut shell_host = KernelShellHost { process_engine, net_stack, live_nic };
            shell_runtime.execute_line(shell_env, fs, console, &mut shell_host, &command);
            drop(shell_host);
            // Process round-trip: the same command also flows through the
            // spawned shell process's stdin/stdout fds so the console is a
            // real terminal for it.
            if let Some(engine) = process_engine.as_mut() {
                if let Some(process) = engine.process_mut(orbita_process::Pid(1)) {
                    process.stdin_mut().push_line(&command);
                }
                let mut process_host = ShellProcessHost { runtime: shell_runtime };
                engine.pump(&mut process_host);
                if let Some(process) = engine.process_mut(orbita_process::Pid(1)) {
                    for line in process.fd_table.stdout.drain_lines() {
                        console.push_line(&format!("[shell:{}] {}", line.len(), line));
                    }
                    for line in process.fd_table.stderr.drain_lines() {
                        console.push_line(&format!("[shell:err] {line}"));
                    }
                }
            }
            console.cwd = String::from(shell_env.cwd());
            runtime_events.push(format!("shell: {}", command.trim()));
            RedrawKind::Full
        }
        KeyAction::NextApp => {
            if chrome.active_panel() == DesktopChromePanel::Start
                || chrome.active_panel() == DesktopChromePanel::Search
            {
                let count = chrome_match_count(chrome);
                chrome.cycle_selection(count);
                console.set_status(chrome_status(chrome).as_str());
                runtime_events.push(format!("chrome: selection {}", chrome.selection()));
                return RedrawKind::Full;
            }
            if app_launch.active_app().id == "files" {
                let count = fs
                    .list_path(shell_env.cwd())
                    .map(|listing| listing.entries.len())
                    .unwrap_or(0);
                workspace.cycle_files(count);
                console.set_status("files: selection moved");
                runtime_events.push(format!("files: selection {}", workspace.files_selection()));
                return RedrawKind::Full;
            }
            if app_launch.active_app().id == "settings" {
                workspace.cycle_settings();
                console.set_status(workspace.settings_section().label());
                runtime_events.push(format!("settings: section {}", workspace.settings_section().label()));
                return RedrawKind::Full;
            }
            app_launch.cycle_next();
            chrome.clear();
            shell_env.set_active_app(app_launch.active_app().id);
            pointer.set_focus_target(DesktopFocusTarget::MainWindow);
            console.set_status(app_launch.active_app().name);
            console.push_line_fmt(format_args!("desktop: active app -> {}", app_launch.active_app().name));
            runtime_events.push(format!("desktop: active app -> {}", app_launch.active_app().id));
            RedrawKind::Full
        }
        KeyAction::LaunchApp(index) => {
            if app_launch.activate_by_index(index) {
                chrome.clear();
                shell_env.set_active_app(app_launch.active_app().id);
                pointer.set_focus_target(DesktopFocusTarget::MainWindow);
                console.set_status(app_launch.active_app().name);
                console.push_line_fmt(format_args!("desktop: active app -> {}", app_launch.active_app().name));
                runtime_events.push(format!("desktop: active app -> {}", app_launch.active_app().id));
                RedrawKind::Full
            } else {
                RedrawKind::None
            }
        }
        KeyAction::PointerMove(dx, dy) => {
            pointer.move_by(dx, dy, framebuffer_width, framebuffer_height);
            RedrawKind::Full
        }
        KeyAction::PointerActivate => {
            match desktop_hit_target(framebuffer_width, framebuffer_height, pointer.x, pointer.y) {
                DesktopFocusTarget::DockApp(index) => {
                    if app_launch.activate_by_index(index) {
                        chrome.clear();
                        shell_env.set_active_app(app_launch.active_app().id);
                        pointer.set_focus_target(DesktopFocusTarget::MainWindow);
                        console.set_status(app_launch.active_app().name);
                        console.push_line_fmt(format_args!("desktop: active app -> {}", app_launch.active_app().name));
                        runtime_events.push(format!("pointer: activate {}", app_launch.active_app().id));
                        return RedrawKind::Full;
                    }
                }
                DesktopFocusTarget::DockStart => {
                    chrome.activate(DesktopChromePanel::Start);
                    pointer.set_focus_target(DesktopFocusTarget::DockStart);
                    console.set_status("Start menu");
                    console.push_line("desktop: start surface selected");
                    runtime_events.push("pointer: focus dock-start");
                    return RedrawKind::Full;
                }
                DesktopFocusTarget::DockSearch => {
                    chrome.activate(DesktopChromePanel::Search);
                    pointer.set_focus_target(DesktopFocusTarget::DockSearch);
                    console.set_status("Search");
                    console.push_line("desktop: search surface selected");
                    runtime_events.push("pointer: focus dock-search");
                    return RedrawKind::Full;
                }
                DesktopFocusTarget::DockTray => {
                    chrome.activate(DesktopChromePanel::Tray);
                    pointer.set_focus_target(DesktopFocusTarget::DockTray);
                    console.set_status("Tray");
                    console.push_line("desktop: tray surface selected");
                    runtime_events.push("pointer: focus dock-tray");
                    return RedrawKind::Full;
                }
                target => {
                    chrome.clear();
                    pointer.set_focus_target(target);
                    console.set_status(target.label());
                    console.push_line_fmt(format_args!("desktop: focus -> {}", target.label()));
                    runtime_events.push(format!("pointer: focus {}", target.label()));
                    return RedrawKind::Full;
                }
            }
            RedrawKind::None
        }
    }
}

pub(crate) fn handle_files_shortcut(
    console: &mut BootConsole,
    fs: &mut MemoryVolume,
    shell_env: &mut ShellEnvironment,
    workspace: &mut DesktopWorkspaceState,
    runtime_events: &mut RuntimeEventBuffer,
    ch: char,
) -> Option<RedrawKind> {
    match ch {
        'a' | 'A' => {
            let parent = parent_path(shell_env.cwd());
            shell_env.set_cwd(parent.as_str());
            workspace.set_files_selection(0);
            console.set_status(parent.as_str());
            console.push_line_fmt(format_args!("files: parent -> {}", parent));
            runtime_events.push(format!("files: parent {}", parent));
            Some(RedrawKind::Full)
        }
        'n' | 'N' => {
            let id = workspace.next_file_action_id();
            let path = join_cwd(shell_env.cwd(), format!("note-{}.txt", id).as_str());
            let body = format!("Orbita note {}\ncreated from desktop files app\n", id);
            match fs.create_file_path(path.as_str(), body.as_bytes()) {
                Ok(()) => {
                    console.set_status("files: note created");
                    console.push_line_fmt(format_args!("files: created {}", path));
                    runtime_events.push(format!("files: create {}", path));
                    Some(RedrawKind::Full)
                }
                Err(_) => Some(RedrawKind::None),
            }
        }
        'm' | 'M' => {
            let id = workspace.next_file_action_id();
            let path = join_cwd(shell_env.cwd(), format!("dir-{}", id).as_str());
            match fs.create_dir_all(path.as_str()) {
                Ok(()) => {
                    console.set_status("files: directory created");
                    console.push_line_fmt(format_args!("files: mkdir {}", path));
                    runtime_events.push(format!("files: mkdir {}", path));
                    Some(RedrawKind::Full)
                }
                Err(_) => Some(RedrawKind::None),
            }
        }
        'c' | 'C' => {
            match selected_directory_entry_path(fs, shell_env.cwd(), workspace.files_selection()) {
                Some(source) => {
                    if copy_path_into_cwd(fs, shell_env.cwd(), source.as_str()) {
                        console.set_status("files: copied");
                        console.push_line_fmt(format_args!("files: copy {}", source));
                        runtime_events.push(format!("files: copy {}", source));
                        Some(RedrawKind::Full)
                    } else {
                        Some(RedrawKind::None)
                    }
                }
                None => Some(RedrawKind::None),
            }
        }
        'd' | 'D' => {
            match selected_directory_entry_path(fs, shell_env.cwd(), workspace.files_selection()) {
                Some(path) => match fs.remove_path(path.as_str()) {
                    Ok(()) => {
                        workspace.set_files_selection(0);
                        console.set_status("files: deleted");
                        console.push_line_fmt(format_args!("files: delete {}", path));
                        runtime_events.push(format!("files: delete {}", path));
                        Some(RedrawKind::Full)
                    }
                    Err(_) => Some(RedrawKind::None),
                },
                None => Some(RedrawKind::None),
            }
        }
        _ => None,
    }
}

pub(crate) fn handle_settings_shortcut(
    console: &mut BootConsole,
    fs: &mut MemoryVolume,
    workspace: &mut DesktopWorkspaceState,
    runtime_events: &mut RuntimeEventBuffer,
    ch: char,
) -> Option<RedrawKind> {
    match ch {
        't' | 'T' => {
            let _ = fs;
            console.set_status("settings: install toolchains via pkg/apt");
            console.push_line("settings: use `pkg install python3 nodejs rust build-essential`");
            console.push_line("settings: `apt install python3 nodejs rust build-essential` is also supported");
            runtime_events.push("settings: toolchain install hint");
            Some(RedrawKind::Full)
        }
        'w' | 'W' => {
            if toggle_config_value(
                fs,
                "/etc/network.toml",
                "stack = \"planned\"",
                "stack = \"lab-ready\"",
            ) || toggle_config_value(
                fs,
                "/etc/network.toml",
                "stack = \"lab-ready\"",
                "stack = \"planned\"",
            ) {
                console.set_status("settings: network stack toggled");
                console.push_line("settings: toggled network stack profile");
                runtime_events.push("settings: toggle network stack");
                Some(RedrawKind::Full)
            } else {
                Some(RedrawKind::None)
            }
        }
        's' | 'S' => {
            workspace.cycle_settings();
            console.set_status(workspace.settings_section().label());
            runtime_events.push(format!("settings: section {}", workspace.settings_section().label()));
            Some(RedrawKind::Full)
        }
        'g' | 'G' => {
            workspace.cycle_graphics_backend();
            console.set_status(workspace.graphics_backend().label());
            console.push_line_fmt(format_args!("settings: graphics backend -> {}", workspace.graphics_backend().label()));
            runtime_events.push(format!("settings: graphics backend {}", workspace.graphics_backend().label()));
            Some(RedrawKind::Full)
        }
        _ => None,
    }
}

pub(crate) fn handle_monitor_shortcut(
    console: &mut BootConsole,
    fs: &mut MemoryVolume,
    runtime_events: &mut RuntimeEventBuffer,
    ch: char,
) -> Option<RedrawKind> {
    match ch {
        'p' | 'P' => {
            let services = fs
                .read_file_path("/run/services/status.toml")
                .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
                .unwrap_or_else(|_| String::from("services unavailable"));
            let events = fs
                .read_file_path("/run/events.log")
                .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
                .unwrap_or_else(|_| String::from("events unavailable"));
            let snapshot = format!("services\n{}\n\nevents\n{}\n", services, events);
            match fs.create_file_path("/run/monitor-snapshot.txt", snapshot.as_bytes()) {
                Ok(()) => {
                    console.set_status("monitor: snapshot saved");
                    console.push_line("monitor: wrote /run/monitor-snapshot.txt");
                    runtime_events.push("monitor: snapshot");
                    Some(RedrawKind::Full)
                }
                Err(_) => Some(RedrawKind::None),
            }
        }
        'l' | 'L' => {
            runtime_events.push("monitor: heartbeat");
            console.set_status("monitor: heartbeat logged");
            console.push_line("monitor: heartbeat event appended");
            Some(RedrawKind::Full)
        }
        _ => None,
    }
}

pub(crate) fn join_cwd(cwd: &str, leaf: &str) -> String {
    if cwd == "/" {
        format!("/{}", leaf)
    } else {
        format!("{}/{}", cwd.trim_end_matches('/'), leaf)
    }
}

pub(crate) fn describe_directory_entry(fs: &mut MemoryVolume, cwd: &str, name: &str) -> String {
    let path = join_cwd(cwd, name.trim_start_matches('/'));
    match fs.read_file_path(path.as_str()) {
        Ok(bytes) => {
            let text = String::from_utf8_lossy(&bytes);
            let mut preview = String::new();
            for line in text.lines().take(8) {
                if !preview.is_empty() {
                    preview.push('\n');
                }
                preview.push_str(line);
            }
            if preview.is_empty() {
                String::from("(empty file)")
            } else {
                preview
            }
        }
        Err(_) => match fs.list_path(path.as_str()) {
            Ok(listing) => {
                let mut preview = String::from("directory\n");
                for entry in listing.entries.into_iter().take(8) {
                    let _ = writeln!(&mut preview, "- {}", entry.name);
                }
                preview
            }
            Err(_) => String::from("unavailable"),
        },
    }
}

pub(crate) fn selected_directory_entry_path(fs: &mut MemoryVolume, cwd: &str, selection: usize) -> Option<String> {
    let listing = fs.list_path(cwd).ok()?;
    let entry = listing.entries.get(selection)?;
    Some(join_cwd(cwd, entry.name.as_str()))
}

pub(crate) fn copy_path_into_cwd(fs: &mut MemoryVolume, cwd: &str, source: &str) -> bool {
    let name = source.rsplit('/').next().unwrap_or("copy");
    let target = join_cwd(cwd, format!("copy-{}", name).as_str());
    if let Ok(bytes) = fs.read_file_path(source) {
        return fs.create_file_path(target.as_str(), &bytes).is_ok();
    }
    if fs.list_path(source).is_ok() {
        return fs.create_dir_all(target.as_str()).is_ok();
    }
    false
}

pub(crate) fn parent_path(path: &str) -> String {
    if path == "/" {
        return String::from("/");
    }
    let trimmed = path.trim_end_matches('/');
    if let Some(index) = trimmed.rfind('/') {
        if index == 0 {
            String::from("/")
        } else {
            String::from(&trimmed[..index])
        }
    } else {
        String::from("/")
    }
}

pub(crate) fn chrome_match_count(chrome: &DesktopChromeState) -> usize {
    match chrome.active_panel() {
        DesktopChromePanel::Start => builtin_apps()
            .iter()
            .filter(|app| {
                let needle = chrome.query();
                needle.is_empty()
                    || app.id.contains(needle)
                    || app
                        .name
                        .to_ascii_lowercase()
                        .contains(&needle.to_ascii_lowercase())
            })
            .count(),
        DesktopChromePanel::Search => builtin_apps()
            .iter()
            .filter(|app| {
                let needle = chrome.query();
                needle.is_empty()
                    || app.id.contains(needle)
                    || app
                        .name
                        .to_ascii_lowercase()
                        .contains(&needle.to_ascii_lowercase())
            })
            .count(),
        DesktopChromePanel::None | DesktopChromePanel::Tray => 0,
    }
}

pub(crate) fn chrome_status(chrome: &DesktopChromeState) -> String {
    format!(
        "chrome:{} query='{}' selection={}",
        chrome.active_panel().label(),
        chrome.query(),
        chrome.selection()
    )
}

pub(crate) fn activate_chrome_selection(
    console: &mut BootConsole,
    shell_env: &mut ShellEnvironment,
    app_launch: &mut AppLaunchState,
    chrome: &mut DesktopChromeState,
    pointer: &mut DesktopPointerState,
    runtime_events: &mut RuntimeEventBuffer,
) -> Option<RedrawKind> {
    match chrome.active_panel() {
        DesktopChromePanel::Start | DesktopChromePanel::Search => {
            if let Some(index) = chrome.selected_app_index() {
                if app_launch.activate_by_index(index) {
                    shell_env.set_active_app(app_launch.active_app().id);
                    pointer.set_focus_target(DesktopFocusTarget::MainWindow);
                    console.set_status(app_launch.active_app().name);
                    console.push_line_fmt(format_args!("chrome: launch -> {}", app_launch.active_app().name));
                    runtime_events.push(format!("chrome: launch {}", app_launch.active_app().id));
                    chrome.clear();
                    return Some(RedrawKind::Full);
                }
            } else {
                console.set_status("chrome: no match");
                runtime_events.push("chrome: no match");
                return Some(RedrawKind::Full);
            }
            Some(RedrawKind::None)
        }
        DesktopChromePanel::Tray => {
            chrome.clear();
            pointer.set_focus_target(DesktopFocusTarget::Desktop);
            console.set_status("Tray closed");
            runtime_events.push("chrome: tray closed");
            Some(RedrawKind::Full)
        }
        DesktopChromePanel::None => None,
    }
}

pub(crate) fn activate_workspace_selection(
    console: &mut BootConsole,
    fs: &mut MemoryVolume,
    shell_env: &mut ShellEnvironment,
    app_launch: &AppLaunchState,
    workspace: &mut DesktopWorkspaceState,
    runtime_events: &mut RuntimeEventBuffer,
) -> Option<RedrawKind> {
    match app_launch.active_app().id {
        "files" => {
            let listing = match fs.list_path(shell_env.cwd()) {
                Ok(listing) => listing,
                Err(_) => return Some(RedrawKind::None),
            };
            if let Some(entry) = listing.entries.get(workspace.files_selection()) {
                let path = join_cwd(shell_env.cwd(), entry.name.as_str());
                if fs.list_path(path.as_str()).is_ok() {
                    shell_env.set_cwd(path.as_str());
                    workspace.set_files_selection(0);
                    console.set_status(path.as_str());
                    console.push_line_fmt(format_args!("files: open -> {}", path));
                    runtime_events.push(format!("files: open {}", path));
                } else {
                    console.set_status(path.as_str());
                    console.push_line_fmt(format_args!("files: file -> {}", path));
                    runtime_events.push(format!("files: inspect {}", path));
                }
                return Some(RedrawKind::Full);
            }
            Some(RedrawKind::None)
        }
        "settings" => {
            workspace.cycle_settings();
            console.set_status(workspace.settings_section().label());
            runtime_events.push(format!("settings: section {}", workspace.settings_section().label()));
            Some(RedrawKind::Full)
        }
        _ => None,
    }
}
