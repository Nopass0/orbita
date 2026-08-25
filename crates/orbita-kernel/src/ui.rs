//! Desktop composition glue: panel cache, boot scene, hit-testing, and backend mapping.


extern crate alloc;

use alloc::vec::Vec;
use alloc::format;
use core::fmt::Write;
use orbita_desktop::{BootSplash, DesktopConsoleSnapshot, DesktopRenderer, DesktopScene, RedrawScope};
use orbita_core::{
    AppLaunchState, BootSummary, DesktopChromePanel, DesktopChromeState,
    DesktopFocusTarget, DesktopPointerState, DesktopWorkspaceState,
    GraphicsBackend, builtin_apps,
};
use orbita_fs::{
    MemoryVolume,
    VolumeInspector,
};
use orbita_shell::ShellEnvironment;
use orbita_std::{String, diagnostics};
use orbita_video::{create_backend, FrameCompositor, Framebuffer, Point, Rect};
use crate::console::*;
use crate::input::*;
use crate::seed::*;

pub(crate) struct PanelCache {
    pub(crate) total_text: String,
    pub(crate) free_text: String,
    pub(crate) chrome_body: String,
    pub(crate) files_cwd: String,
    pub(crate) files_listing: String,
    pub(crate) files_selected_name: String,
    pub(crate) files_preview_text: String,
    pub(crate) toolchains_combined_text: String,
    pub(crate) network_combined_text: String,
    pub(crate) services_text: String,
    pub(crate) runtime_services_text: String,
    pub(crate) events_text: String,
}

impl PanelCache {
    pub(crate) fn refresh(
        &mut self,
        fs: &mut MemoryVolume,
        chrome: &DesktopChromeState,
        workspace: &DesktopWorkspaceState,
        shell_env: &ShellEnvironment,
    ) {
        let stats = fs.space_stats();
        self.total_text = format!("{}", diagnostics::format_bytes(stats.total_bytes()));
        self.free_text = format!("{}", diagnostics::format_bytes(stats.available_bytes()));
        self.chrome_body = build_chrome_panel_body(chrome);
        self.files_cwd = String::from(shell_env.cwd());
        let (listing, selected, preview) = match fs.list_path(self.files_cwd.as_str()) {
            Ok(result) => {
                let mut selected_name = String::from(".");
                let mut lines = Vec::new();
                let mut preview = String::from("select an entry");
                for (index, entry) in result.entries.into_iter().enumerate() {
                    if index == workspace.files_selection() {
                        selected_name = entry.name.clone();
                        preview = describe_directory_entry(fs, self.files_cwd.as_str(), entry.name.as_str());
                        lines.push(format!("> {}", entry.name));
                    } else {
                        lines.push(format!("  {}", entry.name));
                    }
                }
                (lines.join("\n"), selected_name, preview)
            }
            Err(_) => (
                String::from("filesystem unavailable"),
                String::from("unavailable"),
                String::from("preview unavailable"),
            ),
        };
        self.files_listing = listing;
        self.files_selected_name = selected;
        self.files_preview_text = preview;
        let toolchains = read_fs_text(fs, "/etc/toolchains.toml", "toolchains unavailable");
        let toolchains_rt = read_fs_text(fs, "/run/toolchains/status.toml", "toolchains runtime unavailable");
        self.toolchains_combined_text = format!("config\n{}\n\nruntime\n{}", toolchains, toolchains_rt);
        let network = read_fs_text(fs, "/etc/network.toml", "network unavailable");
        let network_rt = read_fs_text(fs, "/run/network/status.toml", "network runtime unavailable");
        self.network_combined_text = format!("config\n{}\n\nruntime\n{}", network, network_rt);
        self.services_text = read_fs_text(fs, "/system/manifest/services.toml", "services unavailable");
        self.runtime_services_text = read_fs_text(fs, "/run/services/status.toml", "runtime services unavailable");
        self.events_text = read_fs_text(fs, "/run/events.log", "events unavailable");
    }
}

pub(crate) fn draw_desktop_ui(
    summary: &BootSummary,
    framebuffer: &mut Framebuffer,
    compositor: &mut FrameCompositor,
    console: &BootConsole,
    panels: &PanelCache,
    gpu_identity: &str,
    logical_cpus: u32,
    frame_counter: u32,
    app_launch: &mut AppLaunchState,
    chrome: &DesktopChromeState,
    workspace: &DesktopWorkspaceState,
    pointer: &DesktopPointerState,
    shell_env: &ShellEnvironment,
    renderer: &DesktopRenderer,
    scope: RedrawScope,
) {
    compositor.reconfigure(
        framebuffer.size(),
        create_backend(workspace.graphics_backend().label(), framebuffer.info),
    );
    let renderer_diagnostics = compositor.diagnostics();
    let _ = app_launch.activate_by_id(shell_env.active_app());
    let hovered_app_index = hovered_dock_app_index(
        summary.framebuffer_width,
        summary.framebuffer_height,
        pointer.x,
        pointer.y,
    );
    let hovered_surface = desktop_hit_target(
        summary.framebuffer_width,
        summary.framebuffer_height,
        pointer.x,
        pointer.y,
    );
    let history = console.render_history(framebuffer.height().saturating_sub(280) / 28);
    let prompt = console.prompt_text();
    let PanelCache {
        total_text,
        free_text,
        chrome_body: _,
        files_cwd,
        files_listing,
        files_selected_name,
        files_preview_text,
        toolchains_combined_text,
        network_combined_text,
        services_text,
        runtime_services_text,
        events_text,
    } = panels;
    let scene = DesktopScene {
        framebuffer_width: summary.framebuffer_width,
        framebuffer_height: summary.framebuffer_height,
        gpu_identity,
        graphics_backend: renderer_diagnostics.backend_name,
        graphics_api: renderer_diagnostics.api,
        present_mode: renderer_diagnostics.present_mode,
        frames_in_flight: renderer_diagnostics.frames_in_flight,
        logical_cpus,
        volume_total: &total_text,
        volume_free: &free_text,
        status: &console.status,
        files_cwd: &files_cwd,
        files_listing: &files_listing,
        toolchains_text: &toolchains_combined_text,
        network_text: &network_combined_text,
        services_text: &services_text,
        runtime_services_text: &runtime_services_text,
        events_text: &events_text,
        files_selected_name: &files_selected_name,
        files_preview_text: &files_preview_text,
        settings_section: workspace.settings_section().label(),
        active_app: app_launch.active_app(),
        active_app_index: app_launch.active_index(),
        hovered_app_index,
        focused_surface: pointer.focus_target().label(),
        hovered_surface: hovered_surface.label(),
        chrome_panel: chrome.active_panel().label(),
        chrome_body_text: &panels.chrome_body,
        chrome_query: chrome.query(),
        start_hovered: hovered_surface == DesktopFocusTarget::DockStart,
        search_hovered: hovered_surface == DesktopFocusTarget::DockSearch,
        tray_hovered: hovered_surface == DesktopFocusTarget::DockTray,
        start_active: chrome.active_panel() == DesktopChromePanel::Start,
        search_active: chrome.active_panel() == DesktopChromePanel::Search,
        tray_active: chrome.active_panel() == DesktopChromePanel::Tray,
        main_window_focused: pointer.focus_target() == DesktopFocusTarget::MainWindow,
        system_window_focused: pointer.focus_target() == DesktopFocusTarget::SystemWindow,
        preview_window_focused: pointer.focus_target() == DesktopFocusTarget::PreviewWindow,
        pointer_x: pointer.x,
        pointer_y: pointer.y,
        console: DesktopConsoleSnapshot {
            history: &history,
            prompt: &prompt,
            cursor_visible: console.cursor_visible,
        },
    };
    renderer.render(compositor, &scene, frame_counter, scope);
}

pub(crate) fn draw_boot_scene(framebuffer: &mut Framebuffer, summary: &BootSummary) {
    let renderer = DesktopRenderer::new();
    let memory_text = format!("{}", diagnostics::format_bytes(summary.usable_memory_bytes));
    let splash = BootSplash {
        framebuffer_width: summary.framebuffer_width,
        framebuffer_height: summary.framebuffer_height,
        usable_memory: &memory_text,
    };
    renderer.render_boot_scene(framebuffer, &splash);
}

pub(crate) fn build_chrome_panel_body(chrome: &DesktopChromeState) -> String {
    match chrome.active_panel() {
        DesktopChromePanel::Start => {
            let mut out = String::from("Built-in apps\n\n");
            let needle = chrome.query().to_ascii_lowercase();
            let mut any = false;
            for (index, app) in builtin_apps().iter().enumerate() {
                let app_name = app.name.to_ascii_lowercase();
                if needle.is_empty() || app.id.contains(&needle) || app_name.contains(&needle) {
                    any = true;
                    let marker = if chrome.selected_app_index() == Some(index) {
                        '>'
                    } else {
                        ' '
                    };
                    let _ = writeln!(&mut out, "{} {} [{}]", marker, app.name, app.id);
                }
            }
            if !any {
                out.push_str("no apps matched\n");
            }
            out.push_str("\nType to filter. Tab cycles. Enter launches.");
            out
        }
        DesktopChromePanel::Search => {
            let mut out = String::from("Results\n\n");
            let needle = chrome.query().to_ascii_lowercase();
            let mut any = false;
            for (index, app) in builtin_apps().iter().enumerate() {
                let app_name = app.name.to_ascii_lowercase();
                if needle.is_empty() || app.id.contains(&needle) || app_name.contains(&needle) {
                    any = true;
                    let marker = if chrome.selected_app_index() == Some(index) {
                        '>'
                    } else {
                        ' '
                    };
                    let _ = writeln!(&mut out, "{} {} [{}]", marker, app.name, app.id);
                }
            }
            if !any {
                out.push_str("no apps matched\n");
            }
            out.push_str("\nType to search. Backspace edits. Tab cycles. Enter launches.");
            out
        }
        DesktopChromePanel::Tray => String::from(
            "LAN connected\nGPU active\nDesktop compositor live\nNo background services crashed.\n\nPress Enter to close tray.",
        ),
        DesktopChromePanel::None => String::new(),
    }
}

pub(crate) fn desktop_hit_target(
    framebuffer_width: usize,
    framebuffer_height: usize,
    pointer_x: usize,
    pointer_y: usize,
) -> DesktopFocusTarget {
    let pointer = Point::new(pointer_x, pointer_y);
    let terminal_rect = Rect::new(52, 46, framebuffer_width.saturating_sub(420), framebuffer_height.saturating_sub(180));
    let system_rect = Rect::new(terminal_rect.right().saturating_add(18), 62, 300, 248);
    let preview_rect = Rect::new(terminal_rect.right().saturating_add(18), system_rect.bottom().saturating_add(18), 300, 210);
    let dock = Rect::new(framebuffer_width.saturating_sub(620) / 2, framebuffer_height.saturating_sub(102), 620, 66);
    let start = Rect::new(dock.x + 16, dock.y + 11, 92, 40);
    let search = Rect::new(dock.x + 390, dock.y + 11, 90, 40);
    let tray = Rect::new(dock.right().saturating_sub(132), dock.y + 11, 112, 40);

    if start.contains(pointer) {
        return DesktopFocusTarget::DockStart;
    }
    if search.contains(pointer) {
        return DesktopFocusTarget::DockSearch;
    }
    if tray.contains(pointer) {
        return DesktopFocusTarget::DockTray;
    }
    if let Some(index) = hovered_dock_app_index(framebuffer_width, framebuffer_height, pointer_x, pointer_y) {
        return DesktopFocusTarget::DockApp(index);
    }
    if terminal_rect.contains(pointer) {
        return DesktopFocusTarget::MainWindow;
    }
    if system_rect.contains(pointer) {
        return DesktopFocusTarget::SystemWindow;
    }
    if preview_rect.contains(pointer) {
        return DesktopFocusTarget::PreviewWindow;
    }
    DesktopFocusTarget::Desktop
}

pub(crate) fn hovered_dock_app_index(
    framebuffer_width: usize,
    framebuffer_height: usize,
    pointer_x: usize,
    pointer_y: usize,
) -> Option<usize> {
    let dock = Rect::new(
        framebuffer_width.saturating_sub(620) / 2,
        framebuffer_height.saturating_sub(102),
        620,
        66,
    );
    let app_start_x = dock.x + 126;
    let app_spacing = 62;
    let pointer = Point::new(pointer_x, pointer_y);
    builtin_apps().iter().enumerate().find_map(|(index, _)| {
        let origin = Point::new(app_start_x + index * app_spacing, dock.y + 11);
        let hit = Rect::new(origin.x.saturating_sub(8), origin.y.saturating_sub(6), 56, 52);
        if hit.contains(pointer) {
            Some(index)
        } else {
            None
        }
    })
}

/// Resolve the *effective* backend info for a user preference: if the
/// preferred engine is not registered by a driver, the software backend
/// info is reported instead.
pub(crate) fn effective_backend_info(preference: GraphicsBackend) -> orbita_video::BackendInfo {
    orbita_video::backend_info(preference.label())
}
