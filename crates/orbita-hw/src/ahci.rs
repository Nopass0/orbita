//! AHCI (SATA) host controller driver.
//!
//! Drives one port of an AHCI HBA in DMA mode: IDENTIFY DEVICE,
//! READ DMA EXT (0x25) and WRITE DMA EXT (0x35).
//!
//! Register layout note: mapped empirically against QEMU's ICH9 AHCI —
//! the port block (base = ABAR + 0x100 + 0x80*N) keeps PxCLB/PxFB at the
//! front, with PxCI at +0x10, PxTFD at +0x20 and PxSIG at +0x24. The
//! firmware leaves the port engine running with a valid command list,
//! so this driver reuses that configuration instead of re-initializing
//! the engine: it fills command slot 0 and pokes PxCI.

use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

// --- HBA register offsets (bytes) ---
const REG_GHC: usize = 0x04;
const REG_PI: usize = 0x0C;

// --- Port register offsets relative to 0x100 + 0x80 * port ---
// Matches QEMU's ahci.c register map (PxIS/PxIE sit before PxCMD).
const REG_PXCLB_OFF: usize = 0x00;
const REG_PXFB_OFF: usize = 0x08;
const REG_PXIS_OFF: usize = 0x10;
#[allow(dead_code)]
const REG_PXIE_OFF: usize = 0x14;
const REG_PXCMD_OFF: usize = 0x18;
const REG_PXTFD_OFF: usize = 0x20;
const REG_PXSIG_OFF: usize = 0x24;
const REG_PXSERR_OFF: usize = 0x30;
const REG_PXCI_OFF: usize = 0x38;

// --- PxCMD bits ---
const PXCMD_ST: u32 = 1 << 0;
const PXCMD_FRE: u32 = 1 << 4;
const PXCMD_FR: u32 = 1 << 14;
const PXCMD_RUNNING: u32 = 1 << 15;

// --- ATA commands ---
const CMD_IDENTIFY: u8 = 0xEC;
const CMD_READ_DMA_EXT: u8 = 0x25;
const CMD_WRITE_DMA_EXT: u8 = 0x35;

/// SATA signature of a SATA disk (vs ATAPI/enclosure).
const SIG_SATA: u32 = 0x0000_0101;

/// Task-file "ready, no error" value.
#[allow(dead_code)]
const TFD_READY: u32 = 0x50;

/// Physical kind of a storage medium, detected from IDENTIFY data
/// (rotation rate) or the bus type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageKind {
    /// Rotating magnetic disk (spindle).
    Hdd { rpm: u16 },
    /// Non-rotating flash (SSD / eMMC).
    Ssd,
    /// Removable SATA medium.
    Removable,
    /// NVMe controller (PCI class 01/08).
    Nvme,
    /// USB mass-storage flash stick.
    UsbFlash,
    /// RAM-backed disk.
    RamDisk,
    /// Kind not determinable.
    Unknown,
}

impl StorageKind {
    pub fn label(&self) -> String {
        match self {
            StorageKind::Hdd { rpm } => format!("hdd-{rpm}rpm"),
            StorageKind::Ssd => String::from("ssd"),
            StorageKind::Removable => String::from("removable"),
            StorageKind::Nvme => String::from("nvme"),
            StorageKind::UsbFlash => String::from("usb-flash"),
            StorageKind::RamDisk => String::from("ramdisk"),
            StorageKind::Unknown => String::from("unknown"),
        }
    }

    /// Classifies from IDENTIFY word 217 (nominal media rotation rate):
    /// 0x0001 = non-rotating, 0x0000 = not reported, else RPM.
    pub fn from_identify(words: &[u16]) -> Self {
        match words.get(217).copied().unwrap_or(0) {
            0x0001 => StorageKind::Ssd,
            0x0000 => StorageKind::Unknown,
            rpm if rpm >= 0x0401 => StorageKind::Hdd { rpm },
            _ => StorageKind::Unknown,
        }
    }
}

/// One AHCI port driving a SATA disk, reusing the firmware's engine setup.
pub struct AhciDisk {
    port_regs: *mut u32,
    /// Firmware-configured command list base (physical == virtual).
    cmd_list: *mut u8,
    /// Our command table + data buffer, kept alive here.
    #[allow(dead_code)]
    dma_memory: Vec<u8>,
    cmd_table: *mut u8,
    port_index: usize,
    sectors: u64,
    kind: StorageKind,
}

// The driver only touches plain memory and volatile MMIO.
unsafe impl Send for AhciDisk {}

/// Errors during AHCI bring-up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AhciError {
    PortNotImplemented,
    NoDiskSignature,
    FirmwareCommandListMissing,
    CommandStuck,
    EngineTimeout,
    IdentifyFailed,
    IoFailed,
}

/// Kernel-side diagnostic hook, assigned by the kernel at boot.
static mut DEBUG_HOOK: Option<fn(core::fmt::Arguments<'_>)> = None;

pub fn set_debug_hook(hook: fn(core::fmt::Arguments<'_>)) {
    unsafe { DEBUG_HOOK = Some(hook) }
}

pub fn debug_log(args: core::fmt::Arguments<'_>) {
    unsafe {
        if let Some(hook) = DEBUG_HOOK {
            hook(args);
        }
    }
}

impl AhciDisk {
    /// Brings up `port` of the HBA at `hba_base` (physical == virtual
    /// under the firmware identity mapping). `Ok(None)` means the port
    /// exists but holds no SATA disk.
    pub fn probe(hba_base: u64, port: usize) -> Result<Option<Self>, AhciError> {
        let hba = hba_base as *mut u32;

        // Keep GHC.AE set (AHCI enable).
        unsafe { hba.add(REG_GHC / 4).write_volatile(1 << 31) };

        let pi = unsafe { hba.add(REG_PI / 4).read_volatile() };
        if pi & (1 << port) == 0 {
            return Err(AhciError::PortNotImplemented);
        }

        let port_regs = unsafe { hba.add((0x100 + 0x80 * port) / 4) };
        let sig = unsafe { port_regs.add(REG_PXSIG_OFF / 4).read_volatile() };
        if sig != SIG_SATA {
            return Ok(None);
        }

        // Command list (1 KiB, 1 KiB-aligned), FIS receive (256 B,
        // 256-aligned), command table (128-aligned). One heap block.
        let mut dma = vec![0u8; 0x1000 + 0x400 + 0x200];
        let base = dma.as_mut_ptr() as usize;
        let cmd_list = ((base + 0x3FF) & !0x3FF) as *mut u8;
        let fis_rx = (((cmd_list as usize + 0x400) + 0xFF) & !0xFF) as *mut u8;
        let cmd_table = (((fis_rx as usize + 0x100) + 0x7F) & !0x7F) as *mut u8;

        unsafe {
            // Stop the engine, wait for CR/FR to clear.
            let cmd = port_regs.add(REG_PXCMD_OFF / 4).read_volatile();
            port_regs
                .add(REG_PXCMD_OFF / 4)
                .write_volatile(cmd & !(PXCMD_ST | PXCMD_FRE));
            let mut waited = 0;
            loop {
                let cmd = port_regs.add(REG_PXCMD_OFF / 4).read_volatile();
                if cmd & (PXCMD_RUNNING | PXCMD_FR) == 0 || waited > 1_000_000 {
                    break;
                }
                waited += 1;
                core::hint::spin_loop();
            }
            // Ack interrupts, clear errors, install our structures.
            let is = port_regs.add(REG_PXIS_OFF / 4).read_volatile();
            port_regs.add(REG_PXIS_OFF / 4).write_volatile(is);
            port_regs.add(REG_PXSERR_OFF / 4).write_volatile(0xFFFF_FFFF);
            port_regs.add(REG_PXCLB_OFF / 4).write_volatile(cmd_list as u32);
            port_regs.add(REG_PXCLB_OFF / 4 + 1).write_volatile((cmd_list as u64 >> 32) as u32);
            port_regs.add(REG_PXFB_OFF / 4).write_volatile(fis_rx as u32);
            port_regs.add(REG_PXFB_OFF / 4 + 1).write_volatile((fis_rx as u64 >> 32) as u32);
            core::ptr::write_bytes(cmd_list, 0, 0x400);
            core::ptr::write_bytes(fis_rx, 0, 0x100);

            // Restart: FRE then ST, wait for FR/CR.
            port_regs
                .add(REG_PXCMD_OFF / 4)
                .write_volatile(PXCMD_FRE | PXCMD_ST);
            let mut waited = 0;
            loop {
                let cmd = port_regs.add(REG_PXCMD_OFF / 4).read_volatile();
                if cmd & (PXCMD_RUNNING | PXCMD_FR) != 0 || waited > 1_000_000 {
                    break;
                }
                waited += 1;
                core::hint::spin_loop();
            }
            // QEMU raises TFES/DHRS via an initial D2H FIS when the
            // engines come on; drain those stale bits before any command.
            let stale = port_regs.add(REG_PXIS_OFF / 4).read_volatile();
            port_regs.add(REG_PXIS_OFF / 4).write_volatile(stale);

            let cmd_now = port_regs.add(REG_PXCMD_OFF / 4).read_volatile();
            debug_log(format_args!(
                "ahci port {}: engine cmd=0x{:08x} clb={:#x} fb={:#x}",
                port,
                cmd_now,
                cmd_list as usize,
                fis_rx as usize
            ));
            if cmd_now & (PXCMD_RUNNING | PXCMD_FR) == 0 {
                return Err(AhciError::EngineTimeout);
            }
        }

        let mut disk = Self {
            port_regs,
            cmd_list,
            dma_memory: dma,
            cmd_table,
            port_index: port,
            sectors: 0,
            kind: StorageKind::Unknown,
        };
        unsafe { disk.identify() }.map_err(|_| AhciError::IdentifyFailed)?;
        debug_log(format_args!(
            "ahci port {}: identified, sectors={}",
            port, disk.sectors
        ));
        Ok(Some(disk))
    }

    pub fn port(&self) -> usize {
        self.port_index
    }

    pub fn sector_count(&self) -> u64 {
        self.sectors
    }

    /// Detected medium kind (HDD rpm / SSD / ...).
    pub fn storage_kind(&self) -> StorageKind {
        self.kind
    }

    /// Total capacity in bytes.
    pub fn capacity_bytes(&self) -> u64 {
        self.sectors * 512
    }

    /// Scratch data buffer inside `dma_memory`, right after the table.
    fn data_buffer(&mut self) -> *mut u8 {
        unsafe { self.cmd_table.add(0x200) }
    }

    unsafe fn identify(&mut self) -> Result<(), AhciError> {
        let buffer = self.data_buffer();
        let ok = unsafe { self.execute(CMD_IDENTIFY, 0, 1, buffer) };
        if !ok {
            return Err(AhciError::IdentifyFailed);
        }
        let words = unsafe { core::slice::from_raw_parts(buffer as *const u16, 256) };
        self.kind = StorageKind::from_identify(words);
        // Diagnostics: PRDBC (bytes transferred) + first identify words.
        {
            let header = self.cmd_list as *mut u32;
            let prdbc = unsafe { header.add(1).read_volatile() };
            let tfd = unsafe { self.port_regs.add(REG_PXTFD_OFF / 4).read_volatile() };
            debug_log(format_args!(
                "ahci identify: prdbc={} tfd=0x{:08x} w0=0x{:04x} w5=0x{:04x} w60=0x{:04x} w100=0x{:04x}",
                prdbc, tfd, words[0], words[5], words[60], words[100]
            ));
        }
        self.sectors = ((words[103] as u64) << 48)
            | ((words[102] as u64) << 32)
            | ((words[101] as u64) << 16)
            | words[100] as u64;
        if self.sectors == 0 {
            self.sectors = (words[61] as u64) << 16 | words[60] as u64;
        }
        Ok(())
    }

    /// Reads `count` sectors starting at `lba` into `buf` (512*count bytes).
    pub fn read_sectors(&mut self, lba: u64, count: u16, buf: &mut [u8]) -> bool {
        if buf.len() < count as usize * 512 || count == 0 || count > 8 {
            return false;
        }
        let scratch = self.data_buffer();
        if !unsafe { self.execute(CMD_READ_DMA_EXT, lba, count, scratch) } {
            return false;
        }
        unsafe { core::ptr::copy_nonoverlapping(scratch as *const u8, buf.as_mut_ptr(), count as usize * 512) };
        true
    }

    /// Writes `count` sectors starting at `lba` from `buf`.
    pub fn write_sectors(&mut self, lba: u64, count: u16, buf: &[u8]) -> bool {
        if buf.len() < count as usize * 512 || count == 0 || count > 8 {
            return false;
        }
        let scratch = self.data_buffer();
        unsafe { core::ptr::copy_nonoverlapping(buf.as_ptr(), scratch, count as usize * 512) };
        unsafe { self.execute(CMD_WRITE_DMA_EXT, lba, count, scratch) }
    }

    /// Builds one H2D FIS + PRDT in slot 0 of the firmware command list,
    /// issues it through PxCI, waits for completion, and moves data
    /// between the DMA scratch buffer and the caller's slice.
    unsafe fn execute(&mut self, command: u8, lba: u64, count: u16, buffer: *mut u8) -> bool {
        unsafe {
            // --- Command table: H2D Register FIS (5 dwords) + PRDT. ---
            // The FIS packs bytewise: each dword holds FOUR FIS bytes
            // (little-endian), not one field per dword.
            core::ptr::write_bytes(self.cmd_table, 0, 0x200);
            let table = self.cmd_table as *mut u32;
            let fis = table;
            // DW0 = [type=0x27, flags=C bit, command, features].
            fis.add(0).write_volatile(0x27 | (0x80 << 8) | ((command as u32) << 16));
            // DW1 = [lba 7:0, lba 15:8, lba 23:16, device (LBA bit 6)].
            fis.add(1).write_volatile(
                (lba & 0xFF) as u32
                    | (((lba >> 8) & 0xFF) as u32) << 8
                    | (((lba >> 16) & 0xFF) as u32) << 16
                    | (0x40u32 << 24),
            );
            // DW2 = [lba 31:24, lba 39:32, lba 47:40, features 15:8].
            fis.add(2).write_volatile(
                (((lba >> 24) & 0xFF) as u32)
                    | (((lba >> 32) & 0xFF) as u32) << 8
                    | (((lba >> 40) & 0xFF) as u32) << 16,
            );
            // DW3 = [sector count, count exp, reserved, control].
            fis.add(3).write_volatile(count as u32);

            // PRDT entry at 0x80.
            let prdt = self.cmd_table.add(0x80) as *mut u32;
            prdt.add(0).write_volatile(buffer as u32);
            prdt.add(1).write_volatile((buffer as u64 >> 32) as u32);
            prdt.add(3).write_volatile(count as u32 * 512 - 1);

            // --- Command header slot 0. ---
            let header = self.cmd_list as *mut u32;
            let write_bit = if command == CMD_WRITE_DMA_EXT { 1 << 6 } else { 0 };
            header.write_volatile(5 | (1 << 16) | write_bit);
            header.add(1).write_volatile(0);
            header.add(2).write_volatile(self.cmd_table as u32);
            header.add(3).write_volatile((self.cmd_table as u64 >> 32) as u32);
            {
                let t = self.cmd_table as *const u32;
                debug_log(format_args!(
                    "ahci: hdr dw0={:#010x} dw2={:#010x} ctba={:#x} | fis: {:#010x} {:#010x} {:#010x} {:#010x} {:#010x} | prdt {:#010x} {:#010x} {:#010x}",
                    header.read_volatile(),
                    header.add(2).read_volatile(),
                    self.cmd_table as usize,
                    t.read_volatile(),
                    t.add(1).read_volatile(),
                    t.add(2).read_volatile(),
                    t.add(3).read_volatile(),
                    t.add(4).read_volatile(),
                    (self.cmd_table.add(0x80) as *const u32).read_volatile(),
                    (self.cmd_table.add(0x84) as *const u32).read_volatile(),
                    (self.cmd_table.add(0x8C) as *const u32).read_volatile()
                ));
            }

            // --- Start the engine (ST) if the firmware left it stopped,
            //     keeping FIS receive (FRE) enabled. ---
            let cmd = self.port_regs.add(REG_PXCMD_OFF / 4).read_volatile();
            if cmd & (PXCMD_ST | PXCMD_FRE) != (PXCMD_ST | PXCMD_FRE) {
                self.port_regs
                    .add(REG_PXCMD_OFF / 4)
                    .write_volatile(cmd | PXCMD_ST | PXCMD_FRE);
            }

            // --- Issue slot 0; completion = PxIS bits (D2H FIS received /
            // PIO setup), error = TFES. PxCI alone proved unreliable. ---
            const IS_DHRS: u32 = 1 << 0; // device to host FIS received
            const IS_PSS: u32 = 1 << 1; // PIO setup FIS received
            const IS_TFES: u32 = 1 << 30; // task file error
            // Ack stale status first.
            let is = self.port_regs.add(REG_PXIS_OFF / 4).read_volatile();
            self.port_regs.add(REG_PXIS_OFF / 4).write_volatile(is);
            self.port_regs.add(REG_PXCI_OFF / 4).write_volatile(1);
            let mut ticks = 0;
            loop {
                let is = self.port_regs.add(REG_PXIS_OFF / 4).read_volatile();
                if is & (IS_DHRS | IS_PSS) != 0 {
                    // Command finished; ack the status bits.
                    self.port_regs.add(REG_PXIS_OFF / 4).write_volatile(is);
                    break;
                }
                if is & IS_TFES != 0 {
                    // TFES also fires from the engine-start D2H; only a
                    // real failure if no completion arrives alongside.
                    // Ack and keep waiting for DHRS/PSS.
                    self.port_regs.add(REG_PXIS_OFF / 4).write_volatile(is);
                }
                ticks += 1;
                if ticks > 40_000_000 {
                    let ci = self.port_regs.add(REG_PXCI_OFF / 4).read_volatile();
                    debug_log(format_args!(
                        "ahci: command 0x{:02x} timed out is=0x{:08x} ci=0x{:08x} tfd=0x{:08x}",
                        command,
                        is,
                        ci,
                        self.port_regs.add(REG_PXTFD_OFF / 4).read_volatile()
                    ));
                    return false;
                }
                core::hint::spin_loop();
            }
            let tfd = self.port_regs.add(REG_PXTFD_OFF / 4).read_volatile();
            if tfd & 0x01 != 0 {
                debug_log(format_args!(
                    "ahci: command 0x{:02x} finished with error tfd=0x{:08x}",
                    command, tfd
                ));
                return false;
            }
            true
        }
    }
}
