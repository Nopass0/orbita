# Orbita FS Layout Notes

This document describes the intended filesystem layout in more operational
terms than the architecture overview.

## Suggested block map

- block 0: boot guard or reserved sector
- block 1: superblock
- block 2..N: feature table and compatibility records
- inode region: packed inode metadata and tree roots
- journal region: sequential write log or COW metadata blocks
- free-space region: allocator state
- data region: file contents and directory payloads
- mount-time runtime state: in-memory only, not persisted on disk

The runtime API mirrors this split:

- `VolumeInfo` and `VolumeStatistics` describe the mounted volume
- `VolumeSpaceStats` reports remaining capacity in blocks and bytes
- `OpenFileHandle` and `OpenDirectoryHandle` wrap stable handle identities
- `DirectoryListing` returns owned snapshots for paged directory iteration
- `VolumeFormatRequest` and `VolumeFormatReport` define the format handshake

## Why extents

Extents keep large files fast and reduce metadata overhead. They also make it
easy to add compression and deduplication later because the filesystem can
reason about larger contiguous regions instead of many tiny blocks.

## Why journal plus COW hooks

- journal is good for metadata-heavy operations
- COW is good for atomic updates and snapshot-like growth
- the crate exposes both so the backend can choose a hybrid policy
- replay happens before the mount is marked live, so the kernel sees a clean
  `MountedVolumeState` after recovery

## Directory strategy

The contract is intentionally index-first:

- B-tree style indexes for general-purpose directories
- radix or hash indexes if a backend wants faster lookup for large trees
- stable entry records so cache layers can avoid reparsing names repeatedly

## Mount runtime

The in-memory mount path is intentionally separate from the on-disk layout.
Kernel code should create an `FsMountDescriptor`, hand it a block device
implementation, and then register the resulting `MountedVolumeState` in the
filesystem runtime. This keeps recovery and live state out of the superblock
format itself.

## File and directory operations

The high-level `volume` API is organized around explicit contracts:

- `create_file` and `create_directory` create new namespace objects under a
  typed parent directory handle
- `open_file` and `open_directory` return typed open handles with metadata and
  open options attached
- `read_file` and `write_file` are offset-based and return byte-count results
- `list_directory` is paged by cursor so large directories do not require a
  single allocation spike
- `remove_entry`, `rename_entry`, `truncate_file`, `flush_file`, and
  `sync_volume` cover the rest of the common lifecycle
