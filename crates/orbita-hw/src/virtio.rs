//! Транспорт virtio 1.x поверх PCI (modern) и split-virtqueue.
//!
//! Используется драйверами `virtio-gpu` (рендер на реальной видеокарте
//! хоста через virgl) и `virtio-input` (абсолютный указатель).
//!
//! ```text
//!  PCI config space ──► vendor capabilities (cfg_type 1..4)
//!        │                 COMMON  — фичи, статус, настройка очередей
//!        │                 NOTIFY  — «дверной звонок» очереди
//!        │                 ISR     — причина прерывания
//!        │                 DEVICE  — конфигурация конкретного устройства
//!        ▼
//!   VirtioPci::probe → negotiate(features) → setup_queue(i) → DRIVER_OK
//!                                              │
//!                                  Virtqueue: desc[] / avail / used
//! ```
//!
//! Ядро отображает память тождественно (физический адрес = виртуальный),
//! поэтому адрес буфера в куче сразу годится для DMA.

extern crate alloc;

use alloc::vec;
use alloc::vec::Vec;
use core::ptr::{read_volatile, write_volatile};
use core::sync::atomic::{Ordering, fence};

use crate::pci::{PciAddress, PciBarKind, PciDevice, enable_bus_master, read_config_u8, read_config_u32};

/// Бит `VIRTIO_F_VERSION_1` — обязательный для modern-устройств.
pub const VIRTIO_F_VERSION_1: u64 = 1 << 32;

const STATUS_ACKNOWLEDGE: u8 = 1;
const STATUS_DRIVER: u8 = 2;
const STATUS_DRIVER_OK: u8 = 4;
const STATUS_FEATURES_OK: u8 = 8;
const STATUS_FAILED: u8 = 128;

const CAP_COMMON: u8 = 1;
const CAP_NOTIFY: u8 = 2;
const CAP_ISR: u8 = 3;
const CAP_DEVICE: u8 = 4;

// Смещения в `struct virtio_pci_common_cfg`.
const C_DFSELECT: usize = 0x00;
const C_DF: usize = 0x04;
const C_GFSELECT: usize = 0x08;
const C_GF: usize = 0x0C;
const C_NUM_QUEUES: usize = 0x12;
const C_STATUS: usize = 0x14;
const C_Q_SELECT: usize = 0x16;
const C_Q_SIZE: usize = 0x18;
const C_Q_MSIX: usize = 0x1A;
const C_Q_ENABLE: usize = 0x1C;
const C_Q_NOFF: usize = 0x1E;
const C_Q_DESC: usize = 0x20;
const C_Q_DRIVER: usize = 0x28;
const C_Q_DEVICE: usize = 0x30;

/// Ошибки транспорта.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VirtioError {
    /// Нет нужной vendor-capability или BAR.
    NoCapability,
    /// Устройство не приняло набор фич.
    FeaturesRejected,
    /// Очередь недоступна (размер 0).
    QueueUnavailable,
    /// В очереди нет свободных дескрипторов.
    QueueFull,
    /// Устройство не ответило за отведённое время.
    Timeout,
}

/// Подключение к virtio-устройству через PCI.
pub struct VirtioPci {
    /// Адрес устройства на шине.
    pub address: PciAddress,
    common: *mut u8,
    notify: *mut u8,
    notify_mult: u32,
    isr: *mut u8,
    device: *mut u8,
}

// Устройство используется из одного потока ядра.
unsafe impl Send for VirtioPci {}

fn bar_base(dev: &PciDevice, bar: u8) -> Option<u64> {
    let b = dev.bars.get(bar as usize).copied().flatten()?;
    if b.kind == PciBarKind::IoPort || b.base == 0 {
        return None;
    }
    Some(b.base)
}

impl VirtioPci {
    /// Находит регионы конфигурации по vendor-capabilities устройства.
    ///
    /// Принимает: PCI-устройство (modern virtio).
    /// Возвращает: транспорт или [`VirtioError::NoCapability`].
    pub fn probe(dev: &PciDevice) -> Result<VirtioPci, VirtioError> {
        let a = dev.address;
        let mut common = None;
        let mut notify = None;
        let mut notify_mult = 0;
        let mut isr = None;
        let mut device = None;
        // Обходим список capabilities сами: их может быть больше, чем
        // запоминает инвентаризация PCI.
        let mut off = read_config_u8(a, 0x34) & !3;
        let mut guard = 0;
        while off >= 0x40 && guard < 48 {
            guard += 1;
            let id = read_config_u8(a, off);
            let next = read_config_u8(a, off + 1) & !3;
            if id == 0x09 {
                let cfg_type = read_config_u8(a, off + 3);
                let bar = read_config_u8(a, off + 4);
                let offset = read_config_u32(a, off + 8) as u64;
                if let Some(base) = bar_base(dev, bar) {
                    let ptr = (base + offset) as *mut u8;
                    match cfg_type {
                        CAP_COMMON if common.is_none() => common = Some(ptr),
                        CAP_NOTIFY if notify.is_none() => {
                            notify = Some(ptr);
                            notify_mult = read_config_u32(a, off + 16);
                        }
                        CAP_ISR if isr.is_none() => isr = Some(ptr),
                        CAP_DEVICE if device.is_none() => device = Some(ptr),
                        _ => {}
                    }
                }
            }
            if next == 0 {
                break;
            }
            off = next;
        }
        let (Some(common), Some(notify), Some(isr)) = (common, notify, isr) else {
            return Err(VirtioError::NoCapability);
        };
        enable_bus_master(a);
        Ok(VirtioPci { address: a, common, notify, notify_mult, isr, device: device.unwrap_or(core::ptr::null_mut()) })
    }

    fn r8(&self, off: usize) -> u8 {
        unsafe { read_volatile(self.common.add(off)) }
    }
    fn w8(&self, off: usize, v: u8) {
        unsafe { write_volatile(self.common.add(off), v) }
    }
    fn r16(&self, off: usize) -> u16 {
        unsafe { read_volatile(self.common.add(off) as *const u16) }
    }
    fn w16(&self, off: usize, v: u16) {
        unsafe { write_volatile(self.common.add(off) as *mut u16, v) }
    }
    fn r32(&self, off: usize) -> u32 {
        unsafe { read_volatile(self.common.add(off) as *const u32) }
    }
    fn w32(&self, off: usize, v: u32) {
        unsafe { write_volatile(self.common.add(off) as *mut u32, v) }
    }
    fn w64(&self, off: usize, v: u64) {
        self.w32(off, v as u32);
        self.w32(off + 4, (v >> 32) as u32);
    }

    /// Сбрасывает устройство и согласует фичи.
    ///
    /// Принимает: желаемые фичи (`VIRTIO_F_VERSION_1` добавляется сам).
    /// Возвращает: фичи, которые приняты обеими сторонами.
    pub fn negotiate(&mut self, wanted: u64) -> Result<u64, VirtioError> {
        self.w8(C_STATUS, 0);
        let mut spins = 0;
        while self.r8(C_STATUS) != 0 && spins < 1_000_000 {
            spins += 1;
            for _ in 0..200 {
                core::hint::spin_loop(); // не душим QEMU опросом MMIO
            }
        }
        self.w8(C_STATUS, STATUS_ACKNOWLEDGE);
        self.w8(C_STATUS, STATUS_ACKNOWLEDGE | STATUS_DRIVER);
        self.w32(C_DFSELECT, 0);
        let lo = self.r32(C_DF) as u64;
        self.w32(C_DFSELECT, 1);
        let hi = self.r32(C_DF) as u64;
        let offered = lo | (hi << 32);
        let accepted = offered & (wanted | VIRTIO_F_VERSION_1);
        self.w32(C_GFSELECT, 0);
        self.w32(C_GF, accepted as u32);
        self.w32(C_GFSELECT, 1);
        self.w32(C_GF, (accepted >> 32) as u32);
        self.w8(C_STATUS, STATUS_ACKNOWLEDGE | STATUS_DRIVER | STATUS_FEATURES_OK);
        if self.r8(C_STATUS) & STATUS_FEATURES_OK == 0 {
            self.w8(C_STATUS, STATUS_FAILED);
            return Err(VirtioError::FeaturesRejected);
        }
        Ok(accepted)
    }

    /// Число очередей устройства.
    pub fn num_queues(&self) -> u16 {
        self.r16(C_NUM_QUEUES)
    }

    /// Создаёт и включает очередь `index` (размер — не больше `max_size`).
    pub fn setup_queue(&mut self, index: u16, max_size: u16) -> Result<Virtqueue, VirtioError> {
        self.w16(C_Q_SELECT, index);
        let dev_size = self.r16(C_Q_SIZE);
        if dev_size == 0 {
            return Err(VirtioError::QueueUnavailable);
        }
        let size = dev_size.min(max_size).max(1);
        self.w16(C_Q_SIZE, size);
        self.w16(C_Q_MSIX, 0xFFFF); // без прерываний: опрос used-кольца
        let q = Virtqueue::new(index, size);
        self.w64(C_Q_DESC, q.desc_addr());
        self.w64(C_Q_DRIVER, q.avail_addr());
        self.w64(C_Q_DEVICE, q.used_addr());
        let noff = self.r16(C_Q_NOFF) as usize;
        let mut q = q;
        q.notify = unsafe { self.notify.add(noff * self.notify_mult as usize) } as *mut u16;
        self.w16(C_Q_ENABLE, 1);
        Ok(q)
    }

    /// Сообщает устройству, что драйвер готов.
    pub fn driver_ok(&mut self) {
        let s = self.r8(C_STATUS);
        self.w8(C_STATUS, s | STATUS_DRIVER_OK);
    }

    /// Читает 32-битное поле конфигурации устройства.
    pub fn device_cfg_u32(&self, off: usize) -> u32 {
        if self.device.is_null() {
            return 0;
        }
        unsafe { read_volatile(self.device.add(off) as *const u32) }
    }

    /// Пишет 32-битное поле конфигурации устройства.
    pub fn set_device_cfg_u32(&self, off: usize, v: u32) {
        if !self.device.is_null() {
            unsafe { write_volatile(self.device.add(off) as *mut u32, v) }
        }
    }

    /// Пишет байт конфигурации устройства.
    pub fn set_device_cfg_u8(&self, off: usize, v: u8) {
        if !self.device.is_null() {
            unsafe { write_volatile(self.device.add(off), v) }
        }
    }

    /// Читает байт конфигурации устройства.
    pub fn device_cfg_u8(&self, off: usize) -> u8 {
        if self.device.is_null() {
            return 0;
        }
        unsafe { read_volatile(self.device.add(off)) }
    }

    /// Сбрасывает флаг прерывания (чтение ISR).
    pub fn ack_isr(&self) -> u8 {
        unsafe { read_volatile(self.isr) }
    }
}

#[repr(C, align(16))]
#[derive(Clone, Copy, Default)]
struct Desc {
    addr: u64,
    len: u32,
    flags: u16,
    next: u16,
}

const DESC_F_NEXT: u16 = 1;
const DESC_F_WRITE: u16 = 2;

/// Split-virtqueue: таблица дескрипторов, кольцо avail (драйвер → устройство)
/// и кольцо used (устройство → драйвер).
pub struct Virtqueue {
    /// Номер очереди.
    pub index: u16,
    size: u16,
    storage: Vec<u8>,
    desc_off: usize,
    avail_off: usize,
    used_off: usize,
    free: Vec<u16>,
    last_used: u16,
    avail_idx: u16,
    notify: *mut u16,
    in_flight: usize,
}

unsafe impl Send for Virtqueue {}

impl Virtqueue {
    fn new(index: u16, size: u16) -> Virtqueue {
        let n = size as usize;
        let desc_bytes = 16 * n;
        let avail_bytes = 6 + 2 * n;
        let used_bytes = 6 + 8 * n;
        // Выравнивание: desc 16, avail 2, used 4 — кладём всё с запасом по 4096.
        let total = desc_bytes + avail_bytes + used_bytes + 3 * 4096;
        let storage = vec![0u8; total];
        let base = storage.as_ptr() as usize;
        let desc_off = ((base + 4095) & !4095) - base;
        let avail_off = desc_off + ((desc_bytes + 15) & !15);
        let used_off = ((base + avail_off + avail_bytes + 4095) & !4095) - base;
        Virtqueue {
            index,
            size,
            storage,
            desc_off,
            avail_off,
            used_off,
            free: (0..size).rev().collect(),
            last_used: 0,
            avail_idx: 0,
            notify: core::ptr::null_mut(),
            in_flight: 0,
        }
    }

    fn base(&self) -> *mut u8 {
        self.storage.as_ptr() as *mut u8
    }
    fn desc_addr(&self) -> u64 {
        self.base() as u64 + self.desc_off as u64
    }
    fn avail_addr(&self) -> u64 {
        self.base() as u64 + self.avail_off as u64
    }
    fn used_addr(&self) -> u64 {
        self.base() as u64 + self.used_off as u64
    }
    fn desc(&self, i: u16) -> *mut Desc {
        unsafe { (self.base().add(self.desc_off) as *mut Desc).add(i as usize) }
    }

    /// Размер очереди (дескрипторов).
    pub fn size(&self) -> u16 {
        self.size
    }

    /// Свободных дескрипторов.
    pub fn free_count(&self) -> usize {
        self.free.len()
    }

    /// Ставит в очередь цепочку буферов: сначала читаемые устройством
    /// (`out`), затем записываемые (`inp`). Не ждёт завершения.
    ///
    /// Буферы должны жить до [`Virtqueue::wait_all`]/[`Virtqueue::collect`].
    pub fn push(&mut self, out: &[(u64, u32)], inp: &[(u64, u32)]) -> Result<u16, VirtioError> {
        let n = out.len() + inp.len();
        if n == 0 || self.free.len() < n {
            return Err(VirtioError::QueueFull);
        }
        let mut ids: Vec<u16> = Vec::with_capacity(n);
        for _ in 0..n {
            ids.push(self.free.pop().unwrap());
        }
        for (k, (addr, len)) in out.iter().chain(inp.iter()).enumerate() {
            let write = k >= out.len();
            let mut flags = if write { DESC_F_WRITE } else { 0 };
            let next = if k + 1 < n {
                flags |= DESC_F_NEXT;
                ids[k + 1]
            } else {
                0
            };
            unsafe { write_volatile(self.desc(ids[k]), Desc { addr: *addr, len: *len, flags, next }) };
        }
        let head = ids[0];
        unsafe {
            let ring = self.base().add(self.avail_off + 4) as *mut u16;
            write_volatile(ring.add((self.avail_idx % self.size) as usize), head);
            fence(Ordering::SeqCst);
            self.avail_idx = self.avail_idx.wrapping_add(1);
            write_volatile(self.base().add(self.avail_off + 2) as *mut u16, self.avail_idx);
        }
        self.in_flight += 1;
        Ok(head)
    }

    /// «Звонок» устройству: в очереди есть новые цепочки.
    pub fn kick(&self) {
        fence(Ordering::SeqCst);
        if !self.notify.is_null() {
            unsafe { write_volatile(self.notify, self.index) };
        }
    }

    /// Забирает завершённые цепочки; возвращает их число.
    pub fn collect(&mut self) -> usize {
        let mut done = 0;
        loop {
            fence(Ordering::SeqCst);
            let used_idx = unsafe { read_volatile(self.base().add(self.used_off + 2) as *const u16) };
            if used_idx == self.last_used {
                break;
            }
            let slot = (self.last_used % self.size) as usize;
            let elem = unsafe { self.base().add(self.used_off + 4 + slot * 8) as *const u32 };
            let mut id = unsafe { read_volatile(elem) } as u16;
            // Возвращаем все дескрипторы цепочки в свободный список.
            loop {
                let d = unsafe { read_volatile(self.desc(id)) };
                self.free.push(id);
                if d.flags & DESC_F_NEXT == 0 {
                    break;
                }
                id = d.next;
            }
            self.last_used = self.last_used.wrapping_add(1);
            self.in_flight = self.in_flight.saturating_sub(1);
            done += 1;
        }
        done
    }

    /// Забирает одну завершённую цепочку: (головной дескриптор, записано байт).
    pub fn pop_used(&mut self) -> Option<(u16, u32)> {
        fence(Ordering::SeqCst);
        let used_idx = unsafe { read_volatile(self.base().add(self.used_off + 2) as *const u16) };
        if used_idx == self.last_used {
            return None;
        }
        let slot = (self.last_used % self.size) as usize;
        let elem = unsafe { self.base().add(self.used_off + 4 + slot * 8) as *const u32 };
        let head = unsafe { read_volatile(elem) } as u16;
        let len = unsafe { read_volatile(elem.add(1)) };
        let mut id = head;
        loop {
            let d = unsafe { read_volatile(self.desc(id)) };
            self.free.push(id);
            if d.flags & DESC_F_NEXT == 0 {
                break;
            }
            id = d.next;
        }
        self.last_used = self.last_used.wrapping_add(1);
        self.in_flight = self.in_flight.saturating_sub(1);
        Some((head, len))
    }

    /// Ждёт завершения всех цепочек в полёте.
    pub fn wait_all(&mut self) -> Result<(), VirtioError> {
        let mut spins: u64 = 0;
        while self.in_flight > 0 {
            if self.collect() == 0 {
                spins += 1;
                if spins > 4_000_000_000 {
                    return Err(VirtioError::Timeout);
                }
                core::hint::spin_loop();
            }
        }
        Ok(())
    }

    /// Цепочек в полёте.
    pub fn in_flight(&self) -> usize {
        self.in_flight
    }

    /// Синхронный запрос: один читаемый и один записываемый буфер.
    pub fn request(&mut self, req: &[u8], resp: &mut [u8]) -> Result<(), VirtioError> {
        self.push(&[(req.as_ptr() as u64, req.len() as u32)], &[(resp.as_mut_ptr() as u64, resp.len() as u32)])?;
        self.kick();
        self.wait_all()
    }
}
