//! Persistent storage: AHCI sector device, OrbitaFS disk init, and RAM<->disk VFS sync.


extern crate alloc;

use alloc::vec::Vec;
use alloc::vec;
use alloc::format;
use orbita_fs::{
    BlockAddress, BlockDevice, BlockDeviceError, BlockDeviceGeometry, BlockDeviceInfo,
    BlockDeviceStats, BlockResponse, BlockSize, MemoryVolume,
};
use orbita_fs::diskfs::OrbitaDiskFs;
use orbita_hw::{
    AhciDisk,
};
use orbita_std::String;
use crate::config::*;

/// Bridges the AHCI driver into the OrbitaFS sector contract.
pub(crate) struct AhciSectorDisk {
    pub(crate) inner: AhciDisk,
}

/// Creates the standard system tree on a fresh volume.
pub(crate) fn seed_system_layout<D: orbita_fs::diskfs::SectorDevice>(
    fs: &mut OrbitaDiskFs,
    device: &mut D,
) {
    // /boot — startup binaries and loader config.
    let _ = fs.write_file(device, "/boot/loader.cfg", b"timeout=0\ndefault=orbita\n");
    let _ = fs.write_file(
        device,
        "/boot/orbita-loader",
        b"ORBEXEC\0\x01\0\0boot loader stage (placeholder payload)\n",
    );
    // /bin — system binaries.
    let _ = fs.write_file(
        device,
        "/bin/orbita-init",
        b"ORBEXEC\0\x01\x01system init (placeholder payload)\n",
    );
    let _ = fs.write_file(
        device,
        "/bin/orbita-shell",
        b"ORBEXEC\0\x01\x02interactive shell (placeholder payload)\n",
    );
    // /lib — shared libraries.
    let _ = fs.write_file(device, "/lib/liborbita-fs.orbl", b"ORBLIB\0\x01filesystem services\n");
    let _ = fs.write_file(device, "/lib/liborbita-net.orbl", b"ORBLIB\0\x02network services\n");
    let _ = fs.write_file(device, "/lib/liborbita-ui.orbl", b"ORBLIB\0\x03ui services\n");
    // /etc — the live system configuration.
    if fs.read_file(device, ORBITA_CONF).is_none() {
        let _ = fs.write_file(device, ORBITA_CONF, orbita_conf_default().as_bytes());
    }
}

/// Trees that are user-writable and synced RAM -> disk.
pub(crate) const SYNCED_TREES: &[&str] = &["/etc", "/home", "/var", "/root"];

/// Recursively collects files under `path` on the persistent volume.
pub(crate) fn collect_disk_files(
    fs: &OrbitaDiskFs,
    disk: &mut AhciSectorDisk,
    path: &str,
    out: &mut Vec<(String, Vec<u8>)>,
) {
    let Some(entries) = fs.list_dir(path) else { return };
    let base = if path == "/" { String::new() } else { String::from(path) };
    for name in entries {
        let child = format!("{}/{}", base, name);
        if fs.is_dir(&child) {
            collect_disk_files(fs, disk, &child, out);
        } else if let Some(bytes) = fs.read_file(disk, &child) {
            out.push((child.clone(), bytes));
        }
    }
}

/// Loads the whole persistent volume into the RAM volume so `ls`, `cat`
/// etc. operate on the real on-disk state.
pub(crate) fn load_persistent_into_ram(
    fs: &OrbitaDiskFs,
    disk: &mut AhciSectorDisk,
    ram: &mut MemoryVolume,
) {
    let mut files = Vec::new();
    collect_disk_files(fs, disk, "/", &mut files);
    for (path, bytes) in files {
        let _ = ram.create_file_path(path.as_str(), &bytes);
    }
}

/// Recursively collects files under a RAM-volume directory.
pub(crate) fn collect_ram_files(
    ram: &mut MemoryVolume,
    path: &str,
    out: &mut Vec<(String, Vec<u8>)>,
) {
    let Ok(listing) = ram.list_path(path) else { return };
    let base = if path == "/" { String::new() } else { String::from(path) };
    for entry in listing.entries {
        let child = format!("{}/{}", base, entry.name);
        if let Ok(bytes) = ram.read_file_path(child.as_str()) {
            out.push((child.clone(), bytes));
        }
        collect_ram_files(ram, &child, out);
    }
}

/// Persists changed RAM files back to OrbitaFS. Returns the number of
/// files written.
pub(crate) fn sync_ram_to_disk(
    fs: &mut OrbitaDiskFs,
    disk: &mut AhciSectorDisk,
    ram: &mut MemoryVolume,
) -> usize {
    let mut written = 0usize;
    for tree in SYNCED_TREES {
        let mut files = Vec::new();
        collect_ram_files(ram, tree, &mut files);
        for (path, bytes) in files {
            let unchanged = fs
                .read_file(disk, path.as_str())
                .map(|old| old == bytes)
                .unwrap_or(false);
            if !unchanged && fs.write_file(disk, path.as_str(), &bytes) {
                written += 1;
            }
        }
    }
    written
}

/// Mounts (or formats on first boot) OrbitaFS on the persistent disk that
/// the `ahci-storage` driver bound during boot.
pub(crate) fn init_persistent_disk(
    mut device: AhciSectorDisk,
) -> Option<(OrbitaDiskFs, AhciSectorDisk, u32)> {
    let sectors = device.inner.sector_count();

    let mut fs = match OrbitaDiskFs::mount(&mut device) {
        Ok(fs) => fs,
        Err(_) => {
            // First boot: format using the whole image capacity.
            let blocks = sectors.min(u32::MAX as u64) as u32;
            if OrbitaDiskFs::format(&mut device, blocks).is_err() {
                return None;
            }
            OrbitaDiskFs::mount(&mut device).ok()?
        }
    };

    // Boot counter: read, increment, write back.
    let boots = fs
        .read_file(&mut device, "/boot/counter")
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .and_then(|text| text.trim().parse::<u32>().ok())
        .unwrap_or(0)
        + 1;
    let counter_text = format!("{boots}");
    if !fs.write_file(&mut device, "/boot/counter", counter_text.as_bytes()) {
        return None;
    }

    Some((fs, device, boots))
}

pub(crate) struct BootstrapRamDisk {
    block_size: BlockSize,
    data: Vec<u8>,
    stats: BlockDeviceStats,
}

impl BootstrapRamDisk {
    pub(crate) fn new(block_size: BlockSize, block_count: u64) -> Self {
        Self {
            block_size,
            data: vec![0; block_size.0 as usize * block_count as usize],
            stats: BlockDeviceStats::default(),
        }
    }
}

impl BlockDeviceInfo for BootstrapRamDisk {
    fn geometry(&self) -> BlockDeviceGeometry {
        BlockDeviceGeometry {
            block_size: self.block_size,
            block_count: (self.data.len() / self.block_size.0 as usize) as u64,
        }
    }

    fn stats(&self) -> BlockDeviceStats {
        self.stats
    }
}

impl BlockDevice for BootstrapRamDisk {
    fn read_blocks(
        &mut self,
        start: BlockAddress,
        blocks: u64,
        dst: &mut [u8],
    ) -> Result<BlockResponse, BlockDeviceError> {
        let block_size = self.block_size.0 as usize;
        let offset = start.0 as usize * block_size;
        let len = blocks as usize * block_size;
        if offset + len > self.data.len() || dst.len() < len {
            return Err(BlockDeviceError::OutOfBounds);
        }
        dst[..len].copy_from_slice(&self.data[offset..offset + len]);
        self.stats.read_ops += 1;
        Ok(BlockResponse {
            completed_blocks: blocks,
        })
    }

    fn write_blocks(
        &mut self,
        start: BlockAddress,
        blocks: u64,
        src: &[u8],
    ) -> Result<BlockResponse, BlockDeviceError> {
        let block_size = self.block_size.0 as usize;
        let offset = start.0 as usize * block_size;
        let len = blocks as usize * block_size;
        if offset + len > self.data.len() || src.len() < len {
            return Err(BlockDeviceError::OutOfBounds);
        }
        self.data[offset..offset + len].copy_from_slice(&src[..len]);
        self.stats.write_ops += 1;
        Ok(BlockResponse {
            completed_blocks: blocks,
        })
    }

    fn flush(&mut self) -> Result<(), BlockDeviceError> {
        self.stats.flush_ops += 1;
        Ok(())
    }
}
