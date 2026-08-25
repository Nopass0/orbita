//! Read-only FAT12/16/32 driver over any BlockDevice.
//!
//! Purpose: mount the firmware ESP (the QEMU `fat:rw` drive staged by the
//! host build) as the `/pkg` delivery channel — `.orbpkg` bundles land in
//! `target/orbita-esp/pkg/` on the host and appear inside the OS without
//! rebuilding the kernel.
//!
//! Supports: FAT12/FAT16/FAT32, 8.3 names and LFN (long file names),
//! subdirectories, files up to 4 GiB−1. Read-only by design.

use alloc::format;
use alloc::string::ToString;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use crate::diskfs::SectorDevice;

const SECTOR: usize = 512;
const ATTR_LFN: u8 = 0x0F;
const ATTR_DIRECTORY: u8 = 0x10;
const ATTR_VOLUME_ID: u8 = 0x08;
const DIR_END: u8 = 0x00;

/// Why a mount/read failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FatError {
    /// The volume is not a FAT filesystem.
    NotFat,
    /// Geometry the driver does not support (yet).
    UnsupportedGeometry,
    /// Block device I/O error.
    Io,
    /// Path not found.
    NotFound,
    /// Path component is not a directory.
    NotADirectory,
    /// Cluster chain corruption (loop or invalid index).
    BadClusterChain,
}

/// One directory entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FatDirEntry {
    pub name: String,
    pub is_dir: bool,
    pub size: u32,
    /// First data cluster.
    pub first_cluster: u32,
}

/// FAT flavor detected from the BPB.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FatKind {
    Fat12,
    Fat16,
    Fat32,
}

/// A mounted read-only FAT volume.
pub struct FatVolume<'d> {
    device: &'d mut dyn SectorDevice,
    kind: FatKind,
    fat_start_lba: u64,
    root_dir_lba: u64,
    /// FAT12/16: entry count; FAT32: unused (root is a cluster chain).
    root_entries: usize,
    root_cluster: u32,
    data_start_lba: u64,
    sectors_per_cluster: u64,
    total_clusters: u32,
    // One-FAT-sector cache for cluster traversal.
    cache_lba: u64,
    cache: Vec<u8>,
}

/// Read one 512-byte sector through the [`SectorDevice`] contract.
fn read_sector_of(device: &mut dyn SectorDevice, lba: u64, out: &mut [u8]) -> Result<(), FatError> {
    let mut sector = [0u8; SECTOR];
    if !device.read_sector(lba as u32, &mut sector) {
        return Err(FatError::Io);
    }
    out[..SECTOR].copy_from_slice(&sector);
    Ok(())
}

impl<'d> FatVolume<'d> {
    /// Mount the FAT volume on `device` (reads the boot sector + BPB).
    pub fn mount(device: &'d mut dyn SectorDevice) -> Result<Self, FatError> {
        let mut boot = vec![0u8; SECTOR];
        read_sector_of(device, 0, &mut boot)?;

        if boot[510] != 0x55 || boot[511] != 0xAA {
            return Err(FatError::NotFat);
        }
        let bytes_per_sector = u16::from_le_bytes([boot[11], boot[12]]) as usize;
        if bytes_per_sector != SECTOR {
            return Err(FatError::UnsupportedGeometry);
        }
        let sectors_per_cluster = boot[13] as u64;
        if sectors_per_cluster == 0 || (sectors_per_cluster & (sectors_per_cluster - 1)) != 0 {
            return Err(FatError::UnsupportedGeometry);
        }
        let reserved_sectors = u16::from_le_bytes([boot[14], boot[15]]) as u64;
        let fat_copies = boot[16] as u64;
        let root_entries = u16::from_le_bytes([boot[17], boot[18]]) as usize;
        let total_sectors_16 = u16::from_le_bytes([boot[19], boot[20]]) as u64;
        let fat_sectors_16 = u16::from_le_bytes([boot[22], boot[23]]) as u64;
        let total_sectors_32 = u32::from_le_bytes(boot[32..36].try_into().unwrap()) as u64;
        let fat_sectors_32 = u32::from_le_bytes(boot[36..40].try_into().unwrap()) as u64;
        let total_sectors = if total_sectors_16 != 0 { total_sectors_16 } else { total_sectors_32 };
        let fat_sectors = if fat_sectors_16 != 0 { fat_sectors_16 } else { fat_sectors_32 };
        if fat_sectors == 0 || total_sectors == 0 {
            return Err(FatError::NotFat);
        }

        let root_dir_sectors = (root_entries * 32 + SECTOR - 1) / SECTOR as u64 as usize;
        let root_dir_sectors = root_dir_sectors as u64;
        let fat_start_lba = reserved_sectors;
        let root_dir_lba = fat_start_lba + fat_copies * fat_sectors;
        let data_start_lba = root_dir_lba + root_dir_sectors;
        let data_sectors = total_sectors - data_start_lba;
        let total_clusters = (data_sectors / sectors_per_cluster) as u32;

        let (kind, root_cluster) = if total_clusters < 4085 {
            (FatKind::Fat12, 0)
        } else if total_clusters < 65525 {
            (FatKind::Fat16, 0)
        } else {
            let cluster = u32::from_le_bytes(boot[44..48].try_into().unwrap());
            (FatKind::Fat32, cluster.max(2))
        };

        Ok(Self {
            device,
            kind,
            fat_start_lba,
            root_dir_lba,
            root_entries,
            root_cluster,
            data_start_lba,
            sectors_per_cluster,
            total_clusters,
            cache_lba: u64::MAX,
            cache: vec![0u8; SECTOR],
        })
    }

    /// Detected FAT flavor (diagnostics).
    pub const fn kind(&self) -> FatKind {
        self.kind
    }

    fn read_sector(&mut self, lba: u64, out: &mut [u8]) -> Result<(), FatError> {
        read_sector_of(self.device, lba, out)
    }

    fn cluster_lba(&self, cluster: u32) -> u64 {
        self.data_start_lba + (cluster as u64 - 2) * self.sectors_per_cluster
    }

    fn next_cluster(&mut self, cluster: u32) -> Result<Option<u32>, FatError> {
        let (fat_offset, entry_bytes): (u64, usize) = match self.kind {
            FatKind::Fat12 => ((cluster as u64 * 3) / 2, 2),
            FatKind::Fat16 => (cluster as u64 * 2, 2),
            FatKind::Fat32 => (cluster as u64 * 4, 4),
        };
        let sector = self.fat_start_lba + fat_offset / SECTOR as u64;
        let offset = (fat_offset % SECTOR as u64) as usize;
        // FAT12 entries straddling a sector boundary read both bytes from
        // two adjacent sectors; read the second lazily.
        if self.cache_lba != sector {
            let mut fresh = vec![0u8; SECTOR];
            read_sector_of(self.device, sector, &mut fresh)?;
            self.cache = fresh;
            self.cache_lba = sector;
        }
        let value: u64 = if offset + entry_bytes <= SECTOR {
            match entry_bytes {
                2 => u16::from_le_bytes([self.cache[offset], self.cache[offset + 1]]) as u64,
                _ => u32::from_le_bytes(self.cache[offset..offset + 4].try_into().unwrap()) as u64,
            }
        } else {
            let mut next = [0u8; SECTOR];
            self.read_sector(sector + 1, &mut next)?;
            match entry_bytes {
                2 => u16::from_le_bytes([self.cache[offset], next[0]]) as u64,
                _ => {
                    let mut bytes = [self.cache[offset], 0, 0, 0];
                    bytes[1..].copy_from_slice(&next[..3]);
                    u32::from_le_bytes(bytes) as u64
                }
            }
        };
        let next = match self.kind {
            FatKind::Fat12 => {
                let raw = if cluster & 1 == 1 { value >> 4 } else { value & 0x0FFF };
                match raw {
                    0x000 | 0x001 => return Err(FatError::BadClusterChain),
                    0xFF8..=0xFFF => None,
                    n => Some(n as u32),
                }
            }
            FatKind::Fat16 => match value as u32 {
                0x0000 | 0x0001 => return Err(FatError::BadClusterChain),
                0xFFF8..=0xFFFF => None,
                n => Some(n),
            },
            FatKind::Fat32 => match (value as u32) & 0x0FFF_FFFF {
                0x0000 | 0x0001 => return Err(FatError::BadClusterChain),
                0x0FFF_FF8..=0x0FFF_FFF => None,
                n => Some(n),
            },
        };
        Ok(next)
    }

    /// Iterate the clusters of a chain, calling `f(lba, sector_count)`.
    fn for_each_cluster(
        &mut self,
        first: u32,
        mut f: impl FnMut(&mut Self, u64, u64) -> Result<(), FatError>,
    ) -> Result<(), FatError> {
        let mut cluster = first;
        let mut hops = 0;
        loop {
            f(self, self.cluster_lba(cluster), self.sectors_per_cluster)?;
            hops += 1;
            if hops > self.total_clusters {
                return Err(FatError::BadClusterChain);
            }
            match self.next_cluster(cluster)? {
                Some(next) => cluster = next,
                None => return Ok(()),
            }
        }
    }

    /// Read one cluster into `out` (must be cluster-sized).
    fn read_cluster(&mut self, cluster: u32, out: &mut [u8]) -> Result<(), FatError> {
        let lba = self.cluster_lba(cluster);
        for index in 0..self.sectors_per_cluster {
            read_sector_of(self.device, lba + index, &mut out[(index as usize) * SECTOR..][..SECTOR])?;
        }
        Ok(())
    }

    /// Parse a raw 32-byte directory entry slot.
    fn parse_entry(raw: &[u8]) -> Option<(bool, String, u32, u32)> {
        let first = raw[0];
        if first == DIR_END {
            return None;
        }
        if first == 0xE5 {
            return Some((true, String::new(), 0, 0)); // deleted: skip, continue
        }
        let attr = raw[11];
        if attr & ATTR_LFN == ATTR_LFN {
            return Some((true, String::new(), 0, 0)); // LFN slots consumed by caller
        }
        if attr & ATTR_VOLUME_ID == ATTR_VOLUME_ID {
            return Some((true, String::new(), 0, 0));
        }
        let is_dir = attr & ATTR_DIRECTORY != 0;
        let cluster_hi = u16::from_le_bytes([raw[20], raw[21]]) as u32;
        let cluster_lo = u16::from_le_bytes([raw[26], raw[27]]) as u32;
        let cluster = if is_dir && cluster_lo == 0 {
            0
        } else {
            (cluster_hi << 16) | cluster_lo
        };
        let size = u32::from_le_bytes(raw[28..32].try_into().unwrap());
        // 8.3 name
        
        let stem = String::from_utf8_lossy(&raw[0..8]).trim_end().to_string();
        let ext = String::from_utf8_lossy(&raw[8..11]).trim_end().to_string();
        let name = if ext.is_empty() { stem } else { format!("{stem}.{ext}") };
        Some((false, name, size, cluster))
    }

    /// Decode a long-file-name from LFN slots (UTF-16 pieces).
    fn decode_lfn(slots: &[[u8; 32]]) -> String {
        // Slots arrive in reverse order; each carries 13 UTF-16 units.
        let mut units: Vec<u16> = Vec::new();
        for slot in slots.iter().rev() {
            let mut push = |bytes: &[u8]| {
                if bytes.len() == 2 {
                    units.push(u16::from_le_bytes([bytes[0], bytes[1]]));
                }
            };
            push(&slot[1..3]);
            push(&slot[3..5]);
            push(&slot[5..7]);
            push(&slot[7..9]);
            push(&slot[9..11]);
            push(&slot[14..16]);
            push(&slot[16..18]);
            push(&slot[18..20]);
            push(&slot[20..22]);
            push(&slot[22..24]);
            push(&slot[24..26]);
            push(&slot[28..30]);
            push(&slot[30..32]);
        }
        let mut text = String::new();
        for unit in units {
            if unit == 0x0000 || unit == 0xFFFF {
                break;
            }
            // BMP-only decoding (fine for file names).
            text.push(char::from_u32(unit as u32).unwrap_or('?'));
        }
        text
    }

    fn read_dir_sectors(&mut self, lba: u64, sectors: u64, entries: &mut Vec<FatDirEntry>) -> Result<(), FatError> {
        for index in 0..sectors {
            let mut sector = vec![0u8; SECTOR];
            self.read_sector(lba + index, &mut sector)?;
            let mut lfn_slots: Vec<[u8; 32]> = Vec::new();
            let mut offset = 0;
            while offset + 32 <= SECTOR {
                let raw: [u8; 32] = sector[offset..offset + 32].try_into().unwrap();
                let Some((skip, name, size, cluster)) = Self::parse_entry(&raw) else {
                    return Ok(()); // end-of-directory marker
                };
                if raw[11] & ATTR_LFN == ATTR_LFN {
                    lfn_slots.push(raw);
                } else if !skip {
                    let name = if lfn_slots.is_empty() {
                        name
                    } else {
                        let decoded = Self::decode_lfn(&lfn_slots);
                        lfn_slots.clear();
                        if decoded.is_empty() { name } else { decoded }
                    };
                    entries.push(FatDirEntry {
                        name,
                        is_dir: raw[11] & ATTR_DIRECTORY != 0,
                        size,
                        first_cluster: cluster,
                    });
                } else {
                    lfn_slots.clear();
                }
                offset += 32;
            }
        }
        Ok(())
    }

    /// List a directory by absolute path (`/`-separated; `/` is the root).
    pub fn list_dir(&mut self, path: &str) -> Result<Vec<FatDirEntry>, FatError> {
        let mut entries = Vec::new();
        if self.kind == FatKind::Fat32 {
            let root = self.root_cluster;
            self.for_each_cluster(root, |volume, lba, sectors| {
                volume.read_dir_sectors(lba, sectors, &mut entries)
            })?;
        } else {
            let sectors = (self.root_entries * 32 + SECTOR - 1) / SECTOR;
            self.read_dir_sectors(self.root_dir_lba, sectors as u64, &mut entries)?;
        }
        for component in path.split('/').filter(|c| !c.is_empty()) {
            let target = component.to_ascii_lowercase();
            let Some(found) = entries
                .iter()
                .find(|entry| entry.name.to_ascii_lowercase() == target && entry.is_dir)
            else {
                return Err(FatError::NotFound);
            };
            let cluster = found.first_cluster;
            entries.clear();
            if cluster == 0 {
                return Ok(entries);
            }
            self.for_each_cluster(cluster, |volume, lba, sectors| {
                volume.read_dir_sectors(lba, sectors, &mut entries)
            })?;
        }
        Ok(entries)
    }

    /// Read a whole file by absolute path.
    pub fn read_file(&mut self, path: &str) -> Result<Vec<u8>, FatError> {
        let (parent, file_name) = match path.rsplit_once('/') {
            Some((parent, name)) if !name.is_empty() => (parent, name),
            _ => ("", path.trim_start_matches('/')),
        };
        let entries = self.list_dir(parent)?;
        let target = file_name.to_ascii_lowercase();
        let found = entries
            .iter()
            .find(|entry| !entry.is_dir && entry.name.to_ascii_lowercase() == target)
            .ok_or(FatError::NotFound)?
            .clone();

        let cluster_bytes = (self.sectors_per_cluster as usize) * SECTOR;
        let mut out: Vec<u8> = Vec::with_capacity(found.size as usize);
        if found.first_cluster == 0 {
            return Ok(out);
        }
        let mut cluster = found.first_cluster;
        let mut hops = 0;
        loop {
            let mut chunk = vec![0u8; cluster_bytes];
            self.read_cluster(cluster, &mut chunk)?;
            let remaining = (found.size as usize).saturating_sub(out.len());
            let take = remaining.min(cluster_bytes);
            out.extend_from_slice(&chunk[..take]);
            hops += 1;
            if hops > self.total_clusters as usize || out.len() >= found.size as usize {
                break;
            }
            match self.next_cluster(cluster)? {
                Some(next) => cluster = next,
                None => break,
            }
        }
        Ok(out)
    }

}
