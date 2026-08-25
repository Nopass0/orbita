use alloc::collections::BTreeMap;

use crate::{
    BlockDevice, BlockDeviceError, FsMountDescriptor, MountedVolumeState, VolumeId,
};

/// Mount-time errors surfaced by the runtime registry.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum RuntimeMountError {
    AlreadyMounted,
    DeviceError(BlockDeviceError),
    MissingVolume,
}

/// In-memory filesystem registry that kernel code can use at boot.
pub struct FilesystemRuntime {
    mounts: BTreeMap<VolumeId, MountedVolumeState>,
}

impl FilesystemRuntime {
    pub fn new() -> Self {
        Self {
            mounts: BTreeMap::new(),
        }
    }

    pub fn mount<D: BlockDevice>(
        &mut self,
        descriptor: FsMountDescriptor,
        device: &D,
    ) -> Result<&MountedVolumeState, RuntimeMountError> {
        if self.mounts.contains_key(&descriptor.volume) {
            return Err(RuntimeMountError::AlreadyMounted);
        }

        let geometry = device.geometry();
        let mut state = MountedVolumeState::new(descriptor, geometry);
        state.mark_mounted();
        self.mounts.insert(descriptor.volume, state);
        Ok(self.mounts.get(&descriptor.volume).unwrap())
    }

    pub fn mount_mut<D: BlockDevice>(
        &mut self,
        descriptor: FsMountDescriptor,
        device: &D,
    ) -> Result<&mut MountedVolumeState, RuntimeMountError> {
        if self.mounts.contains_key(&descriptor.volume) {
            return Err(RuntimeMountError::AlreadyMounted);
        }

        let geometry = device.geometry();
        let mut state = MountedVolumeState::new(descriptor, geometry);
        state.mark_mounted();
        self.mounts.insert(descriptor.volume, state);
        Ok(self.mounts.get_mut(&descriptor.volume).unwrap())
    }

    pub fn unmount(&mut self, volume: VolumeId) -> Option<MountedVolumeState> {
        self.mounts.remove(&volume)
    }

    pub fn get(&self, volume: VolumeId) -> Option<&MountedVolumeState> {
        self.mounts.get(&volume)
    }

    pub fn get_mut(&mut self, volume: VolumeId) -> Option<&mut MountedVolumeState> {
        self.mounts.get_mut(&volume)
    }

    pub fn len(&self) -> usize {
        self.mounts.len()
    }

    pub fn is_empty(&self) -> bool {
        self.mounts.is_empty()
    }
}

impl Default for FilesystemRuntime {
    fn default() -> Self {
        Self::new()
    }
}
