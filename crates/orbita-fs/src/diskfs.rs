//! OrbitaFS — the from-scratch persistent block filesystem.
//!
//! Layered on any 512-byte sector device (AHCI disk in the kernel, a RAM
//! mock in tests). Layout:
//!
//! ```text
//! sector 0            superblock (magic, geometry, area offsets)
//! sector 1            free-space bitmap (1 bit per data block)
//! sector 2..          inode table (4 inodes per sector, 128 B each)
//! data_start..        data blocks; file blocks chained by an 8-byte
//!                     next pointer stored at the start of each block,
//!                     leaving 504 bytes of payload per block
//! ```
//!
//! The on-disk format is versioned: `ORBFS1`.

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

/// Sector size assumed throughout.
pub const SECTOR: usize = 512;

/// Payload bytes per data block (8-byte forward pointer + payload).
const BLOCK_PAYLOAD: usize = SECTOR - 8;

/// Inodes are 128 bytes; 4 fit per sector.
const INODE_SIZE: usize = 128;
const INODES_PER_SECTOR: usize = SECTOR / INODE_SIZE;

/// Superblock magic: "ORBFS1\0\0".
const MAGIC: [u8; 8] = *b"ORBFS1\0\0";

/// Maximum number of inodes (fills 32 sectors).
pub const MAX_INODES: usize = 128;

/// On-disk superblock (sector 0).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Superblock {
    pub magic: [u8; 8],
    pub version: u32,
    pub total_blocks: u32,
    pub inode_count: u32,
    pub bitmap_sector: u32,
    pub inode_table_sector: u32,
    pub data_start_block: u32,
    pub checksum: u32,
}

/// Inode kind: 0 = file, 1 = directory.
pub const INODE_KIND_FILE: u32 = 0;
pub const INODE_KIND_DIR: u32 = 1;

/// On-disk inode. `parent` is the inode index of the containing
/// directory; the root directory is always inode 0.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Inode {
    pub valid: u32,
    pub size: u32,
    pub first_block: u32,
    pub name: [u8; 56],
    pub kind: u32,
    pub parent: u32,
}

impl Inode {
    fn empty() -> Self {
        Self {
            valid: 0,
            size: 0,
            first_block: 0,
            name: [0; 56],
            kind: INODE_KIND_FILE,
            parent: 0,
        }
    }

    pub fn is_dir(&self) -> bool {
        self.kind == INODE_KIND_DIR
    }

    fn set_name(&mut self, name: &str) {
        self.name = [0; 56];
        for (slot, byte) in self.name.iter_mut().zip(name.as_bytes().iter()) {
            *slot = *byte;
        }
    }

    fn name_text(&self) -> String {
        let len = self.name.iter().position(|&b| b == 0).unwrap_or(self.name.len());
        String::from_utf8_lossy(&self.name[..len]).into_owned()
    }
}

/// Block-sector device contract. Implemented by the AHCI disk in the
/// kernel and by a RAM array in tests.
pub trait SectorDevice {
    fn read_sector(&mut self, lba: u32, out: &mut [u8; SECTOR]) -> bool;
    fn write_sector(&mut self, lba: u32, data: &[u8; SECTOR]) -> bool;
}

/// A mounted OrbitaFS volume.
pub struct OrbitaDiskFs {
    total_blocks: u32,
    data_start_block: u32,
    /// Sector index of the inode table.
    inode_table_sector: u32,
    /// Sector index of the bitmap.
    bitmap_sector: u32,
    inodes: Vec<Inode>,
    bitmap: Vec<u8>,
}

/// Why a mount failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MountError {
    NotFormatted,
    IoFailure,
    GeometryInvalid,
}

impl OrbitaDiskFs {
    /// Formats the device: writes superblock, empty bitmap, empty inodes.
    pub fn format<D: SectorDevice>(device: &mut D, total_blocks: u32) -> Result<(), MountError> {
        if total_blocks < 64 {
            return Err(MountError::GeometryInvalid);
        }
        let bitmap_sector = 1u32;
        let bitmap_sectors = bitmap_sectors_for(total_blocks);
        let inode_table_sector = 1 + bitmap_sectors;
        let inode_sectors = (MAX_INODES / INODES_PER_SECTOR) as u32;
        let data_start_block = 1 + bitmap_sectors + inode_sectors;

        // Empty bitmap: everything before data_start is used, rest free.
        let mut bitmap = vec![0u8; bitmap_sectors as usize * SECTOR];
        for block in data_start_block..total_blocks {
            set_bit(&mut bitmap, block as usize, true);
        }
        for sector in 0..bitmap_sectors {
            let mut chunk = [0u8; SECTOR];
            chunk.copy_from_slice(
                &bitmap[sector as usize * SECTOR..(sector as usize + 1) * SECTOR],
            );
            if !device.write_sector(bitmap_sector + sector, &chunk) {
                return Err(MountError::IoFailure);
            }
        }

        // Inode table: zeroed, then slot 0 becomes the root directory.
        for index in 0..inode_sectors {
            if !device.write_sector(inode_table_sector + index, &[0u8; SECTOR]) {
                return Err(MountError::IoFailure);
            }
        }
        let mut sector = [0u8; SECTOR];
        let mut root = Inode::empty();
        root.valid = 1;
        root.kind = INODE_KIND_DIR;
        root.set_name("/");
        sector[..INODE_SIZE].copy_from_slice(&encode_inode(&root));
        if !device.write_sector(inode_table_sector, &sector) {
            return Err(MountError::IoFailure);
        }

        let sb = Superblock {
            magic: MAGIC,
            version: 1,
            total_blocks,
            inode_count: MAX_INODES as u32,
            bitmap_sector,
            inode_table_sector,
            data_start_block,
            checksum: 0,
        };
        let mut sector = [0u8; SECTOR];
        encode_superblock(&sb, &mut sector);
        if !device.write_sector(0, &sector) {
            return Err(MountError::IoFailure);
        }
        Ok(())
    }

    /// Mounts a formatted volume.
    pub fn mount<D: SectorDevice>(device: &mut D) -> Result<Self, MountError> {
        let mut sector = [0u8; SECTOR];
        if !device.read_sector(0, &mut sector) {
            return Err(MountError::IoFailure);
        }
        let Some(sb) = decode_superblock(&sector) else {
            return Err(MountError::NotFormatted);
        };
        if sb.magic != MAGIC || sb.version != 1 || sb.total_blocks < 64 {
            return Err(MountError::NotFormatted);
        }

        // Bitmap.
        let bitmap_sectors = bitmap_sectors_for(sb.total_blocks);
        let mut bitmap = vec![0u8; bitmap_sectors as usize * SECTOR];
        for index in 0..bitmap_sectors {
            let mut chunk = [0u8; SECTOR];
            if !device.read_sector(sb.bitmap_sector + index, &mut chunk) {
                return Err(MountError::IoFailure);
            }
            bitmap[index as usize * SECTOR..(index as usize + 1) * SECTOR]
                .copy_from_slice(&chunk);
        }

        // Inode table.
        let mut inodes = Vec::new();
        for index in 0..(MAX_INODES / INODES_PER_SECTOR) {
            let mut chunk = [0u8; SECTOR];
            if !device.read_sector(sb.inode_table_sector + index as u32, &mut chunk) {
                return Err(MountError::IoFailure);
            }
            for slot in 0..INODES_PER_SECTOR {
                let bytes =
                    &chunk[slot * INODE_SIZE..(slot + 1) * INODE_SIZE];
                inodes.push(decode_inode(bytes));
            }
        }

        Ok(Self {
            total_blocks: sb.total_blocks,
            data_start_block: sb.data_start_block,
            inode_table_sector: sb.inode_table_sector,
            bitmap_sector: sb.bitmap_sector,
            inodes,
            bitmap,
        })
    }

    /// Lists valid file names.
    pub fn list(&self) -> Vec<String> {
        self.inodes
            .iter()
            .filter(|i| i.valid != 0)
            .map(|i| i.name_text())
            .collect()
    }

    pub fn file_count(&self) -> usize {
        self.inodes
            .iter()
            .filter(|i| i.valid != 0 && i.kind == INODE_KIND_FILE)
            .count()
    }

    pub fn dir_count(&self) -> usize {
        self.inodes
            .iter()
            .filter(|i| i.valid != 0 && i.kind == INODE_KIND_DIR)
            .count()
    }

    /// Reads a file into a byte vector.
    pub fn read_file<D: SectorDevice>(
        &self,
        device: &mut D,
        name: &str,
    ) -> Option<Vec<u8>> {
        let inode = self.find_inode(name)?;
        if inode.kind != INODE_KIND_FILE {
            return None; // directories have no byte content
        }
        let mut out = Vec::new();
        let mut remaining = inode.size as usize;
        let mut block = inode.first_block;
        let mut sector = [0u8; SECTOR];
        while block != 0 && remaining > 0 {
            if !device.read_sector(block, &mut sector) {
                return None;
            }
            let next = u64::from_le_bytes(sector[..8].try_into().ok()?) as u32;
            let take = remaining.min(BLOCK_PAYLOAD);
            out.extend_from_slice(&sector[8..8 + take]);
            remaining -= take;
            block = next;
        }
        Some(out)
    }

    /// Writes a file (creating or replacing; parent directories are
    /// created automatically). Frees the old chain first.
    pub fn write_file<D: SectorDevice>(
        &mut self,
        device: &mut D,
        path: &str,
        data: &[u8],
    ) -> bool {
        let components = Self::split_path(path);
        let Some(name) = components.last().copied() else {
            return false;
        };
        if name.is_empty() || name.len() > 56 {
            return false;
        }
        let parent = if components.len() == 1 {
            0usize
        } else {
            match self.ensure_dir(device, &components[..components.len() - 1].join("/")) {
                Some(index) => index,
                None => return false,
            }
        };
        // Remove a same-named file sibling first; never replace a directory.
        if let Some(existing) = self.child_index(parent as u32, name) {
            if self.inodes[existing].kind == INODE_KIND_DIR {
                return false;
            }
            self.delete_file(device, path);
        }

        let blocks_needed = data.len().div_ceil(BLOCK_PAYLOAD);
        let chain = match self.alloc_blocks(blocks_needed) {
            Some(chain) if chain.len() == blocks_needed => chain,
            _ => return false,
        };

        for (index, &block) in chain.iter().enumerate() {
            let mut sector = [0u8; SECTOR];
            let next = if index + 1 < chain.len() { chain[index + 1] } else { 0 };
            sector[..8].copy_from_slice(&(next as u64).to_le_bytes());
            let start = index * BLOCK_PAYLOAD;
            let end = (start + BLOCK_PAYLOAD).min(data.len());
            sector[8..8 + (end - start)].copy_from_slice(&data[start..end]);
            if !device.write_sector(block, &sector) {
                return false;
            }
        }

        let slot = match self.inodes.iter().position(|i| i.valid == 0) {
            Some(slot) => slot,
            None => {
                self.free_blocks(&chain);
                return false;
            }
        };
        let mut inode = Inode::empty();
        inode.valid = 1;
        inode.size = data.len() as u32;
        inode.first_block = chain.first().copied().unwrap_or(0);
        inode.kind = INODE_KIND_FILE;
        inode.parent = parent as u32;
        inode.set_name(name);
        self.inodes[slot] = inode;

        self.flush_bitmap(device) && self.flush_inode(device, slot)
    }

    /// Deletes a file, or an empty directory, freeing its blocks.
    pub fn delete_file<D: SectorDevice>(&mut self, device: &mut D, path: &str) -> bool {
        let Some(slot) = self.resolve_index(path) else {
            return false;
        };
        if slot == 0 {
            return false; // root is immutable
        }
        let inode = self.inodes[slot];
        if inode.kind == INODE_KIND_DIR
            && self.inodes.iter().any(|i| i.valid != 0 && i.parent == slot as u32)
        {
            return false; // directory not empty
        }
        let mut block = inode.first_block;
        let mut sector = [0u8; SECTOR];
        let mut freed = Vec::new();
        while block != 0 && freed.len() < self.total_blocks as usize {
            if !device.read_sector(block, &mut sector) {
                break;
            }
            let next = u64::from_le_bytes(sector[..8].try_into().unwrap_or([0; 8])) as u32;
            freed.push(block);
            if next == block {
                break; // defensive: corrupted chain
            }
            block = next;
        }
        self.free_blocks(&freed);
        self.inodes[slot] = Inode::empty();
        self.flush_bitmap(device) && self.flush_inode(device, slot)
    }

    pub fn total_blocks(&self) -> u32 {
        self.total_blocks
    }

    pub fn free_blocks_count(&self) -> u32 {
        // The bitmap only tracks data blocks (metadata blocks are never
        // set), so the popcount is exactly the free count.
        self.bitmap.iter().map(|byte| byte.count_ones()).sum()
    }

    // -----------------------------------------------------------------
    // Hierarchical paths: components walked from the root directory
    // (inode 0). "a/b/c.txt" and "/a/b/c.txt" are equivalent.
    // -----------------------------------------------------------------

    fn split_path(path: &str) -> Vec<&str> {
        path.split('/').filter(|c| !c.is_empty()).collect()
    }

    fn child_index(&self, parent: u32, name: &str) -> Option<usize> {
        if parent == 0 && name == "/" {
            return Some(0);
        }
        self.inodes.iter().position(|i| {
            i.valid != 0 && i.parent == parent && i.name_text() == name
        })
    }

    /// Resolves a path to an inode index.
    fn resolve_index(&self, path: &str) -> Option<usize> {
        if Self::split_path(path).is_empty() {
            return Some(0); // root
        }
        let mut current = 0usize;
        for component in Self::split_path(path) {
            if self.inodes[current].kind != INODE_KIND_DIR {
                return None;
            }
            current = self.child_index(current as u32, component)?;
        }
        Some(current)
    }

    fn find_inode(&self, name: &str) -> Option<&Inode> {
        self.resolve_index(name).map(|index| &self.inodes[index])
    }

    /// Whether `path` resolves to a directory.
    pub fn is_dir(&self, path: &str) -> bool {
        self.find_inode(path).is_some_and(Inode::is_dir)
    }

    /// Lists the entry names of a directory.
    pub fn list_dir(&self, path: &str) -> Option<Vec<String>> {
        let index = self.resolve_index(path)?;
        if self.inodes[index].kind != INODE_KIND_DIR {
            return None;
        }
        let parent = index as u32;
        // `enumerate + index != parent` matters for the root directory:
        // the root inode's parent is 0 — its own index — so without the
        // exclusion list_dir("/") would return the root itself and
        // recursive walks would loop until the stack overflows.
        Some(
            self.inodes
                .iter()
                .enumerate()
                .filter(|(i, inode)| *i as u32 != parent && inode.valid != 0 && inode.parent == parent)
                .map(|(_, inode)| inode.name_text())
                .collect(),
        )
    }

    /// Creates a directory (missing parents are created automatically).
    pub fn create_dir<D: SectorDevice>(&mut self, device: &mut D, path: &str) -> bool {
        self.ensure_dir(device, path).is_some()
    }

    /// Walks/creates the directory chain for `path`, returning its inode index.
    fn ensure_dir<D: SectorDevice>(&mut self, device: &mut D, path: &str) -> Option<usize> {
        let mut current = 0usize;
        for component in Self::split_path(path) {
            match self.child_index(current as u32, component) {
                Some(index) => {
                    if self.inodes[index].kind != INODE_KIND_DIR {
                        return None;
                    }
                    current = index;
                }
                None => {
                    let slot = self.inodes.iter().position(|i| i.valid == 0)?;
                    let mut inode = Inode::empty();
                    inode.valid = 1;
                    inode.kind = INODE_KIND_DIR;
                    inode.parent = current as u32;
                    inode.set_name(component);
                    self.inodes[slot] = inode;
                    if !self.flush_inode(device, slot) {
                        return None;
                    }
                    current = slot;
                }
            }
        }
        Some(current)
    }

    /// Capacity of the data area in bytes.
    pub fn capacity_bytes(&self) -> u64 {
        (self.total_blocks.saturating_sub(self.data_start_block) as u64) * SECTOR as u64
    }

    /// Free bytes in the data area.
    pub fn free_bytes(&self) -> u64 {
        self.free_blocks_count() as u64 * SECTOR as u64
    }

    /// Used bytes in the data area.
    pub fn used_bytes(&self) -> u64 {
        self.capacity_bytes().saturating_sub(self.free_bytes())
    }

    /// Usage in hundredths of a percent (0..=10000).
    pub fn usage_percent_hundredths(&self) -> u32 {
        let capacity = self.capacity_bytes();
        if capacity == 0 {
            return 0;
        }
        ((self.used_bytes() * 100_00) / capacity) as u32
    }

    /// Allocates `count` free blocks (first-fit) and marks them used.
    fn alloc_blocks(&mut self, count: usize) -> Option<Vec<u32>> {
        let mut found = Vec::new();
        let mut block = self.data_start_block as usize;
        while found.len() < count && block < self.total_blocks as usize {
            if bit_set(&self.bitmap, block) {
                found.push(block as u32);
                set_bit(&mut self.bitmap, block, false);
            }
            block += 1;
        }
        if found.len() == count {
            Some(found)
        } else {
            // Roll back partial allocations.
            for &b in &found {
                set_bit(&mut self.bitmap, b as usize, true);
            }
            None
        }
    }

    fn free_blocks(&mut self, blocks: &[u32]) {
        for &block in blocks {
            if block >= self.data_start_block && block < self.total_blocks {
                set_bit(&mut self.bitmap, block as usize, true);
            }
        }
    }

    fn flush_bitmap<D: SectorDevice>(&self, device: &mut D) -> bool {
        let sectors = self.bitmap.len() / SECTOR;
        for index in 0..sectors {
            let mut chunk = [0u8; SECTOR];
            chunk.copy_from_slice(&self.bitmap[index * SECTOR..(index + 1) * SECTOR]);
            if !device.write_sector(self.bitmap_sector + index as u32, &chunk) {
                return false;
            }
        }
        true
    }

    fn flush_inode<D: SectorDevice>(&self, device: &mut D, slot: usize) -> bool {
        let sector_index = slot / INODES_PER_SECTOR;
        let offset_in_sector = slot % INODES_PER_SECTOR;
        let mut chunk = [0u8; SECTOR];
        // Read-modify-write to preserve sibling inodes.
        if !device.read_sector(self.inode_table_sector + sector_index as u32, &mut chunk) {
            return false;
        }
        let bytes = encode_inode(&self.inodes[slot]);
        chunk[offset_in_sector * INODE_SIZE..(offset_in_sector + 1) * INODE_SIZE]
            .copy_from_slice(&bytes);
        device.write_sector(self.inode_table_sector + sector_index as u32, &chunk)
    }
}

fn bitmap_sectors_for(total_blocks: u32) -> u32 {
    ((total_blocks as usize).div_ceil(8)).div_ceil(SECTOR) as u32
}

fn bit_set(bitmap: &[u8], index: usize) -> bool {
    bitmap[index / 8] & (1 << (index % 8)) != 0
}

fn set_bit(bitmap: &mut [u8], index: usize, value: bool) {
    if value {
        bitmap[index / 8] |= 1 << (index % 8);
    } else {
        bitmap[index / 8] &= !(1 << (index % 8));
    }
}

fn encode_superblock(sb: &Superblock, out: &mut [u8; SECTOR]) {
    out.fill(0);
    out[..8].copy_from_slice(&sb.magic);
    out[8..12].copy_from_slice(&sb.version.to_le_bytes());
    out[12..16].copy_from_slice(&sb.total_blocks.to_le_bytes());
    out[16..20].copy_from_slice(&sb.inode_count.to_le_bytes());
    out[20..24].copy_from_slice(&sb.bitmap_sector.to_le_bytes());
    out[24..28].copy_from_slice(&sb.inode_table_sector.to_le_bytes());
    out[28..32].copy_from_slice(&sb.data_start_block.to_le_bytes());
    let checksum = out[..32]
        .iter()
        .fold(0u32, |acc, byte| acc.wrapping_add(*byte as u32));
    out[32..36].copy_from_slice(&checksum.to_le_bytes());
}

fn decode_superblock(sector: &[u8; SECTOR]) -> Option<Superblock> {
    let checksum = sector[..32]
        .iter()
        .fold(0u32, |acc, byte| acc.wrapping_add(*byte as u32));
    let stored = u32::from_le_bytes(sector[32..36].try_into().ok()?);
    if checksum != stored {
        return None;
    }
    Some(Superblock {
        magic: sector[..8].try_into().ok()?,
        version: u32::from_le_bytes(sector[8..12].try_into().ok()?),
        total_blocks: u32::from_le_bytes(sector[12..16].try_into().ok()?),
        inode_count: u32::from_le_bytes(sector[16..20].try_into().ok()?),
        bitmap_sector: u32::from_le_bytes(sector[20..24].try_into().ok()?),
        inode_table_sector: u32::from_le_bytes(sector[24..28].try_into().ok()?),
        data_start_block: u32::from_le_bytes(sector[28..32].try_into().ok()?),
        checksum,
    })
}

fn encode_inode(inode: &Inode) -> [u8; INODE_SIZE] {
    let mut out = [0u8; INODE_SIZE];
    out[..4].copy_from_slice(&inode.valid.to_le_bytes());
    out[4..8].copy_from_slice(&inode.size.to_le_bytes());
    out[8..12].copy_from_slice(&inode.first_block.to_le_bytes());
    out[12..68].copy_from_slice(&inode.name);
    out[68..72].copy_from_slice(&inode.kind.to_le_bytes());
    out[72..76].copy_from_slice(&inode.parent.to_le_bytes());
    out
}

fn decode_inode(bytes: &[u8]) -> Inode {
    Inode {
        valid: u32::from_le_bytes(bytes[..4].try_into().unwrap_or([0; 4])),
        size: u32::from_le_bytes(bytes[4..8].try_into().unwrap_or([0; 4])),
        first_block: u32::from_le_bytes(bytes[8..12].try_into().unwrap_or([0; 4])),
        name: bytes[12..68].try_into().unwrap_or([0; 56]),
        kind: u32::from_le_bytes(bytes[68..72].try_into().unwrap_or([0; 4])),
        parent: u32::from_le_bytes(bytes[72..76].try_into().unwrap_or([0; 4])),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    /// RAM-backed sector device.
    struct RamDisk {
        sectors: Vec<[u8; SECTOR]>,
        fails: bool,
    }

    impl RamDisk {
        fn new(sectors: u32) -> Self {
            Self {
                sectors: vec![[0u8; SECTOR]; sectors as usize],
                fails: false,
            }
        }
    }

    impl SectorDevice for RamDisk {
        fn read_sector(&mut self, lba: u32, out: &mut [u8; SECTOR]) -> bool {
            match self.sectors.get(lba as usize) {
                Some(sector) if !self.fails => {
                    out.copy_from_slice(sector);
                    true
                }
                _ => false,
            }
        }

        fn write_sector(&mut self, lba: u32, data: &[u8; SECTOR]) -> bool {
            match self.sectors.get_mut(lba as usize) {
                Some(sector) if !self.fails => {
                    *sector = *data;
                    true
                }
                _ => false,
            }
        }
    }

    #[test]
    fn format_mount_roundtrip() {
        let mut disk = RamDisk::new(1024);
        OrbitaDiskFs::format(&mut disk, 1024).unwrap();
        let fs = OrbitaDiskFs::mount(&mut disk).unwrap();
        assert_eq!(fs.total_blocks(), 1024);
        assert_eq!(fs.file_count(), 0);
    }


    #[test]
    fn directories_nested_lifecycle() {
        let mut disk = RamDisk::new(2048);
        OrbitaDiskFs::format(&mut disk, 2048).unwrap();
        let mut fs = OrbitaDiskFs::mount(&mut disk).unwrap();
        assert!(fs.write_file(&mut disk, "/etc/net/wifi.cfg", b"ssid=orbita"));
        assert!(fs.write_file(&mut disk, "/etc/net/eth.cfg", b"dhcp"));
        assert!(fs.write_file(&mut disk, "/home/user/notes/todo.txt", b"build os"));
        // Directories auto-created, visible in listings.
        let root = fs.list_dir("/").unwrap();
        assert!(root.contains(&String::from("etc")) && root.contains(&String::from("home")));
        let net = fs.list_dir("/etc/net").unwrap();
        assert_eq!(net.len(), 2);
        assert_eq!(fs.file_count(), 3);
        assert!(fs.dir_count() >= 4); // root + etc + net + home + user
        // Remount: hierarchy survives.
        drop(fs);
        let fs = OrbitaDiskFs::mount(&mut disk).unwrap();
        assert_eq!(fs.read_file(&mut disk, "/etc/net/wifi.cfg").unwrap(), b"ssid=orbita".to_vec());
        assert_eq!(fs.list_dir("/home/user").unwrap()[0], "notes");
    }

    #[test]
    fn delete_rules_for_dirs() {
        let mut disk = RamDisk::new(512);
        OrbitaDiskFs::format(&mut disk, 512).unwrap();
        let mut fs = OrbitaDiskFs::mount(&mut disk).unwrap();
        fs.write_file(&mut disk, "docs/a.txt", b"x");
        assert!(!fs.delete_file(&mut disk, "docs"), "non-empty dir must stay");
        assert!(fs.delete_file(&mut disk, "docs/a.txt"));
        assert!(fs.delete_file(&mut disk, "docs"), "empty dir removable");
        assert!(!fs.delete_file(&mut disk, "/"), "root immutable");
        // File cannot replace a directory.
        fs.create_dir(&mut disk, "cfg");
        assert!(!fs.write_file(&mut disk, "cfg", b"not allowed"));
    }

    #[test]
    fn capacity_and_usage_api() {
        let mut disk = RamDisk::new(1024);
        OrbitaDiskFs::format(&mut disk, 1024).unwrap();
        let mut fs = OrbitaDiskFs::mount(&mut disk).unwrap();
        let capacity = fs.capacity_bytes();
        assert!(capacity > 0);
        assert_eq!(fs.used_bytes(), 0);
        fs.write_file(&mut disk, "/big.bin", &[7u8; 2000]);
        let used = fs.used_bytes();
        assert!(used >= 2000 && used < capacity);
        assert!(fs.usage_percent_hundredths() > 0);
        assert_eq!(fs.free_bytes() + fs.used_bytes(), fs.capacity_bytes());
    }

    #[test]
    fn root_listing_excludes_itself_and_dirs_not_readable() {
        let mut disk = RamDisk::new(128);
        OrbitaDiskFs::format(&mut disk, 128).expect("format");
        let mut fs = OrbitaDiskFs::mount(&mut disk).expect("mount");

        let root = fs.list_dir("/").expect("root listing");
        assert!(
            !root.iter().any(|name| name.contains('/') || name.is_empty()),
            "root must not list itself or garbage: {root:?}"
        );
        assert!(fs.read_file(&mut disk, "/").is_none(), "directories are not readable as files");

        assert!(fs.write_file(&mut disk, "/bin/tool", b"payload"));
        let bin = fs.list_dir("/bin").expect("bin listing");
        assert_eq!(bin, vec![String::from("tool")]);
        assert!(fs.is_dir("/bin"));
        assert!(!fs.is_dir("/bin/tool"));
    }

    #[test]
    fn mount_unformatted_fails() {
        let mut disk = RamDisk::new(1024);
        assert_eq!(OrbitaDiskFs::mount(&mut disk).err(), Some(MountError::NotFormatted));
    }

    #[test]
    fn write_read_persist_across_remount() {
        let mut disk = RamDisk::new(2048);
        OrbitaDiskFs::format(&mut disk, 2048).unwrap();

        {
            let mut fs = OrbitaDiskFs::mount(&mut disk).unwrap();
            assert!(fs.write_file(&mut disk, "/boot/marker.txt", b"orbita boot #1"));
            assert!(fs.write_file(&mut disk, "/etc/note.md", &[0xA5u8; 2000])); // multi-block
        }
        // Remount = fresh in-memory state from disk.
        let fs = OrbitaDiskFs::mount(&mut disk).unwrap();
        assert_eq!(fs.file_count(), 2);
        assert_eq!(
            fs.read_file(&mut disk, "/boot/marker.txt").unwrap(),
            b"orbita boot #1".to_vec()
        );
        let big = fs.read_file(&mut disk, "/etc/note.md").unwrap();
        assert_eq!(big.len(), 2000);
        assert!(big.iter().all(|&b| b == 0xA5));
    }

    #[test]
    fn delete_frees_blocks() {
        let mut disk = RamDisk::new(512);
        OrbitaDiskFs::format(&mut disk, 512).unwrap();
        let mut fs = OrbitaDiskFs::mount(&mut disk).unwrap();
        let free_before = fs.free_blocks_count();
        assert!(fs.write_file(&mut disk, "a.bin", &[1u8; 4000]));
        assert!(fs.free_blocks_count() < free_before);
        assert!(fs.delete_file(&mut disk, "a.bin"));
        assert_eq!(fs.free_blocks_count(), free_before);
        assert!(fs.read_file(&mut disk, "a.bin").is_none());
    }

    #[test]
    fn replace_file_reuses_slot() {
        let mut disk = RamDisk::new(512);
        OrbitaDiskFs::format(&mut disk, 512).unwrap();
        let mut fs = OrbitaDiskFs::mount(&mut disk).unwrap();
        assert!(fs.write_file(&mut disk, "cfg", b"v1"));
        assert!(fs.write_file(&mut disk, "cfg", b"version-two-longer"));
        assert_eq!(fs.file_count(), 1);
        assert_eq!(
            fs.read_file(&mut disk, "cfg").unwrap(),
            b"version-two-longer".to_vec()
        );
    }

    #[test]
    fn io_failure_reported() {
        let mut disk = RamDisk::new(1024);
        OrbitaDiskFs::format(&mut disk, 1024).unwrap();
        disk.fails = true;
        assert_eq!(OrbitaDiskFs::mount(&mut disk).err(), Some(MountError::IoFailure));
    }
}
