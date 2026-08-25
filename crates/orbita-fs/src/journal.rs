use crate::{BlockAddress, InodeId, InodeMetadata};

/// Unique transaction identifier.
#[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct TransactionId(pub u64);

/// Journal record type.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum JournalKind {
    Begin,
    WriteMetadata,
    WriteData,
    Commit,
    Abort,
    Checkpoint,
}

/// A single journal entry.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct JournalEntry {
    pub tx: TransactionId,
    pub kind: JournalKind,
    pub target_inode: Option<InodeId>,
    pub target_block: Option<BlockAddress>,
    pub metadata: Option<InodeMetadata>,
    pub payload_crc32: u32,
}

/// Commit record written once a transaction is durable.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct JournalCommit {
    pub tx: TransactionId,
    pub checksum: u32,
}

/// Journal policy used by the filesystem.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum JournalPolicy {
    WriteAhead,
    CopyOnWrite,
    Hybrid,
}

/// High-level replay action yielded by a journal backend.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum JournalAction {
    ApplyMetadata,
    ApplyData,
    Commit,
    Abort,
    Checkpoint,
}

/// Progress snapshot for a replay run.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct JournalReplayProgress {
    pub processed: u64,
    pub committed: u64,
    pub checkpointed: bool,
}

/// Errors returned by a journal replay backend.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum JournalReplayError {
    CorruptedEntry,
    MissingCheckpoint,
    Unsupported,
}

/// Replay state tracked in memory during mount.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct JournalReplayState {
    pub policy: JournalPolicy,
    pub last_tx: Option<TransactionId>,
    pub dirty: bool,
    pub replay_required: bool,
}

impl JournalReplayState {
    pub fn new(policy: JournalPolicy) -> Self {
        Self {
            policy,
            last_tx: None,
            dirty: false,
            replay_required: true,
        }
    }
}

/// Contract implemented by a backend that can replay durable journal records.
pub trait JournalReplay {
    fn replay_entry(&mut self, entry: JournalEntry) -> Result<JournalAction, JournalReplayError>;

    fn finish(&mut self) -> Result<JournalReplayProgress, JournalReplayError>;
}
