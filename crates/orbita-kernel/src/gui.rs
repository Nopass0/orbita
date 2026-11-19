//! Рабочий стол Orbita (дизайн-система `orbita-ui`) в ядре.
//!
//! Главный цикл графического режима:
//!
//! ```mermaid
//! flowchart TD
//!     N[сеть: DHCP, e1000, TCP-эхо] --> I
//!     I[ввод: PS/2 клавиатура + virtio-tablet] --> F[Desktop::frame]
//!     F --> R{virtio-gpu с virgl?}
//!     R -->|да| G[GpuRenderer: TGSI-шейдеры на видеокарте хоста]
//!     R -->|нет| S[SoftRenderer → UEFI GOP]
//!     G --> T[время кадра, FPS]
//!     S --> T
//!     T --> N
//! ```
//!
//! Оболочка получает систему через [`KernelPort`]: терминал рабочего стола
//! исполняет команды настоящим shell'ом ядра (`run`, `ps`, `ping`, файлы
//! тома в памяти с синхронизацией на диск).

extern crate alloc;

use alloc::boxed::Box;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::Ordering;

use orbita_fs::{MemoryVolume, VolumeInspector};
use orbita_hw::virtio_gpu::{Resource3d, VirtioGpu};
use orbita_hw::virtio_input::VirtioTablet;
use orbita_hw::{PciInventory, VirtioDeviceType, poll_ps2_data};
use orbita_net::NetworkStack;
use orbita_process::ProcessEngine;
use orbita_shell::{ShellEnvironment, ShellOutput, ShellRuntime};
use orbita_std::println;
use orbita_ui::gfx::soft::SoftRenderer;
use orbita_ui::input::{FrameInput, Key, KeyEvent, Modifiers, MouseButtons};
use orbita_ui::system::{DirEntry, ExecOutput, SysInfo, SystemPort, WallClock};
use orbita_ui::{Desktop, Vec2};
use orbita_video::Framebuffer;
use orbita_virgl::{GpuDevice, GpuRenderer, ResourceDesc};

use crate::disk::{AhciSectorDisk, sync_ram_to_disk};
use crate::hosts::KernelShellHost;
use orbita_fs::diskfs::OrbitaDiskFs;

/// Указатели на состояние ядра, которое живёт в кадре `kernel_main`
/// (функция никогда не возвращается, поэтому указатели валидны всегда).
/// Ядро однопоточно; ссылки из них создаются только на время одного вызова.
#[derive(Clone, Copy)]
pub(crate) struct KernelRefs {
    pub fs: *mut MemoryVolume,
    pub env: *mut ShellEnvironment,
    pub procs: *mut Option<ProcessEngine>,
    pub net: *mut NetworkStack,
    pub nic: *mut Option<orbita_hw::E1000>,
    pub disk: *mut Option<(OrbitaDiskFs, AhciSectorDisk, String)>,
}

/// [`SystemPort`] ядра: shell, файлы, сеть, часы RTC.
struct KernelPort {
    k: KernelRefs,
    runtime: ShellRuntime,
    info: SysInfo,
}

struct Collect {
    lines: Vec<String>,
    clear: bool,
}

impl ShellOutput for Collect {
    fn write_line(&mut self, line: &str) {
        for l in line.split('\n') {
            self.lines.push(l.to_string());
        }
    }
    fn set_status(&mut self, _status: &str) {}
    fn clear(&mut self) {
        self.lines.clear();
        self.clear = true;
    }
}

impl SystemPort for KernelPort {
    fn info(&self) -> SysInfo {
        self.info.clone()
    }

    fn exec(&mut self, line: &str) -> ExecOutput {
        let mut out = Collect { lines: Vec::new(), clear: false };
        // SAFETY: см. KernelRefs — однопоточно, ссылки только на время вызова.
        let (fs, env) = unsafe { (&mut *self.k.fs, &mut *self.k.env) };
        let mut host = unsafe {
            KernelShellHost { process_engine: &mut *self.k.procs, net_stack: &mut *self.k.net, live_nic: &mut *self.k.nic }
        };
        self.runtime.execute_line(env, fs, &mut out, &mut host, line);
        ExecOutput { lines: out.lines, clear: out.clear, status: env.last_status() }
    }

    fn cwd(&self) -> String {
        unsafe { String::from((*self.k.env).cwd()) }
    }

    fn list_dir(&mut self, path: &str) -> Result<Vec<DirEntry>, String> {
        let fs = unsafe { &mut *self.k.fs };
        let listing = fs.list_path(path).map_err(|e| format!("{e:?}"))?;
        let mut v: Vec<DirEntry> = listing
            .entries
            .into_iter()
            .map(|e| DirEntry { is_dir: e.metadata.is_directory(), size: e.metadata.size_bytes, name: e.name })
            .collect();
        v.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then(a.name.cmp(&b.name)));
        Ok(v)
    }

    fn read_file(&mut self, path: &str) -> Result<Vec<u8>, String> {
        unsafe { (*self.k.fs).read_file_path(path).map_err(|e| format!("{e:?}")) }
    }

    fn write_file(&mut self, path: &str, data: &[u8]) -> Result<(), String> {
        let fs = unsafe { &mut *self.k.fs };
        let _ = fs.remove_path(path);
        fs.create_file_path(path, data).map_err(|e| format!("{e:?}"))
    }

    fn remove(&mut self, path: &str) -> Result<(), String> {
        unsafe { (*self.k.fs).remove_path(path).map_err(|e| format!("{e:?}")) }
    }

    fn mkdir(&mut self, path: &str) -> Result<(), String> {
        unsafe { (*self.k.fs).create_dir_all(path).map_err(|e| format!("{e:?}")) }
    }

    fn disk_usage(&mut self) -> (u64, u64) {
        let s = unsafe { (*self.k.fs).space_stats() };
        (s.total_bytes().saturating_sub(s.available_bytes()), s.total_bytes())
    }

    fn clock(&self) -> Option<WallClock> {
        Some(read_rtc())
    }

    fn network(&self) -> String {
        let net = unsafe { &*self.k.net };
        let iface = net
            .interfaces
            .iter()
            .find(|i| i.kind == orbita_net::InterfaceKind::Ethernet)
            .map(|i| i.summary())
            .unwrap_or_default();
        if iface.is_empty() { String::from("нет сети") } else { format!("e1000 · {iface}") }
    }
}

// ------------------------------------------------------------------ RTC

fn cmos(reg: u8) -> u8 {
    let mut idx = orbita_arch_x86_64::io::Port::<u8>::new(0x70);
    let mut data = orbita_arch_x86_64::io::Port::<u8>::new(0x71);
    unsafe {
        idx.write(reg);
        data.read()
    }
}

/// Читает часы реального времени CMOS (QEMU: `-rtc base=localtime`).
fn read_rtc() -> WallClock {
    while cmos(0x0A) & 0x80 != 0 {
        core::hint::spin_loop();
    }
    let bcd = cmos(0x0B) & 0x04 == 0;
    let conv = |v: u8| if bcd { (v & 0x0F) + (v >> 4) * 10 } else { v };
    let minute = conv(cmos(0x02));
    let hour_raw = cmos(0x04);
    let pm = hour_raw & 0x80 != 0;
    let mut hour = conv(hour_raw & 0x7F);
    if cmos(0x0B) & 0x02 == 0 && pm {
        hour = (hour % 12) + 12;
    }
    let day = conv(cmos(0x07));
    let month = conv(cmos(0x08));
    let year = 2000 + conv(cmos(0x09)) as i32;
    // День недели по алгоритму Сакамото (0 = понедельник).
    let t = [0i32, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
    let y = if month < 3 { year - 1 } else { year };
    let dow = (y + y / 4 - y / 100 + y / 400 + t[(month.max(1) - 1) as usize % 12] + day as i32) % 7; // 0 = воскресенье
    WallClock { hour, minute, weekday: ((dow + 6) % 7) as u8, day, month }
}

// ------------------------------------------------------------------ время

fn rdtsc() -> u64 {
    unsafe { core::arch::x86_64::_rdtsc() }
}

/// Калибрует частоту TSC по тикам таймера ядра.
fn calibrate_tsc(quantum_hz: u64) -> f64 {
    let ticks = &crate::TIMER_IRQ_COUNT;
    let start = ticks.load(Ordering::Relaxed);
    let mut guard = 0u64;
    while ticks.load(Ordering::Relaxed) == start && guard < 2_000_000_000 {
        guard += 1;
        core::hint::spin_loop();
    }
    let t0 = rdtsc();
    let s0 = ticks.load(Ordering::Relaxed);
    let target = s0 + (quantum_hz / 10).max(2);
    guard = 0;
    while ticks.load(Ordering::Relaxed) < target && guard < 4_000_000_000 {
        guard += 1;
        core::hint::spin_loop();
    }
    let t1 = rdtsc();
    let n = ticks.load(Ordering::Relaxed).saturating_sub(s0).max(1);
    let hz = (t1 - t0) as f64 * quantum_hz as f64 / n as f64;
    if hz < 1.0e8 { 2.0e9 } else { hz }
}

// ------------------------------------------------------------------ GPU

/// Адаптер драйвера virtio-gpu к трейту рендерера.
struct VirglDevice {
    gpu: VirtioGpu,
    ctx: u32,
}

fn err<E: core::fmt::Debug>(e: E) -> String {
    format!("{e:?}")
}

impl GpuDevice for VirglDevice {
    fn create_resource(&mut self, d: ResourceDesc) -> Result<u32, String> {
        self.gpu
            .resource_create_3d(self.ctx, Resource3d { target: d.target, format: d.format, bind: d.bind, width: d.width, height: d.height })
            .map_err(err)
    }
    fn attach_backing(&mut self, res: u32, addr: u64, len: u32) -> Result<(), String> {
        self.gpu.attach_backing(res, addr, len).map_err(err)
    }
    fn upload(&mut self, res: u32, x: u32, y: u32, w: u32, h: u32, offset: u64, stride: u32) -> Result<(), String> {
        self.gpu.transfer_to_host_3d(self.ctx, res, x, y, w, h, offset, stride).map_err(err)
    }
    fn submit(&mut self, cmds: &[u32]) -> Result<(), String> {
        self.gpu.submit_3d(self.ctx, cmds).map_err(err)
    }
    fn set_scanout(&mut self, res: u32, w: u32, h: u32) -> Result<(), String> {
        self.gpu.set_scanout(0, res, w, h).map_err(err)
    }
    fn present(&mut self, res: u32, w: u32, h: u32) -> Result<(), String> {
        self.gpu.flush(res, w, h).map_err(err)
    }
    fn finish(&mut self) -> Result<(), String> {
        self.gpu.wait().map_err(err)
    }
}

/// Пытается поднять virgl: устройство, контекст, разрешение экрана.
fn init_virgl(pci: &PciInventory) -> Option<(VirglDevice, u32, u32, String)> {
    let dev = pci.devices().iter().find(|d| d.virtio_device_type == Some(VirtioDeviceType::Gpu))?;
    let mut gpu = match VirtioGpu::init(dev) {
        Ok(g) => g,
        Err(e) => {
            println!("Orbita OS: virtio-gpu init failed: {e:?}");
            return None;
        }
    };
    println!("Orbita OS: {}", gpu.describe());
    if !gpu.virgl {
        println!("Orbita OS: virtio-gpu без virgl (нужен -device virtio-vga-gl -display sdl,gl=on)");
        return None;
    }
    let mut capsets = Vec::new();
    for i in 0..4 {
        if let Ok(c) = gpu.capset_info(i) {
            capsets.push(c);
        }
    }
    println!("Orbita OS: virgl capsets {:?}", capsets);
    let (w, h) = gpu.display_info().ok().and_then(|v| v.first().copied()).unwrap_or((1280, 800));
    if let Err(e) = gpu.ctx_create(1, "orbita-ui") {
        println!("Orbita OS: virgl ctx_create failed: {e:?}");
        return None;
    }
    let api = String::from("virgl · OpenGL хоста");
    Some((VirglDevice { gpu, ctx: 1 }, w, h, api))
}

// ------------------------------------------------------------------ клавиатура

/// Декодер PS/2 set 1 → события `orbita-ui` (модификаторы, раскладка RU/EN).
struct KeyDecoder {
    ext: bool,
    mods: Modifiers,
    russian: bool,
}

const RU_LOWER: &[(u8, char)] = &[
    (0x10, 'й'), (0x11, 'ц'), (0x12, 'у'), (0x13, 'к'), (0x14, 'е'), (0x15, 'н'), (0x16, 'г'), (0x17, 'ш'),
    (0x18, 'щ'), (0x19, 'з'), (0x1A, 'х'), (0x1B, 'ъ'), (0x1E, 'ф'), (0x1F, 'ы'), (0x20, 'в'), (0x21, 'а'),
    (0x22, 'п'), (0x23, 'р'), (0x24, 'о'), (0x25, 'л'), (0x26, 'д'), (0x27, 'ж'), (0x28, 'э'), (0x2C, 'я'),
    (0x2D, 'ч'), (0x2E, 'с'), (0x2F, 'м'), (0x30, 'и'), (0x31, 'т'), (0x32, 'ь'), (0x33, 'б'), (0x34, 'ю'),
    (0x29, 'ё'),
];

impl KeyDecoder {
    fn new() -> KeyDecoder {
        KeyDecoder { ext: false, mods: Modifiers::default(), russian: false }
    }

    /// Обрабатывает байт: добавляет события и набранный текст.
    fn feed(&mut self, sc: u8, keys: &mut Vec<KeyEvent>, text: &mut String) {
        if sc == 0xE0 {
            self.ext = true;
            return;
        }
        let down = sc & 0x80 == 0;
        let code = sc & 0x7F;
        let ext = core::mem::replace(&mut self.ext, false);
        match (ext, code) {
            (_, 0x2A) | (_, 0x36) => {
                self.mods.shift = down;
                return;
            }
            (_, 0x1D) => {
                self.mods.ctrl = down;
                return;
            }
            (_, 0x38) => {
                // Alt+Shift — переключение раскладки.
                if down && self.mods.shift {
                    self.russian = !self.russian;
                }
                self.mods.alt = down;
                return;
            }
            _ => {}
        }
        let key = if ext {
            match code {
                0x48 => Some(Key::Up),
                0x50 => Some(Key::Down),
                0x4B => Some(Key::Left),
                0x4D => Some(Key::Right),
                0x47 => Some(Key::Home),
                0x4F => Some(Key::End),
                0x49 => Some(Key::PageUp),
                0x51 => Some(Key::PageDown),
                0x53 => Some(Key::Delete),
                0x1C => Some(Key::Enter),
                0x5B | 0x5C => Some(Key::Super),
                _ => None,
            }
        } else {
            match code {
                0x01 => Some(Key::Escape),
                0x0E => Some(Key::Backspace),
                0x0F => Some(Key::Tab),
                0x1C => Some(Key::Enter),
                0x39 => Some(Key::Space),
                0x3B..=0x44 => Some(Key::F(code - 0x3A)),
                0x57 => Some(Key::F(11)),
                0x58 => Some(Key::F(12)),
                _ => crate::console::map_scancode_set1(code, false).filter(|c| c.is_ascii_alphanumeric()).map(Key::Char),
            }
        };
        if let Some(k) = key {
            keys.push(KeyEvent { key: k, down, mods: self.mods });
        }
        if !down || self.mods.ctrl || self.mods.alt || ext {
            return;
        }
        if code == 0x39 {
            text.push(' ');
            return;
        }
        if self.russian {
            if let Some((_, c)) = RU_LOWER.iter().find(|(s, _)| *s == code) {
                let c = if self.mods.shift { c.to_uppercase().next().unwrap_or(*c) } else { *c };
                text.push(c);
                return;
            }
        }
        if let Some(c) = crate::console::map_scancode_set1(code, self.mods.shift) {
            if !c.is_control() {
                text.push(c);
            }
        }
    }
}

// ------------------------------------------------------------------ сеть

/// Один круг обслуживания сети (то же, что делал текстовый цикл).
fn service_network(k: &KernelRefs, dhcp_started: &mut bool, dhcp_logged: &mut bool) {
    let (net, nic) = unsafe { (&mut *k.net, &mut *k.nic) };
    let Some(nic) = nic.as_mut() else { return };
    if !*dhcp_started {
        *dhcp_started = net.dhcp_start();
    }
    for frame in nic.poll_rx() {
        let _ = net.receive(&frame);
    }
    if net.dhcp_bound && !*dhcp_logged {
        *dhcp_logged = true;
        println!("Orbita OS: dhcp bound ok (gui)");
    }
    crate::tcp_echo_service(net);
    while let Some(frame) = net.take_tx_frame() {
        nic.send(&frame);
    }
}

// ------------------------------------------------------------------ цикл

/// Запускает графический рабочий стол. Не возвращается.
pub(crate) fn run(
    k: KernelRefs,
    pci: &PciInventory,
    framebuffer: &mut Framebuffer,
    quantum_hz: u64,
    cpus: u32,
    memory: String,
) -> ! {
    println!("Orbita OS: gui starting (orbita-ui design system)");
    let tsc_hz = calibrate_tsc(quantum_hz.max(1));
    println!("Orbita OS: tsc {} MHz", (tsc_hz / 1.0e6) as u64);
    let now = || rdtsc() as f64 / tsc_hz;

    let virgl = init_virgl(pci);
    let (width, height, gpu_name, renderer_name) = match &virgl {
        Some((_, w, h, api)) => (*w, *h, String::from("virtio-gpu (видеокарта хоста)"), format!("GPU · {api}")),
        None => (framebuffer.width() as u32, framebuffer.height() as u32, String::from("UEFI GOP"), String::from("программный (CPU)")),
    };

    let info = SysInfo {
        os: String::from("Orbita OS 0.1.0"),
        machine: String::from("x86_64 UEFI (QEMU)"),
        cpus,
        memory,
        gpu: gpu_name,
        renderer: renderer_name.clone(),
        hostname: String::from("orbita"),
    };
    let port = KernelPort { k, runtime: ShellRuntime::new(), info };
    unsafe {
        let env = &mut *k.env;
        if env.cwd() == "/" {
            env.set_cwd("/home/user");
        }
    }
    let mut desktop = Desktop::new(Box::new(port));

    let mut tablet = pci
        .devices()
        .iter()
        .find(|d| d.virtio_device_type == Some(VirtioDeviceType::Input))
        .and_then(|d| VirtioTablet::init(d).ok());
    println!("Orbita OS: pointer = {}", if tablet.is_some() { "virtio-tablet" } else { "нет (только клавиатура)" });

    let mut gpu = match virgl {
        Some((mut dev, w, h, _)) => match GpuRenderer::new(&mut dev, w, h, &desktop.ui.p.fonts.atlas) {
            Ok(r) => {
                println!("Orbita OS: gui renderer = virgl GPU {}x{}", w, h);
                Some((dev, r))
            }
            Err(e) => {
                println!("Orbita OS: virgl renderer init failed: {e} — программный режим");
                None
            }
        },
        None => None,
    };
    let (width, height) = if gpu.is_some() { (width, height) } else { (framebuffer.width() as u32, framebuffer.height() as u32) };
    let mut soft = SoftRenderer::new(width, height);
    let mut back = if gpu.is_none() { vec![0u32; (width * height) as usize] } else { Vec::new() };

    let mut keys = KeyDecoder::new();
    let mut mouse = Vec2::new(width as f32 * 0.5, height as f32 * 0.5);
    let mut buttons = 0u8;
    let mut fps = 0.0f32;
    let mut frames_in_window = 0u32;
    let mut window_start = now();
    let mut cpu_ms = 0.0f32;
    let mut gpu_ms = 0.0f32;
    let mut last_sync = now();
    let mut dhcp_started = false;
    let mut dhcp_logged = false;
    let mut logged_first = false;

    loop {
        service_network(&k, &mut dhcp_started, &mut dhcp_logged);

        let mut input = FrameInput { width, height, scale: 1.0, time: now(), ..Default::default() };
        let mut budget = 64;
        while budget > 0 {
            let Some(sc) = poll_ps2_data() else { break };
            budget -= 1;
            keys.feed(sc, &mut input.keys, &mut input.text);
        }
        if let Some(t) = tablet.as_mut() {
            let s = t.poll();
            mouse = Vec2::new(s.x * width as f32, s.y * height as f32);
            buttons = s.buttons;
            input.wheel = Vec2::new(0.0, -(s.wheel as f32) * 48.0);
        }
        input.mouse = Some(mouse);
        input.buttons = MouseButtons(buttons);
        input.mods = keys.mods;
        input.fps = fps;
        input.cpu_ms = cpu_ms;
        input.gpu_ms = gpu_ms;

        let t0 = now();
        desktop.frame(&input);
        let t1 = now();
        match gpu.as_mut() {
            Some((dev, r)) => {
                let frame: *const orbita_ui::gfx::Frame = desktop.ui.p.frame();
                // SAFETY: кадр не меняется до следующего Desktop::frame.
                if let Err(e) = r.render(dev, unsafe { &*frame }, &mut desktop.ui.p.fonts.atlas) {
                    println!("Orbita OS: gpu frame error: {e}");
                }
                if !logged_first {
                    logged_first = true;
                    println!("Orbita OS: gui first frame on GPU, instances={} captures={}", r.stats.instances, r.stats.captures);
                }
            }
            None => {
                let frame = desktop.ui.p.frame();
                let dmg = soft.render(frame, &desktop.ui.p.fonts.atlas, &mut back, width as usize);
                present_gop(framebuffer, &back, width, &dmg);
                if !logged_first {
                    logged_first = true;
                    println!("Orbita OS: gui first frame (software)");
                }
            }
        }
        let t2 = now();
        cpu_ms = ((t1 - t0) * 1000.0) as f32;
        gpu_ms = ((t2 - t1) * 1000.0) as f32;
        frames_in_window += 1;
        if t2 - window_start >= 0.5 {
            fps = (frames_in_window as f64 / (t2 - window_start)) as f32;
            frames_in_window = 0;
            window_start = t2;
        }
        // Изменённые файлы — на диск раз в 5 секунд.
        if t2 - last_sync > 5.0 {
            last_sync = t2;
            unsafe {
                if let Some((diskfs, disk, _)) = (*k.disk).as_mut() {
                    let n = sync_ram_to_disk(diskfs, disk, &mut *k.fs);
                    if n > 0 {
                        println!("Orbita OS: vfs sync wrote {n} file(s) to disk");
                    }
                }
            }
        }
    }
}

/// Копирует повреждённые области заднего буфера в GOP framebuffer.
fn present_gop(fb: &mut Framebuffer, back: &[u32], width: u32, damage: &[orbita_ui::Rect]) {
    let info = fb.info;
    let stride_px = info.stride / 4;
    for r in damage {
        let x0 = r.x.max(0.0) as usize;
        let y0 = r.y.max(0.0) as usize;
        let x1 = (r.right() as usize).min(info.width);
        let y1 = (r.bottom() as usize).min(info.height);
        for y in y0..y1 {
            let src = &back[y * width as usize + x0..y * width as usize + x1];
            unsafe {
                let dst = (info.base as *mut u32).add(y * stride_px + x0);
                core::ptr::copy_nonoverlapping(src.as_ptr(), dst, x1 - x0);
            }
        }
    }
}
