#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum BlockTransportKind {
    Ahci,
    VirtioBlk,
    Nvme,
    UsbMassStorage,
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct BlockPciClassCode {
    pub class: u8,
    pub subclass: u8,
    pub programming_interface: Option<u8>,
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct BlockPciSnapshot {
    pub vendor_id: u16,
    pub device_id: u16,
    pub class_code: BlockPciClassCode,
    pub msi_capable: bool,
    pub msix_capable: bool,
}

#[derive(Debug, Copy, Clone)]
pub struct BlockCandidate {
    pub name: &'static str,
    pub transport: BlockTransportKind,
    pub vendor_id: Option<u16>,
    pub device_ids: &'static [u16],
    pub class_codes: &'static [BlockPciClassCode],
    pub requires_msi: bool,
    pub prefers_msix: bool,
    pub notes: &'static str,
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct BlockCandidateMatch {
    pub name: &'static str,
    pub transport: BlockTransportKind,
    pub score: u8,
}

impl BlockCandidate {
    pub fn matches(&self, snapshot: &BlockPciSnapshot) -> Option<BlockCandidateMatch> {
        if let Some(vendor_id) = self.vendor_id {
            if vendor_id != snapshot.vendor_id {
                return None;
            }
        }

        if !self.device_ids.is_empty() && !self.device_ids.contains(&snapshot.device_id) {
            return None;
        }

        if !self.class_codes.is_empty()
            && !self.class_codes.iter().any(|class_code| {
                class_code.class == snapshot.class_code.class
                    && class_code.subclass == snapshot.class_code.subclass
                    && match class_code.programming_interface {
                        Some(pi) => snapshot.class_code.programming_interface == Some(pi),
                        None => true,
                    }
            })
        {
            return None;
        }

        if self.requires_msi && !(snapshot.msi_capable || snapshot.msix_capable) {
            return None;
        }

        let mut score = 0;
        if self.vendor_id.is_some() {
            score += 3;
        }
        if !self.device_ids.is_empty() {
            score += 3;
        }
        if !self.class_codes.is_empty() {
            score += 2;
        }
        if self.requires_msi && snapshot.msi_capable {
            score += 1;
        }
        if self.prefers_msix && snapshot.msix_capable {
            score += 1;
        }

        Some(BlockCandidateMatch {
            name: self.name,
            transport: self.transport,
            score,
        })
    }
}
