use orbita_core::{BuiltinAppIcon, builtin_apps};
use orbita_video::{
    Color, CursorKind, CursorStyle, DesktopTheme, FrameCompositor, Framebuffer, GuiCanvas,
    Insets, OwnedImage, Point, Rect, SoftwareFramebuffer, TextAlign, TextStyle, TextWrap,
    inside_rounded_rect,
};

use crate::model::{BootSplash, DesktopScene};

/// How much of the screen a redraw has to repaint. Carried explicitly so
/// the kernel can keep keystrokes cheap: static chrome survives in the
/// compositor's back buffer and only the affected region is redrawn.
#[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub enum RedrawScope {
    /// Prompt line + cursor only (cursor blink).
    Prompt,
    /// Terminal contents over cached chrome (typed input, new output).
    Full,
    /// Everything including background and window chrome (first frame,
    /// mode/app switches).
    Chrome,
}

pub struct DesktopRenderer {
    theme: DesktopTheme,
    #[allow(dead_code)]
    assets: DesktopAssets,
}

impl DesktopRenderer {
    pub fn new() -> Self {
        Self {
            theme: DesktopTheme::aurora(),
            assets: DesktopAssets::new(),
        }
    }

    pub fn render_boot_scene(&self, framebuffer: &mut Framebuffer, splash: &BootSplash<'_>) {
        framebuffer.draw_gradient(self.theme.wallpaper_top, self.theme.wallpaper_bottom);
        let screen = Rect::new(0, 0, splash.framebuffer_width, splash.framebuffer_height);
        let main_window = Rect::new(
            52,
            40,
            splash.framebuffer_width.saturating_sub(396),
            splash.framebuffer_height.saturating_sub(174),
        );
        let side_window = Rect::new(main_window.right().saturating_add(20), 52, 324, 284);
        let lower_window = Rect::new(main_window.right().saturating_add(20), 356, 324, 220);
        let dock = Rect::new(
            splash.framebuffer_width.saturating_sub(700) / 2,
            splash.framebuffer_height.saturating_sub(98),
            700,
            68,
        );

        for radius in [300usize, 220, 140] {
            framebuffer.draw_circle(
                Point::new(splash.framebuffer_width / 2, splash.framebuffer_height / 4),
                radius,
                Color::rgb(170, 220, 255),
            );
        }

        fill_rounded_rect_fb(framebuffer, main_window, 26, Color::rgb(18, 30, 56));
        fill_rounded_rect_fb(framebuffer, side_window, 26, Color::rgb(24, 38, 68));
        fill_rounded_rect_fb(framebuffer, lower_window, 26, Color::rgb(24, 38, 68));
        fill_rounded_rect_fb(framebuffer, dock, 30, Color::rgba(34, 52, 92, 255));
        stroke_rounded_rect_fb(framebuffer, main_window, 26, Color::rgb(216, 234, 255));
        stroke_rounded_rect_fb(framebuffer, side_window, 26, Color::rgb(216, 234, 255));
        stroke_rounded_rect_fb(framebuffer, lower_window, 26, Color::rgb(216, 234, 255));
        stroke_rounded_rect_fb(framebuffer, dock, 30, Color::rgb(236, 244, 255));

        {
            let mut back = FrameCompositor::new(
                orbita_video::Size::new(screen.width, screen.height),
                alloc::boxed::Box::new(SoftwareFramebuffer::new(framebuffer.info)),
            );
            {
                let mut canvas = back.canvas();
                let title_bar = Rect::new(
                    main_window.x + 1,
                    main_window.y + 1,
                    main_window.width.saturating_sub(2),
                    48,
                );
                canvas.fill_rounded_rect(title_bar, 24, Color::rgb(67, 127, 232));
                let panel = Rect::new(
                    main_window.x + 18,
                    main_window.y + 68,
                    main_window.width.saturating_sub(36),
                    main_window.height.saturating_sub(92),
                );
                canvas.fill_rounded_rect(panel, 18, Color::rgb(8, 16, 30));
                canvas.stroke_rounded_rect(panel, 18, 1, Color::rgb(76, 104, 152));
                draw_window_controls(&mut canvas, main_window);

                let mut title_style = TextStyle::monospace(Color::rgb(244, 248, 255));
                title_style.scale = 2;
                canvas.draw_text(
                    Point::new(main_window.x + 20, main_window.y + 14),
                    "Orbita Desktop",
                    title_style,
                );

                let mut body_style = TextStyle::monospace(Color::rgb(220, 230, 244));
                body_style.wrap = TextWrap::Character;
                canvas.draw_text(
                    Point::new(main_window.x + 30, main_window.y + 82),
                    &alloc::format!(
                        "Desktop shell\nFramebuffer {}x{}\nMemory {}\n\nRounded windows, visible cursor,\nfloating taskbar and live compositor path.",
                        splash.framebuffer_width,
                        splash.framebuffer_height,
                        splash.usable_memory
                    ),
                    body_style,
                );

                let side_style = TextStyle::monospace(Color::rgb(232, 238, 248));
                canvas.draw_text(Point::new(side_window.x + 18, side_window.y + 18), "System", side_style);
                canvas.draw_text(Point::new(side_window.x + 18, side_window.y + 58), "GPU active", side_style);
                canvas.draw_text(Point::new(side_window.x + 18, side_window.y + 78), "Windows-like controls", side_style);
                canvas.draw_text(Point::new(side_window.x + 18, side_window.y + 98), "Rounded windows", side_style);
                canvas.draw_text(Point::new(side_window.x + 18, side_window.y + 118), "Glass taskbar", side_style);
                canvas.draw_text(Point::new(side_window.x + 18, side_window.y + 138), "Visible cursor", side_style);

                canvas.draw_text(Point::new(lower_window.x + 18, lower_window.y + 18), "Taskbar", side_style);
                let start_pill = Rect::new(dock.x + 18, dock.y + 14, 96, 40);
                canvas.fill_rounded_rect(start_pill, 20, Color::rgb(102, 150, 234));
                for index in 0..5 {
                    let icon = Rect::new(dock.x + 132 + index * 86, dock.y + 14, 38, 38);
                    canvas.fill_rounded_rect(
                        icon,
                        10,
                        Color::rgb(72 + (index as u8 * 16), 152, 228),
                    );
                    canvas.stroke_rounded_rect(icon, 10, 1, Color::rgb(232, 244, 255));
                }
                let tray = Rect::new(dock.right().saturating_sub(130), dock.y + 14, 110, 40);
                canvas.fill_rounded_rect(tray, 20, Color::rgb(70, 106, 178));
                canvas.draw_cursor(
                    Point::new(dock.right().saturating_sub(140), dock.y.saturating_sub(48)),
                    CursorStyle::mac_like(),
                    128,
                );
            }
            back.present();
        }
    }

    /// Draws the scene with an explicit redraw scope.
    ///
    /// * [`RedrawScope::Chrome`] — repaints everything (background,
    ///   window chrome, contents). Needed on the first frame and after
    ///   app/mode switches.
    /// * [`RedrawScope::Full`] — repaints only the terminal contents over
    ///   the cached chrome. Fast path for typed input.
    /// * [`RedrawScope::Prompt`] — repaints just the prompt line + cursor.
    ///   Fastest path, used for cursor blinking.
    ///
    /// The back buffer inside the compositor persists between calls, so
    /// the chrome survives `Full`/`Prompt` redraws untouched.
    pub fn render(
        &self,
        compositor: &mut FrameCompositor,
        scene: &DesktopScene<'_>,
        frame_counter: u32,
        scope: RedrawScope,
    ) {
        let size = compositor.size();
        let screen = Rect::new(0, 0, size.width, size.height);
        let phase = animation_phase(frame_counter, 4);
        let terminal_rect = Rect::new(20, 20, screen.width.saturating_sub(40), screen.height.saturating_sub(40));

        let dirty = match scope {
            RedrawScope::Chrome => screen,
            RedrawScope::Full => terminal_rect,
            RedrawScope::Prompt => {
                let body = terminal_rect.inset(Insets::new(20, 54, 20, 22)).unwrap_or(terminal_rect);
                Rect::new(body.x + 12, body.bottom().saturating_sub(52), body.width.saturating_sub(24), 34)
            }
        };
        match scope {
            RedrawScope::Chrome => {
                let mut canvas = compositor.canvas();
                canvas.clear(Color::rgb(5, 14, 20));
                canvas.fill_gradient(screen, Color::rgb(6, 20, 28), Color::rgb(4, 11, 16));
                draw_window(
                    &mut canvas,
                    terminal_rect,
                    "Orbita Console",
                    &self.theme,
                    Color::rgba(124, 242, 205, 255),
                    true,
                );
                draw_terminal_contents(&mut canvas, terminal_rect, scene, phase, &self.theme);
            }
            RedrawScope::Full => {
                let mut canvas = compositor.canvas();
                draw_terminal_contents(&mut canvas, terminal_rect, scene, phase, &self.theme);
            }
            RedrawScope::Prompt => {
                let mut canvas = compositor.canvas();
                draw_prompt_only(&mut canvas, terminal_rect, scene, phase, &self.theme);
            }
        }
        compositor.present_region(dirty);
    }
}

struct DesktopAssets {
    orbita_logo: OwnedImage,
    folder_icon: OwnedImage,
    terminal_icon: OwnedImage,
    monitor_icon: OwnedImage,
    settings_icon: OwnedImage,
}

impl DesktopAssets {
    fn new() -> Self {
        Self {
            orbita_logo: build_orbita_logo(),
            folder_icon: build_folder_icon(),
            terminal_icon: build_terminal_icon(),
            monitor_icon: build_monitor_icon(),
            settings_icon: build_settings_icon(),
        }
    }

    fn icon_for(&self, icon: BuiltinAppIcon) -> &OwnedImage {
        match icon {
            BuiltinAppIcon::Planet => &self.orbita_logo,
            BuiltinAppIcon::Terminal => &self.terminal_icon,
            BuiltinAppIcon::Folder => &self.folder_icon,
            BuiltinAppIcon::Settings => &self.settings_icon,
            BuiltinAppIcon::Monitor => &self.monitor_icon,
        }
    }
}

#[allow(dead_code)]
fn draw_wallpaper_mesh(canvas: &mut GuiCanvas<'_>, screen: Rect, phase: u8) {
    let line_color = Color::rgba(255, 255, 255, 18);
    let spacing = 72usize;
    let offset = wave_offset(phase, 18);
    let mut x = 0usize;
    while x < screen.width {
        canvas.fill_rounded_rect(
            Rect::new(x.saturating_add(offset), screen.height / 2, 1, screen.height / 2),
            1,
            line_color,
        );
        x += spacing;
    }
}

#[allow(dead_code)]
fn draw_wallpaper_ribbons(canvas: &mut GuiCanvas<'_>, screen: Rect, phase: u8) {
    let ribbon_a = Rect::new(screen.width / 6, screen.height / 6 + wave_offset(phase, 16), screen.width / 2, 118);
    let ribbon_b = Rect::new(screen.width / 3, screen.height / 3 + wave_offset(phase.wrapping_add(48), 18), screen.width / 2, 132);
    canvas.glass_panel(
        ribbon_a,
        58,
        Color::rgba(255, 255, 255, 14),
        Color::rgba(180, 212, 255, 30),
        Color::rgba(255, 255, 255, 18),
    );
    canvas.glass_panel(
        ribbon_b,
        64,
        Color::rgba(146, 193, 255, 12),
        Color::rgba(84, 136, 244, 26),
        Color::rgba(255, 255, 255, 16),
    );
}

fn draw_window(
    canvas: &mut GuiCanvas<'_>,
    rect: Rect,
    title: &str,
    theme: &DesktopTheme,
    accent: Color,
    focused: bool,
) {
    let shadow = if focused {
        Color::rgba(0, 0, 0, 72)
    } else {
        Color::rgba(0, 0, 0, 56)
    };
    canvas.shadow(rect, 24, 18, shadow);
    canvas.glass_panel(rect, 22, theme.window_fill, theme.window_fill_2, theme.window_border);
    let title_bar = Rect::new(rect.x + 1, rect.y + 1, rect.width.saturating_sub(2), 46);
    let title_top = if focused {
        Color::rgba(96, 166, 255, 228)
    } else {
        Color::rgba(87, 144, 255, 196)
    };
    let title_bottom = if focused {
        Color::rgba(24, 92, 200, 184)
    } else {
        Color::rgba(34, 70, 158, 136)
    };
    canvas.fill_gradient(title_bar, title_top, title_bottom);
    canvas.fill_rect(
        Rect::new(rect.x + 18, rect.y + 44, rect.width.saturating_sub(36), 1),
        accent.with_alpha(120),
    );
    let border = if focused {
        accent.with_alpha(170)
    } else {
        theme.window_border
    };
    canvas.stroke_rounded_rect(rect, 22, 1, border);
    let mut title_style = TextStyle::monospace(theme.title_text);
    title_style.scale = 2;
    canvas.draw_text(Point::new(rect.x + 18, rect.y + 15), title, title_style);
    draw_window_controls(canvas, rect);
}

fn draw_window_controls(canvas: &mut GuiCanvas<'_>, rect: Rect) {
    let controls_y = rect.y + 9;
    let close = Rect::new(rect.right().saturating_sub(52), controls_y, 34, 24);
    let max = Rect::new(rect.right().saturating_sub(88), controls_y, 34, 24);
    let min = Rect::new(rect.right().saturating_sub(124), controls_y, 34, 24);
    canvas.fill_rounded_rect(min, 7, Color::rgba(255, 255, 255, 28));
    canvas.fill_rounded_rect(max, 7, Color::rgba(255, 255, 255, 28));
    canvas.fill_rounded_rect(close, 7, Color::rgba(229, 77, 66, 220));
    canvas.stroke_rounded_rect(min, 7, 1, Color::rgba(255, 255, 255, 44));
    canvas.stroke_rounded_rect(max, 7, 1, Color::rgba(255, 255, 255, 44));
    canvas.stroke_rounded_rect(close, 7, 1, Color::rgba(255, 255, 255, 32));
    let symbol = Color::rgba(255, 255, 255, 180);
    canvas.fill_rect(Rect::new(min.x + 10, min.y + 15, 13, 1), symbol);
    canvas.stroke_rounded_rect(Rect::new(max.x + 10, max.y + 7, 12, 9), 2, 1, symbol);
    canvas.fill_rect(Rect::new(close.x + 10, close.y + 7, 12, 1), symbol);
    canvas.fill_rect(Rect::new(close.x + 10, close.y + 16, 12, 1), symbol);
}

fn draw_prompt_only(canvas: &mut GuiCanvas<'_>, window: Rect, scene: &DesktopScene<'_>, phase: u8, theme: &DesktopTheme) {
    let body = window.inset(Insets::new(20, 54, 20, 22)).unwrap_or(window);
    let prompt_box = Rect::new(body.x + 14, body.bottom().saturating_sub(50), body.width.saturating_sub(28), 30);
    // Repaint the prompt pill opaquely so the previous frame's glyphs
    // cannot ghost through.
    canvas.fill_rounded_rect(prompt_box, 12, Color::rgb(14, 24, 31));
    canvas.stroke_rounded_rect(prompt_box, 12, 1, Color::rgba(255, 255, 255, 14));
    let prompt_color = if scene.console.cursor_visible {
        Color::rgb(122, 243, 196)
    } else {
        Color::rgb(88, 186, 156)
    };
    let mut prompt_style = TextStyle::monospace(prompt_color);
    prompt_style.scale = 2;
    canvas.draw_text(
        Point::new(prompt_box.x + 16, prompt_box.y + 7),
        scene.console.prompt,
        prompt_style,
    );
    let glyph_advance = 17usize;
    let beam_x = prompt_box.x + 16 + scene.console.prompt.chars().count().saturating_mul(glyph_advance);
    canvas.draw_cursor(
        Point::new(beam_x.min(prompt_box.right().saturating_sub(10)), prompt_box.y + 5),
        CursorStyle {
            kind: CursorKind::Beam,
            fill: prompt_color.with_alpha(180u8.saturating_add(phase / 3)),
            outline: Color::rgba(255, 255, 255, 28),
            shadow: Color::rgba(0, 0, 0, 0),
        },
        phase,
    );
    let _ = theme;
}

fn draw_terminal_contents(canvas: &mut GuiCanvas<'_>, rect: Rect, scene: &DesktopScene<'_>, phase: u8, theme: &DesktopTheme) {
    let body = rect.inset(Insets::new(20, 54, 20, 22)).unwrap_or(rect);
    let header = Rect::new(body.x + 14, body.y + 14, body.width.saturating_sub(28), 34);
    let prompt_box = Rect::new(body.x + 14, body.bottom().saturating_sub(50), body.width.saturating_sub(28), 30);
    canvas.fill_rounded_rect(body, 18, Color::rgb(7, 13, 18));
    canvas.stroke_rounded_rect(body, 18, 1, Color::rgba(148, 239, 214, 34));
    canvas.fill_rounded_rect(header, 12, Color::rgb(19, 31, 40));
    canvas.stroke_rounded_rect(header, 12, 1, Color::rgba(255, 255, 255, 14));
    let mut header_style = TextStyle::monospace(Color::rgb(207, 230, 238));
    header_style.scale = 1;
    canvas.draw_text(
        Point::new(header.x + 14, header.y + 11),
        &alloc::format!("rendered console  |  {}", scene.status),
        header_style,
    );

    let mut history_style = TextStyle::monospace(theme.body_text);
    history_style.scale = 2;
    history_style.line_spacing = 4;
    history_style.wrap = TextWrap::Character;
    canvas.draw_text(Point::new(body.x + 18, body.y + 58), scene.console.history, history_style);

    // The prompt pill is drawn AFTER the history text: if wrapped lines
    // run long, the opaque pill covers them instead of overlapping.
    canvas.fill_rounded_rect(prompt_box, 12, Color::rgb(14, 24, 31));
    canvas.stroke_rounded_rect(prompt_box, 12, 1, Color::rgba(255, 255, 255, 14));
    canvas.fill_rect(
        Rect::new(body.x + 14, prompt_box.y.saturating_sub(14), body.width.saturating_sub(28), 1),
        Color::rgba(92, 126, 138, 52),
    );
    let prompt_color = if scene.console.cursor_visible {
        Color::rgb(122, 243, 196)
    } else {
        Color::rgb(88, 186, 156)
    };
    let mut prompt_style = TextStyle::monospace(prompt_color);
    prompt_style.scale = 2;
    canvas.draw_text(
        Point::new(prompt_box.x + 16, prompt_box.y + 7),
        scene.console.prompt,
        prompt_style,
    );
    let glyph_advance = 17usize;
    let beam_x = prompt_box.x + 16 + scene.console.prompt.chars().count().saturating_mul(glyph_advance);
    canvas.draw_cursor(
        Point::new(beam_x.min(prompt_box.right().saturating_sub(10)), prompt_box.y + 5),
        CursorStyle {
            kind: CursorKind::Beam,
            fill: prompt_color.with_alpha(180u8.saturating_add(phase / 3)),
            outline: Color::rgba(255, 255, 255, 28),
            shadow: Color::rgba(0, 0, 0, 0),
        },
        phase,
    );
}

#[allow(dead_code)]
fn draw_files_contents(canvas: &mut GuiCanvas<'_>, rect: Rect, scene: &DesktopScene<'_>, theme: &DesktopTheme) {
    let body = rect.inset(Insets::new(18, 52, 18, 20)).unwrap_or(rect);
    canvas.fill_rounded_rect(body, 16, Color::rgba(12, 16, 26, 174));
    canvas.stroke_rounded_rect(body, 16, 1, Color::rgba(255, 255, 255, 14));
    let sidebar = Rect::new(body.x + 10, body.y + 10, 150, body.height.saturating_sub(20));
    canvas.fill_rounded_rect(sidebar, 14, Color::rgba(255, 255, 255, 16));
    let content = Rect::new(sidebar.right().saturating_add(12), body.y + 10, body.width.saturating_sub(sidebar.width + 32), body.height.saturating_sub(20));
    let listing = Rect::new(content.x, content.y, content.width / 2, content.height);
    let preview = Rect::new(listing.right().saturating_add(10), content.y, content.width.saturating_sub(listing.width + 10), content.height);
    canvas.fill_rounded_rect(content, 14, Color::rgba(255, 255, 255, 10));
    canvas.fill_rounded_rect(listing, 14, Color::rgba(255, 255, 255, 8));
    canvas.fill_rounded_rect(preview, 14, Color::rgba(255, 255, 255, 8));
    let side_style = TextStyle::monospace(Color::rgb(224, 232, 244));
    canvas.draw_text(Point::new(sidebar.x + 16, sidebar.y + 18), "Places\n/\n/home\n/etc\n/system\n/var", side_style);
    let mut listing_style = TextStyle::monospace(theme.body_text);
    listing_style.wrap = TextWrap::Character;
    let text = alloc::format!(
        "Directory\n{}\nselected: {}\n\n{}",
        scene.files_cwd,
        scene.files_selected_name,
        scene.files_listing
    );
    canvas.draw_text(Point::new(listing.x + 18, listing.y + 18), &text, listing_style);
    let preview_text = alloc::format!(
        "Preview\n{}\n\nShortcuts\nEnter open\nTab next\na parent\nn new note\nm new dir\nc copy\nd delete",
        scene.files_preview_text
    );
    canvas.draw_text(Point::new(preview.x + 18, preview.y + 18), &preview_text, listing_style);
}

#[allow(dead_code)]
fn draw_settings_contents(canvas: &mut GuiCanvas<'_>, rect: Rect, scene: &DesktopScene<'_>, theme: &DesktopTheme) {
    let body = rect.inset(Insets::new(18, 52, 18, 20)).unwrap_or(rect);
    canvas.fill_rounded_rect(body, 16, Color::rgba(12, 16, 26, 174));
    canvas.stroke_rounded_rect(body, 16, 1, Color::rgba(255, 255, 255, 14));
    let header = Rect::new(body.x + 16, body.y + 16, body.width.saturating_sub(32), 56);
    canvas.glass_panel(
        header,
        16,
        Color::rgba(255, 255, 255, 16),
        Color::rgba(136, 188, 255, 30),
        Color::rgba(255, 255, 255, 22),
    );
    let title_style = TextStyle::monospace(Color::rgb(244, 248, 255));
    canvas.draw_text(Point::new(header.x + 18, header.y + 20), "Orbita Settings", title_style);
    let mut body_style = TextStyle::monospace(theme.body_text);
    body_style.wrap = TextWrap::Character;
    let text = match scene.settings_section {
        "network" => alloc::format!(
            "Section: network\n\n{}\n\nShortcuts\nTab or Enter next section\nw toggle network stack",
            scene.network_text
        ),
        "services" => alloc::format!(
            "Section: services\n\n{}\n\nShortcuts\nTab or Enter next section\ns rotate section",
            scene.services_text
        ),
        _ => alloc::format!(
            "Section: toolchains\n\n{}\n\nGraphics\nbackend: {}\napi: {}\npresent: {}\nframes in flight: {}\n\nInstall from terminal\npkg install python3 nodejs rust build-essential\napt install python3 nodejs rust build-essential\n\nShortcuts\nTab or Enter next section\nt show install hint\ng rotate graphics backend",
            scene.toolchains_text,
            scene.graphics_backend,
            scene.graphics_api,
            scene.present_mode,
            scene.frames_in_flight
        ),
    };
    canvas.draw_text(Point::new(body.x + 18, body.y + 92), &text, body_style);
}

#[allow(dead_code)]
fn draw_monitor_contents(canvas: &mut GuiCanvas<'_>, rect: Rect, scene: &DesktopScene<'_>, theme: &DesktopTheme) {
    let body = rect.inset(Insets::new(18, 52, 18, 20)).unwrap_or(rect);
    canvas.fill_rounded_rect(body, 16, Color::rgba(12, 16, 26, 174));
    canvas.stroke_rounded_rect(body, 16, 1, Color::rgba(255, 255, 255, 14));
    let cards = [
        Rect::new(body.x + 16, body.y + 16, body.width / 2 - 24, 82),
        Rect::new(body.x + body.width / 2 + 8, body.y + 16, body.width / 2 - 24, 82),
        Rect::new(body.x + 16, body.y + 112, body.width.saturating_sub(32), body.height.saturating_sub(128)),
    ];
    for card in cards {
        canvas.glass_panel(
            card,
            16,
            Color::rgba(255, 255, 255, 14),
            Color::rgba(126, 166, 246, 24),
            Color::rgba(255, 255, 255, 18),
        );
    }
    let style = TextStyle::monospace(theme.body_text);
    let runtime_services = summarize_lines(scene.runtime_services_text, 8, 44);
    let events = summarize_lines(scene.events_text, 10, 44);
    let services = summarize_lines(scene.services_text, 6, 44);
    canvas.draw_text(
        Point::new(cards[0].x + 18, cards[0].y + 18),
        &alloc::format!(
            "GPU\n{}\n{}\n{}",
            scene.gpu_identity, scene.graphics_backend, scene.graphics_api
        ),
        style,
    );
    canvas.draw_text(Point::new(cards[1].x + 18, cards[1].y + 18), &alloc::format!("CPU\n{} logical", scene.logical_cpus), style);
    canvas.draw_text(
        Point::new(cards[2].x + 18, cards[2].y + 18),
        &alloc::format!(
            "Volume total: {}\nVolume free: {}\nStatus: {}\nActive app: {}\nFocus: {}\nHover: {}\nPointer: {},{}\nRenderer: {}\nPresent: {}\nFrames in flight: {}\n\nDesktop Session\nframe/live app tracked in /run/desktop/session.toml\n\nRuntime Services\n{}\nEvents\n{}\nService Catalog\n{}\n\nShortcuts\np snapshot monitor\nl log heartbeat",
            scene.volume_total,
            scene.volume_free,
            scene.status,
            scene.active_app.name,
            scene.focused_surface,
            scene.hovered_surface,
            scene.pointer_x,
            scene.pointer_y,
            scene.graphics_backend,
            scene.present_mode,
            scene.frames_in_flight,
            runtime_services,
            events,
            services
        ),
        style,
    );
}

#[allow(dead_code)]
fn draw_active_app_contents(canvas: &mut GuiCanvas<'_>, rect: Rect, scene: &DesktopScene<'_>, phase: u8, theme: &DesktopTheme) {
    match scene.active_app.id {
        "files" => draw_files_contents(canvas, rect, scene, theme),
        "settings" => draw_settings_contents(canvas, rect, scene, theme),
        "monitor" => draw_monitor_contents(canvas, rect, scene, theme),
        _ => draw_terminal_contents(canvas, rect, scene, phase, theme),
    }
}

#[allow(dead_code)]
fn draw_system_window(canvas: &mut GuiCanvas<'_>, rect: Rect, scene: &DesktopScene<'_>, theme: &DesktopTheme) {
    let body = rect.inset(Insets::new(18, 54, 18, 18)).unwrap_or(rect);
    canvas.fill_rounded_rect(body, 18, Color::rgba(6, 14, 30, 110));
    canvas.stroke_rounded_rect(body, 18, 1, Color::rgba(255, 255, 255, 16));
    let mut style = TextStyle::monospace(theme.body_text);
    style.wrap = TextWrap::Character;
    let content = alloc::format!(
        "Framebuffer\n{}x{}\n\nGPU\n{}\nbackend: {}\napi: {}\npresent: {}\nframes: {}\n\nCPU\n{} logical\n\nVolume\n{} total\n{} free\n\nFocus\n{}\nHover\n{}\n\nStatus\n{}",
        scene.framebuffer_width,
        scene.framebuffer_height,
        scene.gpu_identity,
        scene.graphics_backend,
        scene.graphics_api,
        scene.present_mode,
        scene.frames_in_flight,
        scene.logical_cpus,
        scene.volume_total,
        scene.volume_free,
        scene.focused_surface,
        scene.hovered_surface,
        scene.status
    );
    canvas.draw_text(Point::new(body.x + 6, body.y + 6), &content, style);
}

#[allow(dead_code)]
fn summarize_lines(text: &str, max_lines: usize, max_columns: usize) -> alloc::string::String {
    let mut out = alloc::string::String::new();
    let mut count = 0usize;
    for line in text.lines() {
        if count == max_lines {
            out.push_str("...");
            break;
        }
        let mut trimmed = alloc::string::String::new();
        for ch in line.chars().take(max_columns) {
            trimmed.push(ch);
        }
        if line.chars().count() > max_columns {
            trimmed.push_str("...");
        }
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(trimmed.as_str());
        count += 1;
    }
    if out.is_empty() {
        out.push_str("(empty)");
    }
    out
}

#[allow(dead_code)]
fn draw_preview_window(
    canvas: &mut GuiCanvas<'_>,
    rect: Rect,
    assets: &DesktopAssets,
    scene: &DesktopScene<'_>,
    phase: u8,
    theme: &DesktopTheme,
) {
    let body = rect.inset(Insets::new(16, 50, 16, 16)).unwrap_or(rect);
    canvas.fill_rounded_rect(body, 18, Color::rgba(9, 18, 34, 104));
    canvas.fill_gradient(body, Color::rgba(38, 66, 126, 162), Color::rgba(11, 18, 36, 146));
    canvas.fill_radial_glow(Point::new(body.x + body.width / 2, body.y + body.height / 3), 60, Color::rgba(106, 226, 255, 42));
    let apps = builtin_apps();
    canvas.blit_image(
        Point::new(body.x + 18, body.y + 18),
        assets.icon_for(scene.active_app.icon).as_view(),
    );
    if let Some(second) = apps.get((scene.active_app_index + 1) % apps.len().max(1)) {
        canvas.blit_image(Point::new(body.x + 92, body.y + 18), assets.icon_for(second.icon).as_view());
    }
    let title = TextStyle::monospace(theme.body_text);
    let preview_text = if apps.len() >= 3 {
        alloc::format!(
            "active: {}\nmodule: {}\nnext: {}\nfocus: {}\nhover: {}\nstatus: {}",
            scene.active_app.name,
            scene.active_app.module,
            apps[(scene.active_app_index + 1) % apps.len()].name,
            scene.focused_surface,
            scene.hovered_surface,
            scene.status
        )
    } else {
        alloc::format!("Orbita shell\nrounded windows\nfloating taskbar\nhigh refresh redraw")
    };
    canvas.draw_text(Point::new(body.x + 18, body.y + 110), &preview_text, title);
    let highlight = Rect::new(body.x + 170 + wave_offset(phase, 5), body.y + 36, 78, 54);
    canvas.glass_panel(
        highlight,
        14,
        Color::rgba(255, 255, 255, 26),
        Color::rgba(138, 186, 255, 52),
        Color::rgba(255, 255, 255, 38),
    );
}

#[allow(dead_code)]
fn draw_dock(
    canvas: &mut GuiCanvas<'_>,
    rect: Rect,
    assets: &DesktopAssets,
    scene: &DesktopScene<'_>,
    phase: u8,
    theme: &DesktopTheme,
) {
    canvas.shadow(rect, 28, 18, Color::rgba(0, 0, 0, 48));
    canvas.glass_panel(rect, 28, theme.dock_tint_top, theme.dock_tint_bottom, theme.dock_border);
    let start_pill = Rect::new(rect.x + 16, rect.y + 11, 92, 40);
    let app_start_x = rect.x + 126;
    let app_spacing = 62;
    let start_fill = if scene.start_active {
        Color::rgba(106, 210, 255, 88)
    } else if scene.start_hovered {
        Color::rgba(138, 230, 255, 58)
    } else {
        Color::rgba(255, 255, 255, 42)
    };
    canvas.fill_rounded_rect(start_pill, 20, start_fill);
    canvas.stroke_rounded_rect(start_pill, 20, 1, Color::rgba(255, 255, 255, 54));
    let icon_y = rect.y + 11;
    let apps = builtin_apps();
    let phase_step = 18u8;
    for (index, app) in apps.iter().enumerate() {
        let offset_phase = phase.wrapping_add((index as u8).wrapping_mul(phase_step));
        let position = Point::new(
            app_start_x + index * app_spacing,
            icon_y + wave_offset(offset_phase, 4),
        );
        if scene.active_app_index == index || scene.hovered_app_index == Some(index) {
            let highlight = Rect::new(position.x.saturating_sub(8), position.y.saturating_sub(6), 56, 52);
            let fill = if scene.active_app_index == index {
                Color::rgba(255, 255, 255, 24)
            } else {
                Color::rgba(126, 220, 255, 22)
            };
            canvas.fill_rounded_rect(highlight, 18, fill);
            canvas.stroke_rounded_rect(highlight, 18, 1, Color::rgba(255, 255, 255, 40));
        }
        canvas.blit_image(position, assets.icon_for(app.icon).as_view());
    }
    canvas.draw_text(Point::new(start_pill.x + 18, start_pill.y + 14), "Start", TextStyle::monospace(Color::rgb(250, 252, 255)));
    for (index, app) in apps.iter().enumerate() {
        let mut style = TextStyle::monospace(Color::rgb(241, 244, 248));
        style.align = TextAlign::Center;
        canvas.draw_text(Point::new(app_start_x.saturating_sub(8) + index * app_spacing, rect.y + 44), app.name, style);
    }
    let search = Rect::new(rect.x + 390, rect.y + 11, 90, 40);
    let search_fill = if scene.search_active {
        Color::rgba(106, 210, 255, 72)
    } else if scene.search_hovered {
        Color::rgba(138, 230, 255, 40)
    } else {
        Color::rgba(255, 255, 255, 22)
    };
    canvas.fill_rounded_rect(search, 20, search_fill);
    canvas.stroke_rounded_rect(search, 20, 1, Color::rgba(255, 255, 255, 32));
    canvas.draw_text(Point::new(search.x + 18, search.y + 14), "Search", TextStyle::monospace(Color::rgba(245, 248, 255, 196)));
    let tray = Rect::new(rect.right().saturating_sub(132), rect.y + 11, 112, 40);
    let tray_fill = if scene.tray_active {
        Color::rgba(106, 210, 255, 72)
    } else if scene.tray_hovered {
        Color::rgba(138, 230, 255, 42)
    } else {
        Color::rgba(255, 255, 255, 24)
    };
    canvas.fill_rounded_rect(tray, 20, tray_fill);
    canvas.stroke_rounded_rect(tray, 20, 1, Color::rgba(255, 255, 255, 30));
    canvas.draw_text(Point::new(tray.x + 16, tray.y + 14), "12:45  LAN", TextStyle::monospace(Color::rgb(248, 250, 255)));

    let mut hint = TextStyle::monospace(Color::rgba(244, 248, 255, 188));
    hint.align = TextAlign::Center;
    canvas.draw_text(
        Point::new(rect.x + rect.width / 2, rect.y.saturating_sub(18)),
        scene.hovered_surface,
        hint,
    );
}

#[allow(dead_code)]
fn draw_chrome_panel(
    canvas: &mut GuiCanvas<'_>,
    screen: Rect,
    scene: &DesktopScene<'_>,
    theme: &DesktopTheme,
) {
    let panel_rect = match scene.chrome_panel {
        "start" => Some(Rect::new(screen.width.saturating_sub(320) / 2, screen.height.saturating_sub(438), 320, 300)),
        "search" => Some(Rect::new(screen.width.saturating_sub(360) / 2, screen.height.saturating_sub(320), 360, 182)),
        "tray" => Some(Rect::new(screen.width.saturating_sub(268), screen.height.saturating_sub(298), 228, 160)),
        _ => None,
    };
    let Some(rect) = panel_rect else {
        return;
    };

    canvas.shadow(rect, 26, 18, Color::rgba(0, 0, 0, 58));
    canvas.glass_panel(
        rect,
        24,
        Color::rgba(18, 30, 48, 166),
        Color::rgba(44, 74, 124, 148),
        Color::rgba(255, 255, 255, 26),
    );
    let title = match scene.chrome_panel {
        "start" => "Start",
        "search" => "Search",
        "tray" => "Quick Tray",
        _ => "",
    };
    let mut title_style = TextStyle::monospace(Color::rgb(244, 248, 255));
    title_style.scale = 2;
    canvas.draw_text(Point::new(rect.x + 18, rect.y + 18), title, title_style);
    let mut body_style = TextStyle::monospace(theme.body_text);
    body_style.wrap = TextWrap::Character;
    if scene.chrome_panel == "search" {
        let query_box = Rect::new(rect.x + 18, rect.y + 56, rect.width.saturating_sub(36), 36);
        canvas.fill_rounded_rect(query_box, 16, Color::rgba(255, 255, 255, 16));
        canvas.stroke_rounded_rect(query_box, 16, 1, Color::rgba(255, 255, 255, 26));
        let query_text = alloc::format!("query: {}", scene.chrome_query);
        canvas.draw_text(Point::new(query_box.x + 14, query_box.y + 12), &query_text, TextStyle::monospace(Color::rgb(248, 250, 255)));
        canvas.draw_text(Point::new(rect.x + 18, rect.y + 104), scene.chrome_body_text, body_style);
    } else {
        canvas.draw_text(Point::new(rect.x + 18, rect.y + 70), scene.chrome_body_text, body_style);
    }
}

fn build_orbita_logo() -> OwnedImage {
    let mut image = OwnedImage::new(40, 40, Color::TRANSPARENT);
    for y in 0..40 {
        for x in 0..40 {
            let dx = x as isize - 20;
            let dy = y as isize - 20;
            let distance = dx * dx + dy * dy;
            if distance < 144 {
                let mix = ((x + y) * 255 / 80) as u8;
                image.set_pixel(x, y, Color::rgb(66, 220, 220).lerp(Color::rgb(22, 140, 255), mix));
            } else if distance < 168 && dy.abs() < 10 {
                image.set_pixel(x, y, Color::rgba(218, 250, 255, 220));
            }
        }
    }
    for x in 6..34 {
        let y = 18 + ((x as isize - 20).abs() / 4) as usize;
        if y < 40 {
            image.set_pixel(x, y, Color::rgba(238, 248, 255, 255));
        }
    }
    image
}

fn build_folder_icon() -> OwnedImage {
    let mut image = OwnedImage::new(40, 40, Color::TRANSPARENT);
    for y in 12..32 {
        for x in 5..35 {
            if inside_rounded_rect(x, y, Rect::new(5, 12, 30, 20), 6) {
                image.set_pixel(x, y, Color::rgb(255, 206, 98));
            }
        }
    }
    for y in 8..18 {
        for x in 8..22 {
            if inside_rounded_rect(x, y, Rect::new(8, 8, 14, 10), 4) {
                image.set_pixel(x, y, Color::rgb(255, 225, 148));
            }
        }
    }
    image
}

fn build_terminal_icon() -> OwnedImage {
    let mut image = OwnedImage::new(40, 40, Color::TRANSPARENT);
    for y in 6..34 {
        for x in 6..34 {
            if inside_rounded_rect(x, y, Rect::new(6, 6, 28, 28), 8) {
                image.set_pixel(x, y, Color::rgb(18, 24, 40));
            }
        }
    }
    for step in 0..8 {
        image.set_pixel(12 + step, 16 + step, Color::rgb(118, 250, 196));
    }
    for x in 19..28 {
        image.set_pixel(x, 24, Color::rgb(118, 250, 196));
    }
    image
}

fn build_monitor_icon() -> OwnedImage {
    let mut image = OwnedImage::new(40, 40, Color::TRANSPARENT);
    for y in 7..27 {
        for x in 5..35 {
            if inside_rounded_rect(x, y, Rect::new(5, 7, 30, 20), 6) {
                image.set_pixel(x, y, Color::rgb(81, 103, 139));
            }
        }
    }
    for y in 10..24 {
        for x in 8..32 {
            let mix = ((x + y) * 255 / 56) as u8;
            image.set_pixel(x, y, Color::rgb(25, 46, 76).lerp(Color::rgb(93, 224, 255), mix));
        }
    }
    for y in 28..31 {
        for x in 16..24 {
            image.set_pixel(x, y, Color::rgb(184, 196, 210));
        }
    }
    for y in 31..35 {
        for x in 12..28 {
            image.set_pixel(x, y, Color::rgb(118, 132, 150));
        }
    }
    image
}

fn build_settings_icon() -> OwnedImage {
    let mut image = OwnedImage::new(40, 40, Color::TRANSPARENT);
    for y in 0..40 {
        for x in 0..40 {
            let dx = x as isize - 20;
            let dy = y as isize - 20;
            let distance = dx * dx + dy * dy;
            if (80..144).contains(&distance) {
                image.set_pixel(x, y, Color::rgb(176, 214, 255));
            }
            if distance < 42 {
                image.set_pixel(x, y, Color::rgb(82, 136, 220));
            }
        }
    }
    for &(x, y) in &[(20usize, 5usize), (32, 12), (35, 20), (32, 28), (20, 35), (8, 28), (5, 20), (8, 12)] {
        for yy in y.saturating_sub(2)..=(y + 2).min(39) {
            for xx in x.saturating_sub(2)..=(x + 2).min(39) {
                image.set_pixel(xx, yy, Color::rgb(214, 236, 255));
            }
        }
    }
    image
}

fn fill_rounded_rect_fb(framebuffer: &mut Framebuffer, rect: Rect, radius: usize, color: Color) {
    for y in rect.y..rect.bottom().min(framebuffer.height()) {
        for x in rect.x..rect.right().min(framebuffer.width()) {
            if inside_rounded_rect(x, y, rect, radius) {
                framebuffer.write_pixel(x, y, color);
            }
        }
    }
}

fn stroke_rounded_rect_fb(framebuffer: &mut Framebuffer, rect: Rect, radius: usize, color: Color) {
    let inner = rect.inset(Insets::new(1, 1, 1, 1));
    for y in rect.y..rect.bottom().min(framebuffer.height()) {
        for x in rect.x..rect.right().min(framebuffer.width()) {
            let outer_hit = inside_rounded_rect(x, y, rect, radius);
            let inner_hit = inner
                .map(|inner_rect| inside_rounded_rect(x, y, inner_rect, radius.saturating_sub(1)))
                .unwrap_or(false);
            if outer_hit && !inner_hit {
                framebuffer.write_pixel(x, y, color);
            }
        }
    }
}

fn animation_phase(frame_counter: u32, divisor: u32) -> u8 {
    ((frame_counter / divisor) & 0xFF) as u8
}

fn wave_offset(phase: u8, amplitude: usize) -> usize {
    let normalized = phase as usize;
    let center = 128usize;
    if normalized >= center {
        ((normalized - center) * amplitude) / center
    } else {
        ((center - normalized) * amplitude) / center
    }
}
