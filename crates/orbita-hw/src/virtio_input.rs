//! Драйвер `virtio-input` (QEMU `virtio-tablet-pci`): абсолютный указатель.
//!
//! Планшет сообщает координаты курсора хоста в диапазоне `0..=max` по
//! каждой оси, поэтому мышь в окне QEMU работает без «захвата» и сразу
//! совпадает с курсором Windows. События — структуры evdev Linux.

extern crate alloc;

use alloc::vec;
use alloc::vec::Vec;

use crate::pci::PciDevice;
use crate::virtio::{VirtioError, VirtioPci, Virtqueue};

const EV_SYN: u16 = 0;
const EV_KEY: u16 = 1;
const EV_REL: u16 = 2;
const EV_ABS: u16 = 3;
const BTN_LEFT: u16 = 0x110;
const BTN_RIGHT: u16 = 0x111;
const BTN_MIDDLE: u16 = 0x112;
const REL_WHEEL: u16 = 8;
const ABS_X: u16 = 0;
const ABS_Y: u16 = 1;
const CFG_ABS_INFO: u8 = 0x12;

const EVENT_BUFFERS: usize = 64;

/// Накопленное состояние указателя.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct PointerState {
    /// X в долях ширины экрана `0..=1`.
    pub x: f32,
    /// Y в долях высоты `0..=1`.
    pub y: f32,
    /// Кнопки: бит 0 — левая, 1 — правая, 2 — средняя.
    pub buttons: u8,
    /// Накопленная прокрутка (щелчки колеса, вверх — положительно).
    pub wheel: i32,
    /// Были ли события с момента прошлого чтения.
    pub moved: bool,
}

/// Драйвер планшета.
pub struct VirtioTablet {
    _pci: VirtioPci,
    eventq: Virtqueue,
    buffers: Vec<[u8; 8]>,
    head_to_buf: Vec<u16>,
    max_x: f32,
    max_y: f32,
    state: PointerState,
}

unsafe impl Send for VirtioTablet {}

impl VirtioTablet {
    /// Инициализирует устройство и заполняет очередь событий.
    pub fn init(dev: &PciDevice) -> Result<VirtioTablet, VirtioError> {
        let mut pci = VirtioPci::probe(dev)?;
        pci.negotiate(0)?;
        let mut eventq = pci.setup_queue(0, EVENT_BUFFERS as u16)?;
        let _status = pci.setup_queue(1, 8).ok();
        pci.driver_ok();
        let axis_max = |pci: &VirtioPci, axis: u8| -> f32 {
            pci.set_device_cfg_u8(0, CFG_ABS_INFO);
            pci.set_device_cfg_u8(1, axis);
            // data.abs: min (+8), max (+12)
            let max = pci.device_cfg_u32(12);
            if max == 0 { 32767.0 } else { max as f32 }
        };
        let max_x = axis_max(&pci, 0);
        let max_y = axis_max(&pci, 1);
        let n = (eventq.size() as usize).min(EVENT_BUFFERS);
        let mut buffers = vec![[0u8; 8]; n];
        let mut head_to_buf = vec![0u16; eventq.size() as usize];
        for (i, b) in buffers.iter_mut().enumerate() {
            let head = eventq.push(&[], &[(b.as_mut_ptr() as u64, 8)])?;
            head_to_buf[head as usize] = i as u16;
        }
        eventq.kick();
        Ok(VirtioTablet {
            _pci: pci,
            eventq,
            buffers,
            head_to_buf,
            max_x,
            max_y,
            state: PointerState { x: 0.5, y: 0.5, ..Default::default() },
        })
    }

    /// Разбирает пришедшие события и возвращает текущее состояние.
    ///
    /// Поле `wheel` и флаг `moved` сбрасываются после чтения.
    pub fn poll(&mut self) -> PointerState {
        let mut requeue = Vec::new();
        while let Some((head, _len)) = self.eventq.pop_used() {
            let bi = self.head_to_buf[head as usize] as usize;
            let b = self.buffers[bi];
            let ty = u16::from_le_bytes([b[0], b[1]]);
            let code = u16::from_le_bytes([b[2], b[3]]);
            let value = i32::from_le_bytes([b[4], b[5], b[6], b[7]]);
            match ty {
                EV_ABS if code == ABS_X => {
                    self.state.x = value as f32 / self.max_x;
                    self.state.moved = true;
                }
                EV_ABS if code == ABS_Y => {
                    self.state.y = value as f32 / self.max_y;
                    self.state.moved = true;
                }
                EV_KEY => {
                    let bit = match code {
                        BTN_LEFT => 1,
                        BTN_RIGHT => 2,
                        BTN_MIDDLE => 4,
                        _ => 0,
                    };
                    if value != 0 {
                        self.state.buttons |= bit;
                    } else {
                        self.state.buttons &= !bit;
                    }
                    self.state.moved = true;
                }
                EV_REL if code == REL_WHEEL => {
                    self.state.wheel += value;
                    self.state.moved = true;
                }
                EV_SYN | _ => {}
            }
            requeue.push(bi);
        }
        for bi in requeue {
            let addr = self.buffers[bi].as_mut_ptr() as u64;
            if let Ok(head) = self.eventq.push(&[], &[(addr, 8)]) {
                self.head_to_buf[head as usize] = bi as u16;
            }
        }
        self.eventq.kick();
        let s = self.state;
        self.state.wheel = 0;
        self.state.moved = false;
        s
    }
}
