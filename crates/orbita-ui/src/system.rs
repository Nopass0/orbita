//! Связь оболочки с системой: файлы, команды, сведения об устройстве.
//!
//! Оболочка не знает, где она запущена. Всё системное — через трейт
//! [`SystemPort`]:
//!
//! * в ядре его реализует [`ShellPort`] поверх настоящих `orbita-shell` и
//!   `orbita-fs` (тот же shell, что и в консоли ОС: пайпы, перенаправления,
//!   `pkg`, `ping`, скрипты);
//! * в эмуляторе — тот же [`ShellPort`] с засеянным томом в памяти, так
//!   что терминал и проводник ведут себя одинаково в обоих мирах.

extern crate alloc;

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;

/// Запись каталога для проводника.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DirEntry {
    /// Имя.
    pub name: String,
    /// Каталог ли это.
    pub is_dir: bool,
    /// Размер в байтах (для каталогов 0).
    pub size: u64,
}

/// Сведения о системе (окно «Монитор», `about`).
#[derive(Clone, Debug, PartialEq, Default)]
pub struct SysInfo {
    /// Название ОС.
    pub os: String,
    /// Где запущена: «QEMU x86_64 (UEFI)», «Эмулятор Orbita (wgpu)».
    pub machine: String,
    /// Число логических процессоров.
    pub cpus: u32,
    /// Память (строкой, уже отформатирована).
    pub memory: String,
    /// Видеоадаптер.
    pub gpu: String,
    /// Активный рендерер: «GPU · Vulkan», «программный».
    pub renderer: String,
    /// Имя узла.
    pub hostname: String,
}

/// Результат выполнения строки shell'а.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct ExecOutput {
    /// Строки вывода.
    pub lines: Vec<String>,
    /// Команда попросила очистить экран (`clear`).
    pub clear: bool,
    /// Код возврата.
    pub status: u32,
}

/// Локальное время для часов верхней панели.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct WallClock {
    /// Часы 0–23.
    pub hour: u8,
    /// Минуты.
    pub minute: u8,
    /// День недели 0 = понедельник.
    pub weekday: u8,
    /// День месяца 1–31.
    pub day: u8,
    /// Месяц 1–12.
    pub month: u8,
}

/// Порт системы — всё, что оболочке нужно от ОС.
pub trait SystemPort {
    /// Сведения о системе.
    fn info(&self) -> SysInfo;
    /// Выполнить строку shell'а.
    fn exec(&mut self, line: &str) -> ExecOutput;
    /// Текущий каталог shell'а (для приглашения).
    fn cwd(&self) -> String;
    /// Содержимое каталога.
    fn list_dir(&mut self, path: &str) -> Result<Vec<DirEntry>, String>;
    /// Прочитать файл целиком.
    fn read_file(&mut self, path: &str) -> Result<Vec<u8>, String>;
    /// Записать файл целиком (создаёт при необходимости).
    fn write_file(&mut self, path: &str, data: &[u8]) -> Result<(), String>;
    /// Удалить файл или пустой каталог.
    fn remove(&mut self, path: &str) -> Result<(), String>;
    /// Создать каталог (с родителями).
    fn mkdir(&mut self, path: &str) -> Result<(), String>;
    /// Занято / всего байт на томе.
    fn disk_usage(&mut self) -> (u64, u64);
    /// Локальное время (если платформа знает).
    fn clock(&self) -> Option<WallClock>;
    /// Строка состояния сети («e1000 · 10.0.2.15», «нет сети»).
    fn network(&self) -> String;
}

/// Объединяет путь каталога и имя.
///
/// ```
/// use orbita_ui::system::join_path;
/// assert_eq!(join_path("/", "etc"), "/etc");
/// assert_eq!(join_path("/home/user", "a.txt"), "/home/user/a.txt");
/// ```
pub fn join_path(dir: &str, name: &str) -> String {
    if dir.ends_with('/') {
        let mut s = String::from(dir);
        s.push_str(name);
        s
    } else {
        let mut s = String::from(dir);
        s.push('/');
        s.push_str(name);
        s
    }
}

/// Родительский каталог.
///
/// ```
/// use orbita_ui::system::parent_path;
/// assert_eq!(parent_path("/home/user"), "/home");
/// assert_eq!(parent_path("/home"), "/");
/// assert_eq!(parent_path("/"), "/");
/// ```
pub fn parent_path(path: &str) -> String {
    let trimmed = path.trim_end_matches('/');
    match trimmed.rfind('/') {
        Some(0) | None => String::from("/"),
        Some(i) => String::from(&trimmed[..i]),
    }
}

/// Форматирует размер: «12 Б», «4.2 КБ», «1.5 МБ», «8 ГБ».
///
/// ```
/// use orbita_ui::system::format_size;
/// assert_eq!(format_size(512), "512 Б");
/// assert_eq!(format_size(4300), "4.2 КБ");
/// ```
pub fn format_size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["КБ", "МБ", "ГБ", "ТБ"];
    if bytes < 1024 {
        return alloc::format!("{bytes} Б");
    }
    let mut v = bytes as f64 / 1024.0;
    let mut u = 0;
    while v >= 1024.0 && u < UNITS.len() - 1 {
        v /= 1024.0;
        u += 1;
    }
    if v >= 10.0 {
        alloc::format!("{} {}", (v + 0.5) as u64, UNITS[u])
    } else {
        let tenths = (v * 10.0 + 0.5) as u64;
        alloc::format!("{}.{} {}", tenths / 10, tenths % 10, UNITS[u])
    }
}

// ------------------------------------------------------------------ ShellPort

#[cfg(feature = "system")]
pub use shell_port::{ShellPort, seed_demo_volume};

#[cfg(feature = "system")]
mod shell_port {
    use super::*;
    use alloc::string::ToString;
    use orbita_fs::{BlockSize, FsCapabilities, MemoryVolume, VolumeId, VolumeInspector};
    use orbita_shell::{NoopShellHost, ShellEnvironment, ShellHost, ShellOutput, ShellRuntime, ShellSystemInfo};

    /// [`SystemPort`] поверх настоящего shell'а Orbita OS и тома в памяти.
    pub struct ShellPort {
        /// Том файловой системы.
        pub fs: MemoryVolume,
        /// Окружение shell'а (cwd, переменные, история).
        pub env: ShellEnvironment,
        runtime: ShellRuntime,
        host: Box<dyn ShellHost>,
        info: SysInfo,
        clock: Option<fn() -> Option<WallClock>>,
        network: String,
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

    impl ShellPort {
        /// Создаёт порт.
        ///
        /// Принимает: том, хост сервисов ядра (`run`, `ps`, `ping`), сведения о
        /// системе и функцию часов.
        pub fn new(fs: MemoryVolume, host: Box<dyn ShellHost>, info: SysInfo, clock: Option<fn() -> Option<WallClock>>) -> ShellPort {
            let env = ShellEnvironment::new(ShellSystemInfo::new(
                info.os.clone(),
                info.gpu.clone(),
                info.memory.clone(),
                String::from("auto"),
                info.cpus,
            ));
            ShellPort { fs, env, runtime: ShellRuntime::new(), host, info, clock, network: String::from("нет сети") }
        }

        /// Порт для эмулятора и тестов: засеянный том, хост без ядра.
        ///
        /// ```
        /// use orbita_ui::system::{ShellPort, SystemPort};
        /// let mut port = ShellPort::demo("Эмулятор", "тестовый GPU");
        /// let out = port.exec("echo привет");
        /// assert_eq!(out.lines, vec!["привет".to_string()]);
        /// assert!(port.list_dir("/home/user").unwrap().len() > 2);
        /// ```
        pub fn demo(machine: &str, gpu: &str) -> ShellPort {
            let mut fs = MemoryVolume::new(
                VolumeId(0x0B17_AE57),
                BlockSize(4096),
                131_072,
                FsCapabilities { block_size: BlockSize(4096), features: &[] },
            );
            let _ = seed_demo_volume(&mut fs);
            let info = SysInfo {
                os: String::from("Orbita OS 0.1.0"),
                machine: String::from(machine),
                cpus: 1,
                memory: String::from("—"),
                gpu: String::from(gpu),
                renderer: String::from("GPU"),
                hostname: String::from("orbita"),
            };
            let mut port = ShellPort::new(fs, Box::new(NoopShellHost), info, None);
            port.env.set_cwd("/home/user");
            port
        }

        /// Обновляет сведения о системе (например, GPU после старта рендерера).
        pub fn set_info(&mut self, info: SysInfo) {
            self.info = info;
        }

        /// Задаёт строку состояния сети.
        pub fn set_network(&mut self, s: &str) {
            self.network = String::from(s);
        }

        /// Задаёт функцию часов.
        pub fn set_clock(&mut self, f: fn() -> Option<WallClock>) {
            self.clock = Some(f);
        }
    }

    impl SystemPort for ShellPort {
        fn info(&self) -> SysInfo {
            self.info.clone()
        }

        fn exec(&mut self, line: &str) -> ExecOutput {
            let mut out = Collect { lines: Vec::new(), clear: false };
            self.runtime.execute_line(&mut self.env, &mut self.fs, &mut out, self.host.as_mut(), line);
            ExecOutput { lines: out.lines, clear: out.clear, status: self.env.last_status() }
        }

        fn cwd(&self) -> String {
            String::from(self.env.cwd())
        }

        fn list_dir(&mut self, path: &str) -> Result<Vec<DirEntry>, String> {
            let listing = self.fs.list_path(path).map_err(|e| alloc::format!("{e:?}"))?;
            let mut v: Vec<DirEntry> = listing
                .entries
                .into_iter()
                .map(|e| DirEntry {
                    is_dir: e.metadata.is_directory(),
                    size: e.metadata.size_bytes,
                    name: e.name,
                })
                .collect();
            v.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then(a.name.cmp(&b.name)));
            Ok(v)
        }

        fn read_file(&mut self, path: &str) -> Result<Vec<u8>, String> {
            self.fs.read_file_path(path).map_err(|e| alloc::format!("{e:?}"))
        }

        fn write_file(&mut self, path: &str, data: &[u8]) -> Result<(), String> {
            let _ = self.fs.remove_path(path);
            self.fs.create_file_path(path, data).map_err(|e| alloc::format!("{e:?}"))
        }

        fn remove(&mut self, path: &str) -> Result<(), String> {
            self.fs.remove_path(path).map_err(|e| alloc::format!("{e:?}"))
        }

        fn mkdir(&mut self, path: &str) -> Result<(), String> {
            self.fs.create_dir_all(path).map_err(|e| alloc::format!("{e:?}"))
        }

        fn disk_usage(&mut self) -> (u64, u64) {
            let s = self.fs.space_stats();
            let total = s.total_bytes();
            (total.saturating_sub(s.available_bytes()), total)
        }

        fn clock(&self) -> Option<WallClock> {
            self.clock.and_then(|f| f())
        }

        fn network(&self) -> String {
            self.network.clone()
        }
    }

    /// Засевает том демонстрационной домашней папкой (эмулятор, тесты).
    pub fn seed_demo_volume(fs: &mut MemoryVolume) -> Result<(), orbita_fs::FsError> {
        for d in [
            "/etc",
            "/home/user/Проекты/orbita",
            "/home/user/Документы",
            "/home/user/Загрузки",
            "/home/user/Изображения",
            "/system/apps",
            "/tmp",
            "/var/log",
        ] {
            fs.create_dir_all(d)?;
        }
        fs.create_file_path("/etc/motd", "Добро пожаловать в Orbita OS.\n".as_bytes())?;
        fs.create_file_path("/etc/hostname", b"orbita\n")?;
        fs.create_file_path(
            "/home/user/readme.md",
            "# Orbita OS\n\nДизайн-система: стекло, шум, ambient-градиенты.\nТерминал понимает пайпы, перенаправления и скрипты.\n".as_bytes(),
        )?;
        fs.create_file_path(
            "/home/user/Документы/план.txt",
            "1. Этап C — virtio-gpu\n2. Этап F — UI-платформа\n3. Тесты в QEMU\n".as_bytes(),
        )?;
        fs.create_file_path("/home/user/Документы/заметки.txt", "Купить кофе.\nПочинить DHCP.\n".as_bytes())?;
        fs.create_file_path(
            "/home/user/Проекты/orbita/main.rs",
            b"fn main() {\n    println!(\"hello, orbita\");\n}\n",
        )?;
        fs.create_file_path("/home/user/Проекты/orbita/Cargo.toml", b"[package]\nname = \"orbita\"\n")?;
        fs.create_file_path("/home/user/Изображения/wallpaper.png", &[0u8; 2048])?;
        fs.create_file_path("/home/user/Загрузки/backup.zip", &[0u8; 8192])?;
        fs.create_file_path(
            "/home/user/hello.sh",
            b"echo \"Orbita shell\"\nfor i in 1 2 3; do echo \"step $i\"; done\n",
        )?;
        Ok(())
    }
}

/// Демонстрационная система (для документации и тестов без `feature = "system"`).
pub struct DemoSystem;

impl DemoSystem {
    /// Порт в коробке: с feature `system` — настоящий shell, иначе заглушка.
    pub fn boxed() -> Box<dyn SystemPort> {
        #[cfg(feature = "system")]
        {
            Box::new(ShellPort::demo("Демо", "—"))
        }
        #[cfg(not(feature = "system"))]
        {
            Box::new(NullSystem)
        }
    }
}

/// Пустая система: ничего не умеет, но и не падает.
pub struct NullSystem;

impl SystemPort for NullSystem {
    fn info(&self) -> SysInfo {
        SysInfo { os: String::from("Orbita OS"), ..Default::default() }
    }
    fn exec(&mut self, line: &str) -> ExecOutput {
        ExecOutput { lines: alloc::vec![alloc::format!("{line}: shell недоступен")], clear: false, status: 127 }
    }
    fn cwd(&self) -> String {
        String::from("/")
    }
    fn list_dir(&mut self, _path: &str) -> Result<Vec<DirEntry>, String> {
        Ok(Vec::new())
    }
    fn read_file(&mut self, _path: &str) -> Result<Vec<u8>, String> {
        Err(String::from("нет файловой системы"))
    }
    fn write_file(&mut self, _path: &str, _data: &[u8]) -> Result<(), String> {
        Err(String::from("нет файловой системы"))
    }
    fn remove(&mut self, _path: &str) -> Result<(), String> {
        Err(String::from("нет файловой системы"))
    }
    fn mkdir(&mut self, _path: &str) -> Result<(), String> {
        Err(String::from("нет файловой системы"))
    }
    fn disk_usage(&mut self) -> (u64, u64) {
        (0, 0)
    }
    fn clock(&self) -> Option<WallClock> {
        None
    }
    fn network(&self) -> String {
        String::from("нет сети")
    }
}
