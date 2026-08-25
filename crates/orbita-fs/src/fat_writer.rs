//! FAT16 image *writer* — builds the `/pkg` delivery image on the host.
//!
//! `orbita-build` packs `.orbpkg` bundles into a plain FAT16 image
//! (`target/orbita-pkg.img`); QEMU exposes it as a raw disk, and the
//! kernel's read-only FAT driver (see [`crate::fat`]) mounts it as the
//! `/pkg` channel. Writer and reader live in the same crate and are
//! round-trip tested together — no vvfat quirks, deterministic layout,
//! and the same mechanism works on real hardware (a second partition).

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

const SECTOR: usize = 512;
const ROOT_ENTRIES: usize = 224;
const RESERVED_SECTORS: u64 = 1;
const FAT_COPIES: u64 = 2;
const SECTORS_PER_CLUSTER: u32 = 1;

const ATTR_READ_ONLY: u8 = 0x01;
#[allow(dead_code)]
const ATTR_HIDDEN: u8 = 0x02;
const ATTR_DIRECTORY: u8 = 0x10;
const ATTR_LFN: u8 = 0x0F;

const DIR_END: u8 = 0x00;

/// One file placed into the image (under the `pkg` directory).
pub struct PkgFile<'a> {
    pub name: &'a str,
    pub data: &'a [u8],
}

/// Build a FAT16 image containing `files` under `/pkg`.
///
/// Geometry: 512-byte sectors, 1 sector/cluster, one reserved sector,
/// two FAT copies, 224 root entries — the classic floppy-style layout
/// scaled up, mountable by any FAT implementation.
pub fn build_pkg_image(files: &[PkgFile<'_>]) -> Vec<u8> {
    // --- size the image ----------------------------------------------------
    let data_clusters = files.iter().map(|f| clusters_for(f.data.len())).sum::<u32>()
        + 1; // the pkg directory itself
    // FAT16 wants at least ~4085 clusters? No: FAT16 covers 4085..65524;
    // smaller images are FAT12 — but our reader (and every reader) handles
    // FAT16 regardless of cluster count, so keep a FAT16 table and size the
    // image generously: at least 2 MiB.
    let min_data_sectors = (data_clusters * SECTORS_PER_CLUSTER).max(4096);
    let root_sectors = (ROOT_ENTRIES * 32 + SECTOR - 1) / SECTOR;
    // Each FAT16 entry covers one cluster; iterate to a stable size.
    let mut total_sectors = 1 + root_sectors as u64 + FAT_COPIES * 8 + min_data_sectors as u64;
    loop {
        let clusters = total_sectors - (1 + root_sectors as u64 + FAT_COPIES * fat_sectors_for(total_sectors));
        let fat = fat_sectors_for(total_sectors);
        let data_sectors = total_sectors - (RESERVED_SECTORS + FAT_COPIES * fat + root_sectors as u64);
        let _ = clusters;
        if fat == fat_sectors_for(total_sectors) && data_sectors >= min_data_sectors as u64 {
            break;
        }
        total_sectors += 64;
    }
    let fat_sectors = fat_sectors_for(total_sectors);
    let fat_start = RESERVED_SECTORS;
    let root_start = fat_start + FAT_COPIES * fat_sectors;
    let data_start = root_start + root_sectors as u64;
    let cluster_count = ((total_sectors - data_start) / SECTORS_PER_CLUSTER as u64) as u32;

    let mut image = vec![0u8; total_sectors as usize * SECTOR];

    // --- boot sector --------------------------------------------------------
    let boot = &mut image[..SECTOR];
    boot[0] = 0xEB;
    boot[1] = 0x3C;
    boot[2] = 0x90;
    boot[3..11].copy_from_slice(b"ORBITA  ");
    boot[11..13].copy_from_slice(&(SECTOR as u16).to_le_bytes());
    boot[13] = SECTORS_PER_CLUSTER as u8;
    boot[14..16].copy_from_slice(&(RESERVED_SECTORS as u16).to_le_bytes());
    boot[16] = FAT_COPIES as u8;
    boot[17..19].copy_from_slice(&(ROOT_ENTRIES as u16).to_le_bytes());
    boot[19..21].copy_from_slice(&0u16.to_le_bytes()); // total sectors 16 (use 32 below)
    boot[21] = 0xF8; // fixed disk
    boot[22..24].copy_from_slice(&(fat_sectors as u16).to_le_bytes());
    boot[24..26].copy_from_slice(&63u16.to_le_bytes()); // sectors per track
    boot[26..28].copy_from_slice(&255u16.to_le_bytes()); // heads
    boot[32..36].copy_from_slice(&(total_sectors as u32).to_le_bytes());
    boot[36..38].copy_from_slice(&0u16.to_le_bytes()); // FAT16: no FS-info sector
    boot[38..40].copy_from_slice(&6u16.to_le_bytes()); // FAT16 boot sector copies
    boot[510..512].copy_from_slice(&[0x55, 0xAA]);

    // --- FAT tables ---------------------------------------------------------
    // Cluster 0/1 reserved; the pkg directory occupies cluster 2,
    // each file's chain follows.
    let fat_entry = |cluster: u32| -> u16 {
        if cluster >= 2 && cluster < cluster_count + 2 {
            0x0000
        } else {
            0xFFFF
        }
    };
    let _ = fat_entry;
    let mut chains: Vec<(u32, Vec<u32>)> = Vec::new(); // (first_cluster, clusters)
    let mut next_cluster: u32 = 3;
    let chain_for = |clusters: u32, next: &mut u32| -> Vec<u32> {
        let chain: Vec<u32> = (*next..*next + clusters).collect();
        *next += clusters;
        chain
    };
    let pkg_dir_chain = vec![2u32];
    let mut file_chains: Vec<(String, Vec<u32>, &[u8])> = Vec::new();
    for file in files {
        let chain = chain_for(clusters_for(file.data.len()), &mut next_cluster);
        file_chains.push((String::from(file.name), chain, file.data));
    }
    chains.push((2, pkg_dir_chain.clone()));

    let write_fat = |image: &mut [u8], lba: u64, entries: &[(u32, u16)]| {
        let base = lba as usize * SECTOR;
        for (cluster, value) in entries {
            let offset = base + *cluster as usize * 2;
            image[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
        }
    };
    let mut entries: Vec<(u32, u16)> = vec![(0, 0xFFF8), (1, 0xFFFF), (2, 0xFFFF)];
    for (_, chain, _) in &file_chains {
        for (index, cluster) in chain.iter().enumerate() {
            let next = chain.get(index + 1).copied().unwrap_or(0xFFFF) as u16;
            entries.push((*cluster, next));
        }
    }
    for copy in 0..FAT_COPIES {
        write_fat(&mut image, fat_start + copy * fat_sectors, &entries);
    }

    // --- root directory: the `pkg` directory --------------------------------
    let root = &mut image[root_start as usize * SECTOR..(root_start + root_sectors as u64) as usize * SECTOR];
    write_dir_entry(root, 0, "pkg", ATTR_DIRECTORY, 2, 0);

    // --- pkg directory entries ----------------------------------------------
    // The pkg directory occupies cluster 2; entry writing happens in its
    // own sector slice, data writes afterwards.
    let pkg_dir_lba = data_start as usize; // cluster 2 -> first data sector
    {
        let pkg_dir = &mut image[pkg_dir_lba * SECTOR..(pkg_dir_lba + 1) * SECTOR];
        let mut dir_offset = 0usize;
        for (name, chain, data) in &file_chains {
            let size = data.len() as u32;
            let first = chain.first().copied().unwrap_or(0);
            let used = write_dir_entry(pkg_dir, dir_offset, name, ATTR_READ_ONLY, first, size);
            dir_offset += used;
        }
        if dir_offset * 32 < SECTOR {
            pkg_dir[dir_offset * 32] = DIR_END;
        }
    }
    // File data into cluster chains.
    for (_name, chain, data) in &file_chains {
        let mut offset = 0usize;
        for cluster in chain {
            let base = data_start as usize * SECTOR + (*cluster as usize - 2) * SECTOR;
            let take = (data.len() - offset).min(SECTOR);
            image[base..base + take].copy_from_slice(&data[offset..offset + take]);
            offset += take;
        }
    }

    image
}

fn clusters_for(bytes: usize) -> u32 {
    (bytes.div_ceil(SECTOR) as u32).max(1)
}

fn fat_sectors_for(total_sectors: u64) -> u64 {
    // FAT16 entries = data clusters; sectors = entries*2/512, doubled for slack.
    let approx_clusters = total_sectors.saturating_sub(64);
    (approx_clusters * 2).div_ceil(SECTOR as u64).max(8)
}

/// LFN checksum over the 8.3 name (per FAT spec).
fn lfn_checksum(short: &[u8; 11]) -> u8 {
    let mut sum: u8 = 0;
    for byte in short {
        sum = sum.rotate_right(1).wrapping_add(*byte);
    }
    sum
}

/// Write one directory entry (LFN slots + 8.3 entry) at `slot_offset`
/// entries into `dir`; returns how many 32-byte slots were used.
fn write_dir_entry(dir: &mut [u8], slot_offset: usize, name: &str, attr: u8, first_cluster: u32, size: u32) -> usize {
    let short = short_name(name);
    let needs_lfn = short_is_mangled(name, &short);
    let mut slots_used = 1usize;
    let checksum = lfn_checksum(&short);
    if needs_lfn {
        let units: Vec<u16> = name.encode_utf16().collect();
        let lfn_slots = units.len().div_ceil(13);
        slots_used += lfn_slots;
        for slot in 0..lfn_slots {
            let ordinal = (slot + 1) as u8 | if slot + 1 == lfn_slots { 0x40 } else { 0 };
            let base = (slot_offset + lfn_slots - 1 - slot) * 32;
            dir[base] = ordinal;
            dir[base + 11] = ATTR_LFN;
            dir[base + 12] = 0;
            dir[base + 13] = checksum;
            dir[base + 26] = 0;
            let mut unit_index = slot * 13;
            for range in [
                1..3, 3..5, 5..7, 7..9, 9..11, 14..16, 16..18, 18..20, 20..22, 22..24, 24..26,
                28..30, 30..32,
            ] {
                for position in range.step_by(2) {
                    let unit = if unit_index < units.len() {
                        units[unit_index]
                    } else if unit_index == units.len() {
                        0x0000
                    } else {
                        0xFFFF
                    };
                    dir[base + position..base + position + 2].copy_from_slice(&unit.to_le_bytes());
                    unit_index += 1;
                }
            }
        }
    }
    let base = (slot_offset + slots_used - 1) * 32;
    dir[base..base + 11].copy_from_slice(&short);
    dir[base + 11] = attr;
    dir[base + 20..base + 22].copy_from_slice(&((first_cluster >> 16) as u16).to_le_bytes());
    dir[base + 26..base + 28].copy_from_slice(&(first_cluster as u16).to_le_bytes());
    dir[base + 28..base + 32].copy_from_slice(&size.to_le_bytes());
    slots_used
}

/// 8.3 uppercased name with padding.
fn short_name(name: &str) -> [u8; 11] {
    let mut out = [b' '; 11];
    let (stem, ext) = match name.split_once('.') {
        Some((stem, ext)) => (stem, ext),
        None => (name, ""),
    };
    for (slot, byte) in out.iter_mut().zip(stem.to_ascii_uppercase().as_bytes()) {
        *slot = *byte;
    }
    for (slot, byte) in out[8..].iter_mut().zip(ext.to_ascii_uppercase().as_bytes()) {
        *slot = *byte;
    }
    out
}

fn short_is_mangled(name: &str, short: &[u8; 11]) -> bool {
    let rendered = String::from_utf8_lossy(short).trim_end().to_string();
    !name.eq_ignore_ascii_case(&rendered)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// In-memory 512-byte sector device for round-trips.
    struct RamImage(Vec<u8>);

    impl crate::diskfs::SectorDevice for RamImage {
        fn read_sector(&mut self, lba: u32, out: &mut [u8; SECTOR]) -> bool {
            let base = lba as usize * SECTOR;
            if base + SECTOR > self.0.len() {
                return false;
            }
            out.copy_from_slice(&self.0[base..base + SECTOR]);
            true
        }

        fn write_sector(&mut self, _lba: u32, _data: &[u8; SECTOR]) -> bool {
            false // read-only usage in tests
        }
    }

    #[test]
    fn pkg_image_round_trips_through_fat_reader() {
        let files = [
            PkgFile {
                name: "hello.orbpkg",
                data: b"orbexec-bundle-payload-hello",
            },
            PkgFile {
                name: "sysinfo.orbpkg",
                data: &[0x7Fu8, b'E', b'L', b'F', 1, 2, 3, 4, 5, 6, 7, 8, 9, 10],
            },
        ];
        let image = build_pkg_image(&files);
        let mut ram = RamImage(image);
        let mut fat = crate::fat::FatVolume::mount(&mut ram).expect("mount");
        assert_eq!(fat.kind(), crate::fat::FatKind::Fat16);

        let root_listing = fat.list_dir("/").expect("list root");
        assert!(!root_listing.is_empty(), "root listing empty");
        let _ = &root_listing;
        let listing = fat.list_dir("/pkg").expect("list /pkg");
        let names: Vec<String> = listing.iter().map(|e| e.name.clone()).collect();
        assert!(names.contains(&String::from("hello.orbpkg")), "got {names:?}");
        assert!(names.contains(&String::from("sysinfo.orbpkg")), "got {names:?}");

        let hello = fat.read_file("/pkg/hello.orbpkg").expect("read hello");
        assert_eq!(hello, b"orbexec-bundle-payload-hello");
        let sysinfo = fat.read_file("/pkg/sysinfo.orbpkg").expect("read sysinfo");
        assert_eq!(&sysinfo[..4], &[0x7F, b'E', b'L', b'F']);
    }

    #[test]
    fn multi_cluster_files_round_trip() {
        let big: Vec<u8> = (0..5000u32).map(|i| (i % 251) as u8).collect();
        let files = [PkgFile {
            name: "big.orbpkg",
            data: &big,
        }];
        let image = build_pkg_image(&files);
        let mut ram = RamImage(image);
        let mut fat = crate::fat::FatVolume::mount(&mut ram).expect("mount");
        let read = fat.read_file("/pkg/big.orbpkg").expect("read big");
        assert_eq!(read.len(), big.len());
        assert_eq!(read, big);
    }
}
