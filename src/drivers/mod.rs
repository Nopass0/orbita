#[path = "../../drivers/pci.rs"]
pub mod pci;

pub mod sound {
    #[path = "../../../drivers/sound/ac97.rs"]
    pub mod ac97;

    #[path = "../../../drivers/sound/hda.rs"]
    pub mod hda;
}
