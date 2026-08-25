use crate::{
    BlockDeviceGeometry, FsChecksumPolicy, FsCompressionPolicy, FsFeature, FsLayout, JournalPolicy,
    JournalReplayState, VolumeId,
};

/// Descriptor that kernel code can construct when it knows a filesystem volume.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct FsMountDescriptor {
    pub volume: VolumeId,
    pub layout: FsLayout,
    pub checksum_policy: FsChecksumPolicy,
    pub compression_policy: FsCompressionPolicy,
    pub journal_policy: JournalPolicy,
    pub readonly: bool,
}

impl FsMountDescriptor {
    pub fn new(volume: VolumeId, layout: FsLayout) -> Self {
        Self {
            volume,
            layout,
            checksum_policy: FsChecksumPolicy::MetadataAndData,
            compression_policy: FsCompressionPolicy::Adaptive,
            journal_policy: JournalPolicy::Hybrid,
            readonly: false,
        }
    }

    pub fn with_readonly(mut self, readonly: bool) -> Self {
        self.readonly = readonly;
        self
    }

    pub fn with_journal_policy(mut self, journal_policy: JournalPolicy) -> Self {
        self.journal_policy = journal_policy;
        self
    }
}

/// In-memory mount state for a single volume.
#[derive(Debug, Clone)]
pub struct MountedVolumeState {
    pub descriptor: FsMountDescriptor,
    pub geometry: BlockDeviceGeometry,
    pub journal: JournalReplayState,
    pub mounted: bool,
    pub dirty: bool,
    pub replay_required: bool,
    pub last_checkpoint_tx: Option<u64>,
    pub mounted_features: &'static [FsFeature],
}

impl MountedVolumeState {
    pub fn new(descriptor: FsMountDescriptor, geometry: BlockDeviceGeometry) -> Self {
        Self {
            mounted_features: descriptor.layout.capabilities.features,
            descriptor,
            geometry,
            journal: JournalReplayState::new(descriptor.journal_policy),
            mounted: false,
            dirty: false,
            replay_required: true,
            last_checkpoint_tx: None,
        }
    }

    pub fn mark_mounted(&mut self) {
        self.mounted = true;
        self.replay_required = false;
    }

    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    pub fn mark_checkpointed(&mut self, tx: u64) {
        self.last_checkpoint_tx = Some(tx);
        self.dirty = false;
    }
}
