# Orbita filesystem API

The storage stack has three layers, each a separate module (KISS/DRY by
construction — one concern per file):

```text
crates/orbita-hw/src/ahci.rs        transport: real SATA DMA driver
crates/orbita-fs/src/diskfs.rs      OrbitaFS: persistent block filesystem
crates/orbita-fs/src/memory.rs      MemoryVolume: RAM-backed volume
```

Any block device implements `SectorDevice` (`read_sector`/`write_sector`)
and OrbitaFS runs on top — AHCI in the kernel, a RAM array in tests.

## On-disk layout (512-byte sectors)

```text
sector 0         superblock: magic "ORBFS1", version, geometry, checksum
sector 1..       free-space bitmap (1 bit per data block)
next 32 sectors  inode table (128 B per inode, 4 per sector)
data_start..     data blocks: 8-byte next pointer + 504 B payload
```

Inodes carry `kind` (file/dir), `parent` (directory inode), name (≤56 B),
size and first block. Slot 0 is the immutable root directory `/`.

## Public API (`OrbitaDiskFs`)

| Call | Meaning |
|---|---|
| `format(device, blocks)` | initialize the volume |
| `mount(device)` | open a formatted volume |
| `write_file(dev, path, data)` | write/replace; **parent dirs auto-created** |
| `read_file(dev, path)` | read into a byte vector |
| `delete_file(dev, path)` | delete file or **empty** directory |
| `create_dir(dev, path)` | create directory (mkdir -p) |
| `list_dir(path)` | entry names of a directory |
| `list()` | all file names |
| `file_count()` / `dir_count()` | inventory |
| `capacity_bytes()` / `free_bytes()` / `used_bytes()` | capacity API |
| `usage_percent_hundredths()` | usage, 0..10000 |

Paths are hierarchical (`/etc/net/wifi.cfg`), `/`-optional, resolved
component-wise from the root. Replacement rules: a file may not replace
a directory; directories delete only when empty; the root is immutable.

## System tree (seeded on first format)

```text
/bin    system binaries (ORBEXEC, root) — orbita-init, orbita-shell
/lib    shared libraries (ORBLIB)        — liborbita-fs/net/ui
/boot   boot binaries + loader.cfg + boot counter
/etc    REAL configuration — orbita.conf: loaded from disk at boot,
        applied live (hostname), editable in the OS and persisted back
```

## Medium awareness

`StorageKind` (orbita-hw) classifies HDD (with RPM), SSD, removable,
NVMe, USB flash, RAM disks — from ATA IDENTIFY word 217 and the bus.
`AhciDisk::capacity_bytes()` reports raw disk capacity.

## Guarantees and tests

Host-side unit tests (9) cover: format/mount roundtrip, unformatted
rejection, persistence across remount, multi-block files, delete frees
blocks, replace reuses slots, nested directories lifecycle, delete
rules, capacity/usage arithmetic. Live verification: the QEMU boot
counter increments across reboots (1 → 2 → 3 …).
