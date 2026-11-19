//! Снимок рабочего стола программным растеризатором → PNG.
//!
//! Запуск: `cargo run -p orbita-ui --example snapshot --release -- [dark|light] [out.png]`
//!
//! Это эталон для ядра (тот же код рисует в UEFI framebuffer) и быстрый
//! способ проверить вёрстку без GPU.

use orbita_ui::apps::AppKind;
use orbita_ui::gfx::soft::SoftRenderer;
use orbita_ui::system::ShellPort;
use orbita_ui::tokens::ThemeMode;
use orbita_ui::{Desktop, FrameInput, Vec2};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let light = args.get(1).map(|s| s == "light").unwrap_or(false);
    let out = args.get(2).cloned().unwrap_or_else(|| String::from("target/snapshot.png"));
    let (w, h) = (1280u32, 800u32);
    let mut d = Desktop::new(Box::new(ShellPort::demo("Снимок (программный рендер)", "нет GPU")));
    if light {
        d.settings.mode = ThemeMode::Light;
        d.settings.wallpaper = orbita_ui::shell::Wallpaper::WallB;
    }
    let mut input = FrameInput { width: w, height: h, scale: 1.0, time: 0.0, fps: 165.0, mouse: Some(Vec2::new(640.0, 420.0)), ..Default::default() };
    d.frame(&input);
    d.open(AppKind::Files, Some("/home/user"));
    d.open(AppKind::Terminal, None);
    if light {
        d.open(AppKind::Settings, Some("appearance"));
        d.center_open = true;
    }
    // Прогоняем 3 секунды анимаций.
    let mut sr = SoftRenderer::new(w, h);
    let mut buf = vec![0u32; (w * h) as usize];
    for i in 0..30 {
        input.time = i as f64 * 0.1;
        let frame = d.frame(&input).clone();
        sr.render(&frame, &d.ui.p.fonts.atlas, &mut buf, w as usize);
    }
    let t0 = std::time::Instant::now();
    input.time = 3.1;
    let frame = d.frame(&input).clone();
    sr.force_full = true;
    sr.render(&frame, &d.ui.p.fonts.atlas, &mut buf, w as usize);
    eprintln!("инстансов: {}, пакетов: {}, полный кадр: {:?}", frame.instances.len(), frame.batches.len(), t0.elapsed());
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    for p in &buf {
        rgba.extend_from_slice(&[(p >> 16) as u8, (p >> 8) as u8, *p as u8, 255]);
    }
    let file = std::fs::File::create(&out).expect("create png");
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), w, h);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header().unwrap().write_image_data(&rgba).unwrap();
    eprintln!("записано: {out}");
}
