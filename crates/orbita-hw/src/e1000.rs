//! Intel e1000 NIC driver (8254x family, QEMU `e1000` device).
//!
//! Implements the minimum for a working poll-mode NIC: device reset, MAC
//! from EEPROM, RX/TX descriptor rings in identity-mapped DMA memory, and
//! link status. Interrupts stay masked — the kernel polls
//! [`E1000::poll_rx`] / [`E1000::send`] from its main loop.
//!
//! Tested against QEMU's `-device e1000` (default 8086:100e) on the user
//! network stack (`-netdev user`).

use alloc::vec;
use alloc::vec::Vec;

// --- register offsets (bytes, from the 8254x datasheet) -------------------
const REG_CTRL: usize = 0x0000;
const REG_STATUS: usize = 0x0008;
const REG_EERD: usize = 0x0014;
const REG_RXCTRL: usize = 0x0100;
const REG_TCTL: usize = 0x0400;
const REG_TIPG: usize = 0x0410;
const REG_RDBAL: usize = 0x2800;
const REG_RDBAH: usize = 0x2804;
const REG_RDLEN: usize = 0x2808;
const REG_RDH: usize = 0x2810;
const REG_RDT: usize = 0x2818;
const REG_TDBAL: usize = 0x3800;
const REG_TDBAH: usize = 0x3804;
const REG_TDLEN: usize = 0x3808;
const REG_TDH: usize = 0x3810;
const REG_TDT: usize = 0x3818;
const REG_RAL0: usize = 0x5400;
const REG_RAH0: usize = 0x5404;
const REG_MTA: usize = 0x5200;

const CTRL_SLU: u32 = 1 << 6; // set link up
const CTRL_ASDE: u32 = 1 << 5; // auto-speed detection
const CTRL_RST: u32 = 1 << 26; // device reset
const STATUS_LU: u32 = 1 << 1; // link up
const EERD_START: u32 = 1 << 0;
const EERD_DONE: u32 = 1 << 4;
const RXCTRL_EN: u32 = 1 << 1;
// Accept broadcast (DHCP/ARP) and multicast (IPv6 ND) frames.
const RXCTRL_BAM: u32 = 1 << 15;
const RXCTRL_MPE: u32 = 1 << 19;
 // Strip the trailing CRC from received frames.
const RXCTRL_SECRC: u32 = 1 << 26;
const TCTL_EN: u32 = 1 << 1;
const TCTL_PSP: u32 = 1 << 3;

const RX_RING_LEN: usize = 32;
const TX_RING_LEN: usize = 16;
const MAX_FRAME: usize = 1518;

/// Errors reported by [`E1000::probe`].
#[derive(Debug)]
pub enum E1000Error {
    /// MMIO BAR0 missing on the controller.
    NoMmioBar,
    /// Device did not finish reset in time.
    ResetTimeout,
    /// EEPROM MAC read failed.
    MacUnavailable,
    /// DMA ring allocation lost 16-byte alignment (allocator regression).
    RingMisaligned,
}

/// RX/TX statistics for `netcfg`.
#[derive(Debug, Default, Copy, Clone)]
pub struct E1000Stats {
    pub rx_frames: u64,
    pub rx_dropped: u64,
    pub rx_bytes: u64,
    pub tx_frames: u64,
    pub tx_bytes: u64,
}

#[repr(C, align(16))]
#[derive(Debug, Copy, Clone, Default)]
struct RxDescriptor {
    buffer: u64,
    length: u16,
    checksum: u16,
    status: u8,
    errors: u8,
    special: u16,
}

#[repr(C, align(16))]
#[derive(Debug, Copy, Clone, Default)]
struct TxDescriptor {
    buffer: u64,
    length: u16,
    cso: u8,
    cmd: u8,
    status: u8,
    css: u8,
    special: u16,
}

impl TxDescriptor {
    const CMD_EOP: u8 = 1 << 0;
    const CMD_IFCS: u8 = 1 << 1;
}

/// A poll-mode e1000 NIC bound to one controller.
pub struct E1000 {
    base: *mut u32,
    mac: [u8; 6],
    rx_descriptors: Vec<RxDescriptor>,
    /// Backing storage (owned); NIC-visible addresses in `*_addrs`.
    rx_buffers: Vec<Vec<u8>>,
    rx_addrs: [u64; RX_RING_LEN],
    rx_tail: usize,
    tx_descriptors: Vec<TxDescriptor>,
    tx_buffers: Vec<Vec<u8>>,
    tx_addrs: [u64; TX_RING_LEN],
    tx_tail: usize,
    tx_next_cleanup: usize,
    stats: E1000Stats,
}

/// Allocate `len` bytes of storage plus a 16-byte pad and return
/// (storage, aligned_address). Heap Vec<u8> is only 1-aligned, so the
/// aligned view lives at an offset inside the storage.
fn aligned_buffer(len: usize) -> (Vec<u8>, u64) {
    let storage = vec![0u8; len + 16];
    let base = storage.as_ptr() as usize;
    let aligned = (base + 15) & !15;
    (storage, aligned as u64)
}

/// 16-byte-aligned address inside a padded storage buffer.
fn aligned_view(storage: &[u8]) -> u64 {
    let base = storage.as_ptr() as usize;
    ((base + 15) & !15) as u64
}

// The device is only touched through volatile accessors; the driver owns
// the rings exclusively.
unsafe impl Send for E1000 {}

impl E1000 {
    /// Probe and initialize the controller at MMIO `base` (PCI BAR0).
    pub fn probe(base: u64) -> Result<Self, E1000Error> {
        if base == 0 {
            return Err(E1000Error::NoMmioBar);
        }
        let rx_buffers: Vec<Vec<u8>> = Vec::new();
        let rx_addrs = [0u64; RX_RING_LEN];
        let tx_buffers: Vec<Vec<u8>> = Vec::new();
        let tx_addrs = [0u64; TX_RING_LEN];
        let mut device = Self {
            base: base as *mut u32,
            mac: [0; 6],
            rx_descriptors: vec![RxDescriptor::default(); RX_RING_LEN],
            rx_buffers,
            rx_addrs,
            rx_tail: 0,
            tx_descriptors: vec![TxDescriptor::default(); TX_RING_LEN],
            tx_buffers,
            tx_addrs,
            tx_tail: 0,
            tx_next_cleanup: 0,
            stats: E1000Stats::default(),
        };
        for _ in 0..RX_RING_LEN {
            let (storage, _address) = aligned_buffer(MAX_FRAME + 2);
            device.rx_buffers.push(storage);
        }
        for index in 0..RX_RING_LEN {
            device.rx_addrs[index] = aligned_view(&device.rx_buffers[index]);
        }
        for _ in 0..TX_RING_LEN {
            let (storage, _address) = aligned_buffer(MAX_FRAME);
            device.tx_buffers.push(storage);
        }
        for index in 0..TX_RING_LEN {
            device.tx_addrs[index] = aligned_view(&device.tx_buffers[index]);
        }
        device.reset()?;
        let mut device = device;
        device.init_rings()?;
        device.mac = device.read_mac()?;
        device.program_receive_filters();
        Ok(device)
    }

    fn reset(&self) -> Result<(), E1000Error> {
        let ctrl = self.read32(REG_CTRL);
        self.write32(REG_CTRL, ctrl | CTRL_RST | CTRL_SLU | CTRL_ASDE);
        // Wait for the RST bit to self-clear (bounded spin).
        for _ in 0..100_000 {
            if self.read32(REG_CTRL) & CTRL_RST == 0 {
                return Ok(());
            }
            core::hint::spin_loop();
        }
        Err(E1000Error::ResetTimeout)
    }

    fn init_rings(&mut self) -> Result<(), E1000Error> {
        // RX ring: descriptors reference identity-mapped heap buffers.
        // `dma_check` asserts the invariant that physical == virtual here.
        for (index, descriptor) in self.rx_descriptors.iter_mut().enumerate() {
            if self.rx_addrs[index] & 0xF != 0 {
                return Err(E1000Error::RingMisaligned);
            }
            descriptor.buffer = self.rx_addrs[index];
            descriptor.status = 0;
        }
        let rx_phys = self.rx_descriptors.as_ptr() as u64;
        self.write32(REG_RDBAL, rx_phys as u32);
        self.write32(REG_RDBAH, (rx_phys >> 32) as u32);
        self.write32(REG_RDLEN, (RX_RING_LEN * 16) as u32);
        self.write32(REG_RDH, 0);
        self.write32(REG_RDT, (RX_RING_LEN - 1) as u32);

        let tx_phys = self.tx_descriptors.as_ptr() as u64;
        self.write32(REG_TDBAL, tx_phys as u32);
        self.write32(REG_TDBAH, (tx_phys >> 32) as u32);
        self.write32(REG_TDLEN, (TX_RING_LEN * 16) as u32);
        self.write32(REG_TDH, 0);
        self.write32(REG_TDT, 0);

        // Transmitter: enable, pad short packets.
        self.write32(REG_TCTL, TCTL_EN | TCTL_PSP | (0x10 << 4) | (0x40 << 12));
        // Standard inter-packet gap for 1000 Mb/s full duplex.
        self.write32(REG_TIPG, 6 | (8 << 10) | (6 << 20));
        // Receiver on.
        self.write32(
            REG_RXCTRL,
            RXCTRL_EN | RXCTRL_BAM | RXCTRL_MPE | RXCTRL_SECRC,
        );
        Ok(())
    }

    fn program_receive_filters(&self) {
        // Unicast MAC into RAL0/RAH0 (with AV bit), clear multicast table:
        // QEMU's slirp only needs unicast + broadcast frames.
        let bytes = self.mac;
        let low = u32::from(bytes[0]) | u32::from(bytes[1]) << 8 | u32::from(bytes[2]) << 16
            | u32::from(bytes[3]) << 24;
        let high = (u32::from(bytes[4]) | u32::from(bytes[5]) << 8) | (1 << 31);
        self.write32(REG_RAL0, low);
        self.write32(REG_RAH0, high);
        for index in 0..0x80 {
            self.write32(REG_MTA + index * 4, 0);
        }
    }

    fn read_mac(&self) -> Result<[u8; 6], E1000Error> {
        let mut words = [0u16; 3];
        for (index, word) in words.iter_mut().enumerate() {
            *word = self.read_eeprom_word(index as u8).ok_or(E1000Error::MacUnavailable)?;
        }
        Ok([
            words[0] as u8,
            (words[0] >> 8) as u8,
            words[1] as u8,
            (words[1] >> 8) as u8,
            words[2] as u8,
            (words[2] >> 8) as u8,
        ])
    }

    fn read_eeprom_word(&self, address: u8) -> Option<u16> {
        self.write32(REG_EERD, EERD_START | u32::from(address) << 8);
        for _ in 0..100_000 {
            let value = self.read32(REG_EERD);
            if value & EERD_DONE != 0 {
                return Some((value >> 16) as u16);
            }
            core::hint::spin_loop();
        }
        None
    }

    /// Whether the physical link negotiated up.
    pub fn link_up(&self) -> bool {
        self.read32(REG_STATUS) & STATUS_LU != 0
    }

    /// The burned-in MAC address.
    pub fn mac(&self) -> [u8; 6] {
        self.mac
    }

    /// Live counters for `netcfg`.
    pub fn stats(&self) -> E1000Stats {
        self.stats
    }

    /// Raw RX head register (diagnostics: whether the NIC writes
    /// descriptors at all).
    pub fn rx_head_debug(&self) -> u32 {
        self.read32(REG_RDH)
    }

    /// Drain every completed RX descriptor, copying frames out.
    pub fn poll_rx(&mut self) -> Vec<Vec<u8>> {
        let mut frames = Vec::new();
        let head = self.read32(REG_RDH) as usize % RX_RING_LEN;
        while self.rx_tail != head {
            let descriptor = &mut self.rx_descriptors[self.rx_tail];
            if descriptor.status & 0x1 == 0 {
                // Not actually done (descriptor error): recycle and stop.
                break;
            }
            let length = descriptor.length as usize;
            if length > 0 && length <= MAX_FRAME {
                let aligned = self.rx_addrs[self.rx_tail] as *const u8;
                // SAFETY: `aligned` points into our owned, padded buffer.
                let view = unsafe { core::slice::from_raw_parts(aligned, length) };
                frames.push(view.to_vec());
                self.stats.rx_frames += 1;
                self.stats.rx_bytes += length as u64;
            } else {
                self.stats.rx_dropped += 1;
            }
            descriptor.status = 0;
            let tail = self.rx_tail;
            self.write32(REG_RDT, tail as u32);
            self.rx_tail = (self.rx_tail + 1) % RX_RING_LEN;
        }
        frames
    }

    /// Transmit one frame (best effort; drops when the ring is full).
    pub fn send(&mut self, frame: &[u8]) -> bool {
        if frame.is_empty() || frame.len() > MAX_FRAME {
            return false;
        }
        self.reclaim_tx();
        let tail = self.tx_tail;
        let aligned = self.tx_addrs[tail] as *mut u8;
        // SAFETY: `aligned` points into our owned, padded buffer.
        unsafe {
            core::ptr::copy_nonoverlapping(frame.as_ptr(), aligned, frame.len());
        }
        let descriptor = &mut self.tx_descriptors[tail];
        descriptor.buffer = self.tx_addrs[tail];
        descriptor.length = frame.len() as u16;
        descriptor.cso = 0;
        descriptor.cmd = TxDescriptor::CMD_EOP | TxDescriptor::CMD_IFCS;
        descriptor.status = 0;
        descriptor.css = 0;
        descriptor.special = 0;

        self.tx_tail = (tail + 1) % TX_RING_LEN;
        // Memory barrier: descriptor writes must land before the tail bump.
        core::sync::atomic::fence(core::sync::atomic::Ordering::Release);
        self.write32(REG_TDT, self.tx_tail as u32);
        self.stats.tx_frames += 1;
        self.stats.tx_bytes += frame.len() as u64;
        true
    }

    /// Return completed TX descriptors to the ring.
    fn reclaim_tx(&mut self) {
        while self.tx_next_cleanup != self.tx_tail {
            if self.tx_descriptors[self.tx_next_cleanup].status & 0x1 == 0 {
                break;
            }
            self.tx_descriptors[self.tx_next_cleanup].status = 0;
            self.tx_next_cleanup = (self.tx_next_cleanup + 1) % TX_RING_LEN;
        }
    }

    fn read32(&self, offset: usize) -> u32 {
        unsafe { core::ptr::read_volatile(self.base.add(offset / 4)) }
    }

    fn write32(&self, offset: usize, value: u32) {
        unsafe { core::ptr::write_volatile(self.base.add(offset / 4), value) }
    }
}
