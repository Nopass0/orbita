use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

use crate::{
    BlockDeviceGeometry, BlockSize, DirectoryCreateOptions, DirectoryCursor, DirectoryHandle,
    DirectoryListing, DirectoryListingEntry, DirectoryOpenOptions, DirectoryRecord, FileCreateOptions,
    FileHandle, FileMode, FileOpenOptions, FileType, FilesystemVolume, FsCapabilities, FsChecksumPolicy,
    FsCompressionPolicy, FsError, FsLayout, FsObjectHandle, FsPartition, InodeId, InodeKind,
    InodeMetadata, InodePermissions, OpenDirectoryHandle, OpenFileHandle, ReadResult,
    Superblock, SuperblockFlags, SyncReport, VolumeId, VolumeInspector, VolumeSpaceStats,
    VolumeStatistics, WriteResult,
};

#[derive(Debug, Clone)]
enum MemoryNodeData {
    File(Vec<u8>),
    Directory(BTreeMap<String, InodeId>),
}

#[derive(Debug, Clone)]
struct MemoryNode {
    metadata: InodeMetadata,
    parent: Option<InodeId>,
    data: MemoryNodeData,
}

impl MemoryNode {
    fn file(inode: InodeId, parent: InodeId, permissions: InodePermissions, mode: FileMode) -> Self {
        Self {
            metadata: InodeMetadata {
                inode,
                kind: InodeKind::File,
                file_type: FileType::Regular,
                permissions,
                mode,
                size_bytes: 0,
                blocks: 0,
                generation: 0,
            },
            parent: Some(parent),
            data: MemoryNodeData::File(Vec::new()),
        }
    }

    fn directory(
        inode: InodeId,
        parent: Option<InodeId>,
        permissions: InodePermissions,
        mode: FileMode,
    ) -> Self {
        Self {
            metadata: InodeMetadata {
                inode,
                kind: InodeKind::Directory,
                file_type: FileType::Directory,
                permissions,
                mode,
                size_bytes: 0,
                blocks: 0,
                generation: 0,
            },
            parent,
            data: MemoryNodeData::Directory(BTreeMap::new()),
        }
    }
}

/// In-memory filesystem backend used for the early shell and UI bring-up path.
///
/// This backend is intentionally simple: it provides a real mutable namespace
/// and file contents without pretending to be a durable disk implementation.
pub struct MemoryVolume {
    volume: VolumeId,
    geometry: BlockDeviceGeometry,
    layout: FsLayout,
    capabilities: FsCapabilities,
    superblock: Superblock,
    next_inode: u64,
    next_handle: u64,
    nodes: BTreeMap<InodeId, MemoryNode>,
    handles: BTreeMap<FsObjectHandle, InodeId>,
}

impl MemoryVolume {
    pub fn new(volume: VolumeId, block_size: BlockSize, block_count: u64, capabilities: FsCapabilities) -> Self {
        let geometry = BlockDeviceGeometry {
            block_size,
            block_count,
        };
        let layout = FsLayout {
            partition: FsPartition {
                volume,
                superblock: crate::BlockAddress(1),
                inode_table: crate::BlockAddress(8),
                journal_start: crate::BlockAddress(128),
            },
            capacity_blocks: block_count,
            reserved: crate::SpaceReservation {
                data_blocks: 8,
                metadata_blocks: 8,
            },
            capabilities,
        };

        let root_inode = InodeId(1);
        let mut nodes = BTreeMap::new();
        nodes.insert(
            root_inode,
            MemoryNode::directory(
                root_inode,
                None,
                InodePermissions(0o755),
                FileMode(0o040755),
            ),
        );

        Self {
            volume,
            geometry,
            layout,
            capabilities,
            superblock: Superblock {
                magic: *b"ORBITAFS",
                version_major: 0,
                version_minor: 1,
                block_size,
                volume_id: volume,
                flags: SuperblockFlags(
                    SuperblockFlags::CLEAN.0 | SuperblockFlags::JOURNAL_PRESENT.0,
                ),
                checksum_policy: FsChecksumPolicy::MetadataAndData,
                compression_policy: FsCompressionPolicy::Adaptive,
                root_inode: root_inode.0,
                journal_inode: 2,
                free_space_root: 3,
                features_crc: capabilities.features.len() as u32,
            },
            next_inode: 2,
            next_handle: 1,
            nodes,
            handles: BTreeMap::new(),
        }
    }

    pub fn create_dir_all(&mut self, path: &str) -> Result<(), FsError> {
        let components = normalize_components(path)?;
        let mut current = InodeId(self.superblock.root_inode);
        for component in components {
            if let Some(child) = self.lookup_child(current, component)? {
                let child_node = self.node(child)?;
                if !child_node.metadata.is_directory() {
                    return Err(FsError::NotDirectory);
                }
                current = child;
            } else {
                let parent_handle = DirectoryHandle(self.handle_for_inode(current));
                let dir = self.create_directory(
                    parent_handle,
                    component,
                    DirectoryCreateOptions {
                        create_parents: true,
                        ..DirectoryCreateOptions::default()
                    },
                )?;
                current = dir.inode;
            }
        }
        Ok(())
    }

    pub fn create_file_path(&mut self, path: &str, contents: &[u8]) -> Result<(), FsError> {
        let (parent, name) = self.resolve_parent_path(path)?;
        let parent_handle = DirectoryHandle(self.handle_for_inode(parent));
        let file = match self.open_file(
            parent_handle,
            name,
            FileOpenOptions {
                read: true,
                write: true,
                create: true,
                truncate: true,
                ..FileOpenOptions::default()
            },
        ) {
            Ok(file) => file,
            Err(FsError::NotFound) => {
                let parent_handle = DirectoryHandle(self.handle_for_inode(parent));
                self.create_file(parent_handle, name, FileCreateOptions::default())?
            }
            Err(err) => return Err(err),
        };
        self.write_file(file.handle, 0, contents)?;
        Ok(())
    }

    pub fn read_file_path(&mut self, path: &str) -> Result<Vec<u8>, FsError> {
        let inode = self.resolve_path(path)?;
        let metadata = self.node(inode)?.metadata;
        if !metadata.is_file() {
            return Err(FsError::IsDirectory);
        }
        let mut data = vec![0; metadata.size_bytes as usize];
        let file = OpenFileHandle {
            handle: FileHandle(self.handle_for_inode(inode)),
            inode,
            metadata,
            options: FileOpenOptions::read_only(),
        };
        let _ = self.read_file(file.handle, 0, &mut data)?;
        Ok(data)
    }

    pub fn list_path(&self, path: &str) -> Result<DirectoryListing, FsError> {
        let inode = self.resolve_path(path)?;
        let node = self.node(inode)?;
        if !node.metadata.is_directory() {
            return Err(FsError::NotDirectory);
        }
        self.list_directory(DirectoryHandle(FsObjectHandle(inode.0)), DirectoryCursor(0), usize::MAX)
    }

    pub fn remove_path(&mut self, path: &str) -> Result<(), FsError> {
        let (parent, name) = self.resolve_parent_path(path)?;
        let parent_handle = DirectoryHandle(self.handle_for_inode(parent));
        self.remove_entry(parent_handle, name)
    }

    pub fn rename_path(&mut self, old_path: &str, new_path: &str) -> Result<(), FsError> {
        let (old_parent, old_name) = self.resolve_parent_path(old_path)?;
        let (new_parent, new_name) = self.resolve_parent_path(new_path)?;
        let old_parent_handle = DirectoryHandle(self.handle_for_inode(old_parent));
        let new_parent_handle = DirectoryHandle(self.handle_for_inode(new_parent));
        self.rename_entry(
            old_parent_handle,
            old_name,
            new_parent_handle,
            new_name,
        )
    }

    fn resolve_path(&self, path: &str) -> Result<InodeId, FsError> {
        if path == "/" || path.is_empty() {
            return Ok(InodeId(self.superblock.root_inode));
        }

        let mut current = InodeId(self.superblock.root_inode);
        for component in normalize_components(path)? {
            current = self.lookup_child(current, component)?.ok_or(FsError::NotFound)?;
        }
        Ok(current)
    }

    fn resolve_parent_path<'a>(&self, path: &'a str) -> Result<(InodeId, &'a str), FsError> {
        let trimmed = path.trim();
        if trimmed.is_empty() || trimmed == "/" {
            return Err(FsError::InvalidPath);
        }

        let (parent_path, name) = match trimmed.rsplit_once('/') {
            Some((parent, name)) if !name.is_empty() => {
                let parent = if parent.is_empty() { "/" } else { parent };
                (parent, name)
            }
            None => ("/", trimmed),
            _ => return Err(FsError::InvalidPath),
        };

        validate_name(name)?;
        Ok((self.resolve_path(parent_path)?, name))
    }

    fn lookup_child(&self, parent: InodeId, name: &str) -> Result<Option<InodeId>, FsError> {
        let node = self.node(parent)?;
        match &node.data {
            MemoryNodeData::Directory(children) => Ok(children.get(name).copied()),
            MemoryNodeData::File(_) => Err(FsError::NotDirectory),
        }
    }

    fn node(&self, inode: InodeId) -> Result<&MemoryNode, FsError> {
        self.nodes.get(&inode).ok_or(FsError::NotFound)
    }

    fn node_mut(&mut self, inode: InodeId) -> Result<&mut MemoryNode, FsError> {
        self.nodes.get_mut(&inode).ok_or(FsError::NotFound)
    }

    fn handle_for_inode(&mut self, inode: InodeId) -> FsObjectHandle {
        if let Some((handle, _)) = self.handles.iter().find(|(_, mapped)| **mapped == inode) {
            return *handle;
        }
        let handle = FsObjectHandle(self.next_handle);
        self.next_handle += 1;
        self.handles.insert(handle, inode);
        handle
    }

    fn inode_for_handle(&self, handle: FsObjectHandle) -> Result<InodeId, FsError> {
        if let Some(inode) = self.handles.get(&handle).copied() {
            return Ok(inode);
        }

        let inode = InodeId(handle.0);
        if self.nodes.contains_key(&inode) {
            Ok(inode)
        } else {
            Err(FsError::InvalidHandle)
        }
    }

    fn allocate_inode(&mut self) -> InodeId {
        let inode = InodeId(self.next_inode);
        self.next_inode += 1;
        inode
    }

    fn update_file_blocks(node: &mut MemoryNode, block_size: BlockSize) {
        if let MemoryNodeData::File(data) = &node.data {
            node.metadata.size_bytes = data.len() as u64;
            let bytes = data.len() as u64;
            node.metadata.blocks = if bytes == 0 {
                0
            } else {
                (bytes + block_size.0 as u64 - 1) / block_size.0 as u64
            };
        }
    }

    fn build_space_stats(&self) -> VolumeSpaceStats {
        let mut allocated_blocks = self.layout.reserved.total_blocks();
        let mut files = 0u64;
        let mut directories = 0u64;

        for node in self.nodes.values() {
            if node.metadata.is_file() {
                files += 1;
                allocated_blocks = allocated_blocks.saturating_add(node.metadata.blocks);
            } else if node.metadata.is_directory() {
                directories += 1;
                allocated_blocks = allocated_blocks.saturating_add(1);
            }
        }

        let total_blocks = self.geometry.block_count;
        let free_blocks = total_blocks.saturating_sub(allocated_blocks);
        VolumeSpaceStats {
            block_size: self.geometry.block_size,
            total_blocks,
            free_blocks,
            allocated_blocks,
            reserved_blocks: self.layout.reserved.total_blocks(),
            metadata_blocks: self.layout.reserved.metadata_blocks.saturating_add(directories),
            data_blocks: files,
            dirty_blocks: 0,
        }
    }

    fn build_statistics(&self) -> VolumeStatistics {
        let mut files = 0u64;
        let mut directories = 0u64;
        for node in self.nodes.values() {
            if node.metadata.is_file() {
                files += 1;
            } else if node.metadata.is_directory() {
                directories += 1;
            }
        }
        VolumeStatistics {
            volume: self.volume,
            mounted: true,
            readonly: false,
            clean: true,
            files,
            directories,
            symlinks: 0,
            special_nodes: 0,
            inodes: self.nodes.len() as u64,
            extents: files,
            tx_count: 0,
            last_checkpoint_tx: None,
            space: self.build_space_stats(),
        }
    }
}

impl VolumeInspector for MemoryVolume {
    fn volume_id(&self) -> VolumeId {
        self.volume
    }

    fn geometry(&self) -> BlockDeviceGeometry {
        self.geometry
    }

    fn superblock(&self) -> Superblock {
        self.superblock
    }

    fn layout(&self) -> FsLayout {
        self.layout
    }

    fn capabilities(&self) -> FsCapabilities {
        self.capabilities
    }

    fn space_stats(&self) -> VolumeSpaceStats {
        self.build_space_stats()
    }

    fn volume_stats(&self) -> VolumeStatistics {
        self.build_statistics()
    }
}

impl FilesystemVolume for MemoryVolume {
    fn root_directory(&self) -> Result<OpenDirectoryHandle, FsError> {
        let inode = InodeId(self.superblock.root_inode);
        let metadata = self.node(inode)?.metadata;
        Ok(OpenDirectoryHandle {
            handle: DirectoryHandle(FsObjectHandle(inode.0)),
            inode,
            metadata,
            options: DirectoryOpenOptions {
                read: true,
                create: false,
                create_new: false,
                follow_symlinks: false,
            },
        })
    }

    fn create_file(
        &mut self,
        parent: DirectoryHandle,
        name: &str,
        options: FileCreateOptions,
    ) -> Result<OpenFileHandle, FsError> {
        validate_name(name)?;
        let parent_inode = self.inode_for_handle(parent.0)?;
        if self.lookup_child(parent_inode, name)?.is_some() {
            return Err(FsError::AlreadyExists);
        }

        let inode = self.allocate_inode();
        let file = MemoryNode::file(inode, parent_inode, options.permissions, options.mode);
        self.nodes.insert(inode, file);
        let parent_node = self.node_mut(parent_inode)?;
        match &mut parent_node.data {
            MemoryNodeData::Directory(children) => {
                children.insert(name.to_string(), inode);
            }
            MemoryNodeData::File(_) => return Err(FsError::NotDirectory),
        }

        let handle = FileHandle(self.handle_for_inode(inode));
        let metadata = self.node(inode)?.metadata;
        Ok(OpenFileHandle {
            handle,
            inode,
            metadata,
            options: FileOpenOptions {
                read: true,
                write: true,
                ..FileOpenOptions::default()
            },
        })
    }

    fn create_directory(
        &mut self,
        parent: DirectoryHandle,
        name: &str,
        options: DirectoryCreateOptions,
    ) -> Result<OpenDirectoryHandle, FsError> {
        validate_name(name)?;
        let parent_inode = self.inode_for_handle(parent.0)?;
        if self.lookup_child(parent_inode, name)?.is_some() {
            return Err(FsError::AlreadyExists);
        }

        let inode = self.allocate_inode();
        let directory = MemoryNode::directory(inode, Some(parent_inode), options.permissions, options.mode);
        self.nodes.insert(inode, directory);
        let parent_node = self.node_mut(parent_inode)?;
        match &mut parent_node.data {
            MemoryNodeData::Directory(children) => {
                children.insert(name.to_string(), inode);
            }
            MemoryNodeData::File(_) => return Err(FsError::NotDirectory),
        }

        let handle = DirectoryHandle(self.handle_for_inode(inode));
        let metadata = self.node(inode)?.metadata;
        Ok(OpenDirectoryHandle {
            handle,
            inode,
            metadata,
            options: DirectoryOpenOptions {
                read: true,
                create: false,
                create_new: false,
                follow_symlinks: false,
            },
        })
    }

    fn open_file(
        &mut self,
        parent: DirectoryHandle,
        name: &str,
        options: FileOpenOptions,
    ) -> Result<OpenFileHandle, FsError> {
        let parent_inode = self.inode_for_handle(parent.0)?;
        let inode = match self.lookup_child(parent_inode, name)? {
            Some(inode) => inode,
            None if options.create => {
                return self.create_file(
                    parent,
                    name,
                    FileCreateOptions {
                        exclusive: options.create_new,
                        ..FileCreateOptions::default()
                    },
                );
            }
            None => return Err(FsError::NotFound),
        };

        let metadata = self.node(inode)?.metadata;
        if !metadata.is_file() {
            return Err(FsError::IsDirectory);
        }
        if options.truncate {
            let block_size = self.geometry.block_size;
            let node = self.node_mut(inode)?;
            if let MemoryNodeData::File(data) = &mut node.data {
                data.clear();
                Self::update_file_blocks(node, block_size);
            }
        }

        Ok(OpenFileHandle {
            handle: FileHandle(self.handle_for_inode(inode)),
            inode,
            metadata: self.node(inode)?.metadata,
            options,
        })
    }

    fn open_directory(
        &mut self,
        parent: DirectoryHandle,
        name: &str,
        options: DirectoryOpenOptions,
    ) -> Result<OpenDirectoryHandle, FsError> {
        let parent_inode = self.inode_for_handle(parent.0)?;
        let inode = match self.lookup_child(parent_inode, name)? {
            Some(inode) => inode,
            None if options.create => {
                return self.create_directory(
                    parent,
                    name,
                    DirectoryCreateOptions {
                        exclusive: options.create_new,
                        ..DirectoryCreateOptions::default()
                    },
                );
            }
            None => return Err(FsError::NotFound),
        };
        let metadata = self.node(inode)?.metadata;
        if !metadata.is_directory() {
            return Err(FsError::NotDirectory);
        }
        Ok(OpenDirectoryHandle {
            handle: DirectoryHandle(self.handle_for_inode(inode)),
            inode,
            metadata,
            options,
        })
    }

    fn list_directory(
        &self,
        directory: DirectoryHandle,
        cursor: DirectoryCursor,
        limit: usize,
    ) -> Result<DirectoryListing, FsError> {
        let inode = self.inode_for_handle(directory.0)?;
        let node = self.node(inode)?;
        let MemoryNodeData::Directory(children) = &node.data else {
            return Err(FsError::NotDirectory);
        };

        let mut listing = DirectoryListing::empty();
        let start = cursor.0 as usize;
        for (index, (name, child_inode)) in children.iter().enumerate().skip(start).take(limit) {
            let child = self.node(*child_inode)?;
            listing.push(DirectoryListingEntry::new(
                name.clone(),
                *child_inode,
                DirectoryRecord::new(
                    file_type_tag(child.metadata.file_type),
                    name.len() as u16,
                    checksum_name(name),
                ),
                child.metadata,
            ));
            listing.total_entries = Some(children.len() as u64);
            if index + 1 < children.len() {
                listing.next_cursor = Some(DirectoryCursor((index + 1) as u64));
            }
        }
        Ok(listing)
    }

    fn read_file(
        &mut self,
        file: FileHandle,
        offset: u64,
        dst: &mut [u8],
    ) -> Result<ReadResult, FsError> {
        let inode = self.inode_for_handle(file.0)?;
        let node = self.node(inode)?;
        let MemoryNodeData::File(data) = &node.data else {
            return Err(FsError::IsDirectory);
        };

        let offset = offset as usize;
        if offset >= data.len() {
            return Ok(ReadResult {
                bytes_read: 0,
                end_of_file: true,
            });
        }
        let count = core::cmp::min(dst.len(), data.len() - offset);
        dst[..count].copy_from_slice(&data[offset..offset + count]);
        Ok(ReadResult {
            bytes_read: count,
            end_of_file: offset + count >= data.len(),
        })
    }

    fn write_file(
        &mut self,
        file: FileHandle,
        offset: u64,
        src: &[u8],
    ) -> Result<WriteResult, FsError> {
        let inode = self.inode_for_handle(file.0)?;
        let block_size = self.geometry.block_size;
        let node = self.node_mut(inode)?;
        let MemoryNodeData::File(data) = &mut node.data else {
            return Err(FsError::IsDirectory);
        };

        let offset = offset as usize;
        if data.len() < offset {
            data.resize(offset, 0);
        }
        let end = offset.saturating_add(src.len());
        let grew = end > data.len();
        if grew {
            data.resize(end, 0);
        }
        data[offset..offset + src.len()].copy_from_slice(src);
        Self::update_file_blocks(node, block_size);
        Ok(WriteResult {
            bytes_written: src.len(),
            grew,
        })
    }

    fn truncate_file(&mut self, file: FileHandle, size: u64) -> Result<(), FsError> {
        let inode = self.inode_for_handle(file.0)?;
        let block_size = self.geometry.block_size;
        let node = self.node_mut(inode)?;
        let MemoryNodeData::File(data) = &mut node.data else {
            return Err(FsError::IsDirectory);
        };
        data.resize(size as usize, 0);
        Self::update_file_blocks(node, block_size);
        Ok(())
    }

    fn remove_entry(&mut self, parent: DirectoryHandle, name: &str) -> Result<(), FsError> {
        let parent_inode = self.inode_for_handle(parent.0)?;
        let child_inode = self.lookup_child(parent_inode, name)?.ok_or(FsError::NotFound)?;
        let child = self.node(child_inode)?;
        if let MemoryNodeData::Directory(children) = &child.data {
            if !children.is_empty() {
                return Err(FsError::NotEmpty);
            }
        }

        let parent_node = self.node_mut(parent_inode)?;
        match &mut parent_node.data {
            MemoryNodeData::Directory(children) => {
                children.remove(name);
            }
            MemoryNodeData::File(_) => return Err(FsError::NotDirectory),
        }
        self.nodes.remove(&child_inode);
        Ok(())
    }

    fn rename_entry(
        &mut self,
        old_parent: DirectoryHandle,
        old_name: &str,
        new_parent: DirectoryHandle,
        new_name: &str,
    ) -> Result<(), FsError> {
        validate_name(new_name)?;
        let old_parent_inode = self.inode_for_handle(old_parent.0)?;
        let new_parent_inode = self.inode_for_handle(new_parent.0)?;
        let moved_inode = self.lookup_child(old_parent_inode, old_name)?.ok_or(FsError::NotFound)?;
        if self.lookup_child(new_parent_inode, new_name)?.is_some() {
            return Err(FsError::AlreadyExists);
        }

        {
            let old_parent_node = self.node_mut(old_parent_inode)?;
            let MemoryNodeData::Directory(children) = &mut old_parent_node.data else {
                return Err(FsError::NotDirectory);
            };
            children.remove(old_name);
        }

        {
            let new_parent_node = self.node_mut(new_parent_inode)?;
            let MemoryNodeData::Directory(children) = &mut new_parent_node.data else {
                return Err(FsError::NotDirectory);
            };
            children.insert(new_name.to_string(), moved_inode);
        }

        let moved = self.node_mut(moved_inode)?;
        moved.parent = Some(new_parent_inode);
        Ok(())
    }

    fn flush_file(&mut self, _file: FileHandle) -> Result<(), FsError> {
        Ok(())
    }

    fn sync_volume(&mut self) -> Result<SyncReport, FsError> {
        Ok(SyncReport {
            flushed_metadata: true,
            flushed_data: true,
            committed_transactions: 0,
        })
    }
}

fn normalize_components(path: &str) -> Result<Vec<&str>, FsError> {
    let mut components = Vec::new();
    for component in path.split('/') {
        let component = component.trim();
        if component.is_empty() || component == "." {
            continue;
        }
        if component == ".." {
            return Err(FsError::Unsupported);
        }
        validate_name(component)?;
        components.push(component);
    }
    Ok(components)
}

fn validate_name(name: &str) -> Result<(), FsError> {
    if name.is_empty() {
        return Err(FsError::InvalidName);
    }
    if name.len() > 255 {
        return Err(FsError::NameTooLong);
    }
    if name.contains('/') {
        return Err(FsError::InvalidName);
    }
    Ok(())
}

fn file_type_tag(file_type: FileType) -> u8 {
    match file_type {
        FileType::Regular => 1,
        FileType::Directory => 2,
        FileType::Symlink => 3,
        FileType::Device => 4,
        FileType::Socket => 5,
        FileType::Pipe => 6,
    }
}

fn checksum_name(name: &str) -> u32 {
    name.bytes()
        .fold(0u32, |acc, byte| acc.wrapping_mul(16777619).wrapping_add(byte as u32))
}
