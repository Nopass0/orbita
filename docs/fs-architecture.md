# Orbita FS Architecture

Orbita FS, also called NebulaFS, is the filesystem layer for Orbita OS.
It is designed around fast metadata operations, extent-based file storage,
journal or COW durability, and explicit hook points for checksum and
compression engines.

## Design goals

- fast directory lookup and stable inode/object identity
- extent-based allocation instead of per-block fragmentation
- transactional metadata updates
- checksum and compression hooks that can be swapped without API churn
- a layout that can grow into a high-performance local filesystem
- typed high-level contracts for volumes, handles, formatting, and stats

## On-disk layout

The expected volume layout is:

1. superblock
2. feature and compatibility tables
3. inode table or inode root
4. journal or COW metadata area
5. block allocator structures
6. directory index roots
7. data extents

The crate keeps this as contracts rather than hard-coded structures so the
layout can evolve.

## Metadata model

- `superblock` describes the volume, flags, policies, and root object IDs
- `inode` carries file identity, permissions, size, flags, and extent roots
- `extent tree` maps file offsets to physical blocks and supports sparse files
- `directory index` keeps name lookups fast and avoids linear scans
- `journal` or `COW` records make updates recoverable after power loss
- `mount` keeps the in-memory volume descriptor and mount state separate from the on-disk format
- `block device` contracts define how the filesystem talks to storage backends
- `volume` contracts expose free space, open handles, directory listings, and formatting requests

## Extension points

- `checksum` hook for metadata and payload verification
- `compression` hook for transparent block compression
- `block allocator` contract for free-space management
- `journal replay` contract for mount-time recovery
- `filesystem runtime` for the in-memory mounted volume registry

## High-level API surface

The `orbita-fs` crate is split into small contracts instead of one large
implementation type:

- `device` owns block I/O requests and device statistics
- `layout` owns block sizing, reservations, features, and partition hints
- `superblock` owns persistent volume metadata and format policies
- `volume` owns runtime stats, file and directory handles, directory listings,
  I/O results, and formatting requests
- `mount` and `runtime` keep mounted state separate from the on-disk model

This keeps the crate backend-neutral. A future implementation can swap in a
real allocator, journal replay engine, and directory index without changing the
public API that kernel code uses.

## Space and format model

Space reporting is expressed in blocks and bytes:

- `VolumeSpaceStats` tracks total, free, reserved, allocated, metadata, and
  dirty blocks
- helper methods convert those counters into byte counts from the mounted block
  size
- `VolumeStatistics` combines space counters with inode and object totals

Formatting is split into a request and a report:

- `VolumeFormatRequest` describes the target geometry, layout, policies, and
  feature set
- `VolumeFormatReport` records the root inode, journal inode, partition map,
  and number of blocks written during format
- `VolumeFormatError` stays separate from runtime `FsError` so format-time
  failures do not leak into normal file I/O paths

## Implementation strategy

The next step after this contract layer is a backend that binds the crate to
real block devices, mount logic, replay policy, and cache policy.
