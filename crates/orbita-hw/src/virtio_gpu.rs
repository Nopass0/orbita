//! Драйвер `virtio-gpu` (QEMU `virtio-vga-gl`): вывод изображения и
//! 3D-ускорение через протокол virgl.
//!
//! В режиме virgl гость не рисует пиксели сам: он создаёт 3D-ресурсы
//! (текстуры, буферы) и отправляет поток команд Gallium (`SUBMIT_3D`) с
//! шейдерами TGSI. QEMU передаёт их в virglrenderer, который исполняет их
//! на **реальной видеокарте хоста** через OpenGL. Готовая текстура-цель
//! назначается scanout'ом и показывается окном QEMU (`-display sdl,gl=on`).
//!
//! ```mermaid
//! sequenceDiagram
//!     participant K as Ядро Orbita
//!     participant Q as QEMU virtio-gpu
//!     participant V as virglrenderer
//!     participant G as GPU хоста (RTX)
//!     K->>Q: CTX_CREATE, RESOURCE_CREATE_3D, ATTACH_BACKING
//!     K->>Q: TRANSFER_TO_HOST_3D (вершины, атласы)
//!     K->>Q: SUBMIT_3D (шейдеры, draw_vbo)
//!     Q->>V: vrend_decode
//!     V->>G: OpenGL draw
//!     K->>Q: RESOURCE_FLUSH (scanout)
//!     Q->>G: показать текстуру в окне
//! ```

extern crate alloc;

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use crate::pci::PciDevice;
use crate::virtio::{VirtioError, VirtioPci, Virtqueue};

/// Фича: устройство поддерживает 3D (virgl).
pub const VIRTIO_GPU_F_VIRGL: u64 = 1 << 0;
/// Фича: устройство отдаёт EDID.
pub const VIRTIO_GPU_F_EDID: u64 = 1 << 1;

const CMD_GET_DISPLAY_INFO: u32 = 0x0100;
const CMD_RESOURCE_CREATE_2D: u32 = 0x0101;
const CMD_SET_SCANOUT: u32 = 0x0103;
const CMD_RESOURCE_FLUSH: u32 = 0x0104;
const CMD_TRANSFER_TO_HOST_2D: u32 = 0x0105;
const CMD_RESOURCE_ATTACH_BACKING: u32 = 0x0106;
const CMD_GET_CAPSET_INFO: u32 = 0x0108;
const CMD_CTX_CREATE: u32 = 0x0200;
const CMD_CTX_ATTACH_RESOURCE: u32 = 0x0202;
const CMD_RESOURCE_CREATE_3D: u32 = 0x0204;
const CMD_TRANSFER_TO_HOST_3D: u32 = 0x0205;
const CMD_SUBMIT_3D: u32 = 0x0207;

const RESP_OK_NODATA: u32 = 0x1100;
const RESP_OK_DISPLAY_INFO: u32 = 0x1101;
const RESP_OK_CAPSET_INFO: u32 = 0x1102;

/// Флаг заголовка: команда несёт fence.
const FLAG_FENCE: u32 = 1;

/// Ошибки драйвера.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GpuError {
    /// Ошибка транспорта virtio.
    Transport(VirtioError),
    /// Устройство ответило кодом ошибки (`0x12xx`).
    Device(u32),
}

impl From<VirtioError> for GpuError {
    fn from(e: VirtioError) -> Self {
        GpuError::Transport(e)
    }
}

/// Параметры 3D-ресурса (`virtio_gpu_resource_create_3d`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Resource3d {
    /// `PIPE_BUFFER` = 0, `PIPE_TEXTURE_2D` = 2.
    pub target: u32,
    /// Формат virgl (`VIRGL_FORMAT_*`).
    pub format: u32,
    /// Флаги привязки (`VIRGL_BIND_*`).
    pub bind: u32,
    /// Ширина (для буфера — размер в байтах).
    pub width: u32,
    /// Высота.
    pub height: u32,
}

/// Кодировщик заголовка управляющей команды (24 байта).
fn hdr(out: &mut Vec<u8>, ty: u32, flags: u32, fence: u64, ctx: u32) {
    out.extend_from_slice(&ty.to_le_bytes());
    out.extend_from_slice(&flags.to_le_bytes());
    out.extend_from_slice(&fence.to_le_bytes());
    out.extend_from_slice(&ctx.to_le_bytes());
    out.extend_from_slice(&[0u8; 4]);
}

fn put(out: &mut Vec<u8>, vals: &[u32]) {
    for v in vals {
        out.extend_from_slice(&v.to_le_bytes());
    }
}

/// Драйвер virtio-gpu.
pub struct VirtioGpu {
    pci: VirtioPci,
    ctrl: Virtqueue,
    /// Устройство поддерживает virgl 3D.
    pub virgl: bool,
    /// Число scanout'ов (мониторов).
    pub scanouts: u32,
    next_res: u32,
    fence: u64,
    pending: Vec<(Vec<u8>, Vec<u8>)>,
    /// Последняя ошибка устройства (для диагностики).
    pub last_error: Option<u32>,
}

unsafe impl Send for VirtioGpu {}

impl VirtioGpu {
    /// Инициализирует устройство: фичи (virgl, EDID), управляющая очередь.
    pub fn init(dev: &PciDevice) -> Result<VirtioGpu, GpuError> {
        let mut pci = VirtioPci::probe(dev)?;
        let feats = pci.negotiate(VIRTIO_GPU_F_VIRGL | VIRTIO_GPU_F_EDID)?;
        let ctrl = pci.setup_queue(0, 256)?;
        // Очередь курсора (1) тоже включаем, но курсор рисует сама оболочка.
        let _cursor = pci.setup_queue(1, 16).ok();
        pci.driver_ok();
        let scanouts = pci.device_cfg_u32(8);
        Ok(VirtioGpu {
            pci,
            ctrl,
            virgl: feats & VIRTIO_GPU_F_VIRGL != 0,
            scanouts,
            next_res: 1,
            fence: 1,
            pending: Vec::new(),
            last_error: None,
        })
    }

    /// Синхронная команда с ответом заданного размера.
    fn call(&mut self, req: Vec<u8>, resp_len: usize) -> Result<Vec<u8>, GpuError> {
        self.wait()?;
        let mut resp = vec![0u8; resp_len.max(24)];
        self.ctrl.request(&req, &mut resp)?;
        Ok(resp)
    }

    fn resp_type(resp: &[u8]) -> u32 {
        u32::from_le_bytes([resp[0], resp[1], resp[2], resp[3]])
    }

    /// Ставит команду в очередь без ожидания (буферы живут до [`VirtioGpu::wait`]).
    fn queue(&mut self, req: Vec<u8>) -> Result<(), GpuError> {
        if self.ctrl.free_count() < 2 {
            self.wait()?;
        }
        let mut resp = vec![0u8; 24];
        self.ctrl.push(&[(req.as_ptr() as u64, req.len() as u32)], &[(resp.as_mut_ptr() as u64, 24)])?;
        self.pending.push((req, resp));
        Ok(())
    }

    /// Отправляет устройству накопленные команды (без ожидания).
    pub fn kick(&self) {
        self.ctrl.kick();
    }

    /// Ждёт исполнения всех команд в очереди и проверяет ответы.
    pub fn wait(&mut self) -> Result<(), GpuError> {
        if self.pending.is_empty() {
            return Ok(());
        }
        self.ctrl.kick();
        self.ctrl.wait_all()?;
        for (_, resp) in self.pending.drain(..) {
            let t = Self::resp_type(&resp);
            if t >= 0x1200 {
                self.last_error = Some(t);
            }
        }
        match self.last_error.take() {
            Some(e) => Err(GpuError::Device(e)),
            None => Ok(()),
        }
    }

    /// Разрешения включённых дисплеев.
    pub fn display_info(&mut self) -> Result<Vec<(u32, u32)>, GpuError> {
        let mut req = Vec::new();
        hdr(&mut req, CMD_GET_DISPLAY_INFO, 0, 0, 0);
        let resp = self.call(req, 24 + 16 * 24)?;
        if Self::resp_type(&resp) != RESP_OK_DISPLAY_INFO {
            return Err(GpuError::Device(Self::resp_type(&resp)));
        }
        let mut out = Vec::new();
        for i in 0..16 {
            let o = 24 + i * 24;
            let w = u32::from_le_bytes(resp[o + 8..o + 12].try_into().unwrap());
            let h = u32::from_le_bytes(resp[o + 12..o + 16].try_into().unwrap());
            let enabled = u32::from_le_bytes(resp[o + 16..o + 20].try_into().unwrap());
            if enabled != 0 && w > 0 && h > 0 {
                out.push((w, h));
            }
        }
        Ok(out)
    }

    /// Сведения о наборе возможностей (capset) `index`: (id, версия, размер).
    pub fn capset_info(&mut self, index: u32) -> Result<(u32, u32, u32), GpuError> {
        let mut req = Vec::new();
        hdr(&mut req, CMD_GET_CAPSET_INFO, 0, 0, 0);
        put(&mut req, &[index, 0]);
        let resp = self.call(req, 40)?;
        if Self::resp_type(&resp) != RESP_OK_CAPSET_INFO {
            return Err(GpuError::Device(Self::resp_type(&resp)));
        }
        let rd = |o: usize| u32::from_le_bytes(resp[o..o + 4].try_into().unwrap());
        Ok((rd(24), rd(28), rd(32)))
    }

    /// Создаёт 3D-контекст virgl с номером `ctx`.
    pub fn ctx_create(&mut self, ctx: u32, name: &str) -> Result<(), GpuError> {
        let mut req = Vec::new();
        hdr(&mut req, CMD_CTX_CREATE, 0, 0, ctx);
        let n = name.len().min(63);
        put(&mut req, &[n as u32, 0]);
        let mut debug = [0u8; 64];
        debug[..n].copy_from_slice(&name.as_bytes()[..n]);
        req.extend_from_slice(&debug);
        self.expect_ok(req)
    }

    fn expect_ok(&mut self, req: Vec<u8>) -> Result<(), GpuError> {
        let resp = self.call(req, 24)?;
        match Self::resp_type(&resp) {
            RESP_OK_NODATA => Ok(()),
            e => Err(GpuError::Device(e)),
        }
    }

    /// Создаёт 3D-ресурс и привязывает его к контексту `ctx`.
    ///
    /// Возвращает: идентификатор ресурса (он же handle в потоке virgl).
    pub fn resource_create_3d(&mut self, ctx: u32, r: Resource3d) -> Result<u32, GpuError> {
        let id = self.next_res;
        self.next_res += 1;
        let mut req = Vec::new();
        hdr(&mut req, CMD_RESOURCE_CREATE_3D, 0, 0, ctx);
        // resource_id, target, format, bind, width, height, depth,
        // array_size, last_level, nr_samples, flags, padding
        put(&mut req, &[id, r.target, r.format, r.bind, r.width, r.height, 1, 1, 0, 0, 0, 0]);
        self.expect_ok(req)?;
        let mut req = Vec::new();
        hdr(&mut req, CMD_CTX_ATTACH_RESOURCE, 0, 0, ctx);
        put(&mut req, &[id, 0]);
        self.expect_ok(req)?;
        Ok(id)
    }

    /// Создаёт 2D-ресурс (режим без virgl). Формат 1 — B8G8R8A8.
    pub fn resource_create_2d(&mut self, format: u32, w: u32, h: u32) -> Result<u32, GpuError> {
        let id = self.next_res;
        self.next_res += 1;
        let mut req = Vec::new();
        hdr(&mut req, CMD_RESOURCE_CREATE_2D, 0, 0, 0);
        put(&mut req, &[id, format, w, h]);
        self.expect_ok(req)?;
        Ok(id)
    }

    /// Подключает гостевую память (непрерывную) как хранилище ресурса.
    pub fn attach_backing(&mut self, res: u32, addr: u64, len: u32) -> Result<(), GpuError> {
        let mut req = Vec::new();
        hdr(&mut req, CMD_RESOURCE_ATTACH_BACKING, 0, 0, 0);
        put(&mut req, &[res, 1]);
        req.extend_from_slice(&addr.to_le_bytes());
        put(&mut req, &[len, 0]);
        self.expect_ok(req)
    }

    /// Назначает ресурс изображением scanout'а (монитора).
    pub fn set_scanout(&mut self, scanout: u32, res: u32, w: u32, h: u32) -> Result<(), GpuError> {
        let mut req = Vec::new();
        hdr(&mut req, CMD_SET_SCANOUT, 0, 0, 0);
        put(&mut req, &[0, 0, w, h, scanout, res]);
        self.expect_ok(req)
    }

    /// Копирует область гостевой памяти ресурса в его копию на GPU хоста.
    ///
    /// Принимает: контекст, ресурс, прямоугольник `x, y, w, h` (для буфера —
    /// `x` = смещение, `w` = длина в байтах), смещение в backing, шаг строки.
    pub fn transfer_to_host_3d(&mut self, ctx: u32, res: u32, x: u32, y: u32, w: u32, h: u32, offset: u64, stride: u32) -> Result<(), GpuError> {
        let mut req = Vec::new();
        hdr(&mut req, CMD_TRANSFER_TO_HOST_3D, 0, 0, ctx);
        put(&mut req, &[x, y, 0, w, h, 1]);
        req.extend_from_slice(&offset.to_le_bytes());
        put(&mut req, &[res, 0, stride, 0]);
        self.queue(req)
    }

    /// 2D-передача (режим без virgl).
    pub fn transfer_to_host_2d(&mut self, res: u32, x: u32, y: u32, w: u32, h: u32, offset: u64) -> Result<(), GpuError> {
        let mut req = Vec::new();
        hdr(&mut req, CMD_TRANSFER_TO_HOST_2D, 0, 0, 0);
        put(&mut req, &[x, y, w, h]);
        req.extend_from_slice(&offset.to_le_bytes());
        put(&mut req, &[res, 0]);
        self.queue(req)
    }

    /// Отправляет поток команд virgl (`dwords`) в контекст `ctx`.
    pub fn submit_3d(&mut self, ctx: u32, dwords: &[u32]) -> Result<(), GpuError> {
        let mut req = Vec::with_capacity(32 + dwords.len() * 4);
        let f = self.fence;
        self.fence += 1;
        hdr(&mut req, CMD_SUBMIT_3D, FLAG_FENCE, f, ctx);
        put(&mut req, &[(dwords.len() * 4) as u32, 0]);
        for d in dwords {
            req.extend_from_slice(&d.to_le_bytes());
        }
        self.queue(req)
    }

    /// Показывает обновлённую область ресурса на экране.
    pub fn flush(&mut self, res: u32, w: u32, h: u32) -> Result<(), GpuError> {
        let mut req = Vec::new();
        hdr(&mut req, CMD_RESOURCE_FLUSH, 0, 0, 0);
        put(&mut req, &[0, 0, w, h, res, 0]);
        self.queue(req)
    }

    /// Описание устройства для журнала.
    pub fn describe(&self) -> String {
        alloc::format!(
            "virtio-gpu {:02x}:{:02x}.{} virgl={} scanouts={}",
            self.pci.address.bus,
            self.pci.address.device,
            self.pci.address.function,
            self.virgl,
            self.scanouts
        )
    }
}
