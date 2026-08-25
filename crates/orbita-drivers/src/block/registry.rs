use crate::block::candidate::{
    BlockCandidate, BlockCandidateMatch, BlockPciClassCode, BlockPciSnapshot, BlockTransportKind,
};

const AHCI_CLASS: BlockPciClassCode = BlockPciClassCode {
    class: 0x01,
    subclass: 0x06,
    programming_interface: Some(0x01),
};

const NVME_CLASS: BlockPciClassCode = BlockPciClassCode {
    class: 0x01,
    subclass: 0x08,
    programming_interface: Some(0x02),
};

const USB_MASS_STORAGE_CLASS: BlockPciClassCode = BlockPciClassCode {
    class: 0x0C,
    subclass: 0x03,
    programming_interface: Some(0x00),
};

const PCI_STORAGE_CANDIDATES: [BlockCandidate; 4] = [
    BlockCandidate {
        name: "ahci",
        transport: BlockTransportKind::Ahci,
        vendor_id: None,
        device_ids: &[],
        class_codes: &[AHCI_CLASS],
        requires_msi: false,
        prefers_msix: false,
        notes: "PCI SATA controller exposing AHCI programming interface.",
    },
    BlockCandidate {
        name: "nvme",
        transport: BlockTransportKind::Nvme,
        vendor_id: None,
        device_ids: &[],
        class_codes: &[NVME_CLASS],
        requires_msi: true,
        prefers_msix: true,
        notes: "PCI NVMe controller with MSI or MSI-X support.",
    },
    BlockCandidate {
        name: "virtio-blk",
        transport: BlockTransportKind::VirtioBlk,
        vendor_id: Some(0x1AF4),
        device_ids: &[0x1001, 0x1042],
        class_codes: &[],
        requires_msi: false,
        prefers_msix: true,
        notes: "VirtIO block device exposed through legacy or modern PCI transport.",
    },
    BlockCandidate {
        name: "usb-mass-storage",
        transport: BlockTransportKind::UsbMassStorage,
        vendor_id: None,
        device_ids: &[],
        class_codes: &[USB_MASS_STORAGE_CLASS],
        requires_msi: false,
        prefers_msix: false,
        notes: "USB bulk-only mass storage device.",
    },
];

#[derive(Debug, Copy, Clone)]
pub struct BlockTransportSummary {
    pub candidate_count: usize,
    pub matched_count: usize,
}

#[derive(Debug, Copy, Clone)]
pub struct AttachedBlockDevice {
    pub backend_name: &'static str,
    pub transport: BlockTransportKind,
    pub uses_msi: bool,
    pub uses_msix: bool,
}

pub struct BlockTransportRegistry;

impl BlockTransportRegistry {
    pub const fn new() -> Self {
        Self
    }

    pub fn candidates(&self) -> &'static [BlockCandidate] {
        &PCI_STORAGE_CANDIDATES
    }

    pub fn match_device(&self, snapshot: &BlockPciSnapshot) -> Option<BlockCandidateMatch> {
        self.candidates()
            .iter()
            .filter_map(|candidate| candidate.matches(snapshot))
            .max_by_key(|candidate| candidate.score)
    }

    pub fn summary(&self) -> BlockTransportSummary {
        BlockTransportSummary {
            candidate_count: self.candidates().len(),
            matched_count: 0,
        }
    }

    pub fn attach_device(&self, snapshot: &BlockPciSnapshot) -> Option<AttachedBlockDevice> {
        let matched = self.match_device(snapshot)?;
        Some(AttachedBlockDevice {
            backend_name: matched.name,
            transport: matched.transport,
            uses_msi: snapshot.msi_capable,
            uses_msix: snapshot.msix_capable,
        })
    }
}
