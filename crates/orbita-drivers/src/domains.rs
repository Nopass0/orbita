use crate::{DeviceClass, DriverDescriptor, DriverMaturity};

pub const fn builtin_gpu_drivers() -> &'static [DriverDescriptor] {
    &[
        DriverDescriptor {
            name: "uefi-gop",
            class: DeviceClass::Gpu,
            backend: "framebuffer",
            maturity: DriverMaturity::Bootstrap,
            notes: "Current native GOP framebuffer boot path.",
        },
        DriverDescriptor {
            name: "virtio-gpu",
            class: DeviceClass::Gpu,
            backend: "virtio",
            maturity: DriverMaturity::Contract,
            notes: "Planned accelerated virtual GPU backend.",
        },
        DriverDescriptor {
            name: "qxl",
            class: DeviceClass::Gpu,
            backend: "pci",
            maturity: DriverMaturity::Contract,
            notes: "Virtual display backend for richer QEMU graphics.",
        },
        DriverDescriptor {
            name: "vulkan-render",
            class: DeviceClass::Gpu,
            backend: "userspace-api",
            maturity: DriverMaturity::Experimental,
            notes: "Compatibility compositor with Vulkan-style swapchain and present semantics.",
        },
    ]
}

pub const fn builtin_input_drivers() -> &'static [DriverDescriptor] {
    &[
        DriverDescriptor {
            name: "ps2-keyboard",
            class: DeviceClass::Input,
            backend: "i8042",
            maturity: DriverMaturity::Contract,
            notes: "Classic keyboard controller path.",
        },
        DriverDescriptor {
            name: "ps2-mouse",
            class: DeviceClass::Input,
            backend: "i8042",
            maturity: DriverMaturity::Contract,
            notes: "Classic relative mouse path.",
        },
        DriverDescriptor {
            name: "usb-hid",
            class: DeviceClass::Input,
            backend: "usb",
            maturity: DriverMaturity::Contract,
            notes: "Keyboard, mouse, and generic HID devices.",
        },
    ]
}

pub const fn builtin_net_drivers() -> &'static [DriverDescriptor] {
    &[
        DriverDescriptor {
            name: "virtio-net",
            class: DeviceClass::Net,
            backend: "virtio",
            maturity: DriverMaturity::Contract,
            notes: "Primary virtual NIC target for QEMU guests.",
        },
        DriverDescriptor {
            name: "e1000",
            class: DeviceClass::Net,
            backend: "pci",
            maturity: DriverMaturity::Contract,
            notes: "Common Intel emulated adapter.",
        },
        DriverDescriptor {
            name: "rtl8139",
            class: DeviceClass::Net,
            backend: "pci",
            maturity: DriverMaturity::Contract,
            notes: "Fallback legacy emulated NIC.",
        },
    ]
}

pub const fn builtin_sound_drivers() -> &'static [DriverDescriptor] {
    &[
        DriverDescriptor {
            name: "hda",
            class: DeviceClass::Sound,
            backend: "pci",
            maturity: DriverMaturity::Contract,
            notes: "Intel High Definition Audio path.",
        },
        DriverDescriptor {
            name: "ac97",
            class: DeviceClass::Sound,
            backend: "pci",
            maturity: DriverMaturity::Contract,
            notes: "Legacy virtual audio backend.",
        },
    ]
}

pub const fn builtin_storage_drivers() -> &'static [DriverDescriptor] {
    &[
        DriverDescriptor {
            name: "ahci",
            class: DeviceClass::Storage,
            backend: "pci",
            maturity: DriverMaturity::Contract,
            notes: "SATA controller support path.",
        },
        DriverDescriptor {
            name: "nvme",
            class: DeviceClass::Storage,
            backend: "pci",
            maturity: DriverMaturity::Contract,
            notes: "High-performance block storage path.",
        },
        DriverDescriptor {
            name: "virtio-blk",
            class: DeviceClass::Storage,
            backend: "virtio",
            maturity: DriverMaturity::Contract,
            notes: "Virtual machine optimized block device path.",
        },
    ]
}

pub const fn builtin_block_drivers() -> &'static [DriverDescriptor] {
    &[
        DriverDescriptor {
            name: "ahci",
            class: DeviceClass::Storage,
            backend: "pci/ahci",
            maturity: DriverMaturity::Contract,
            notes: "Matches PCI SATA controllers that expose AHCI programming interface.",
        },
        DriverDescriptor {
            name: "nvme",
            class: DeviceClass::Storage,
            backend: "pci/nvme",
            maturity: DriverMaturity::Contract,
            notes: "Matches PCI NVMe controllers with MSI or MSI-X support.",
        },
        DriverDescriptor {
            name: "virtio-blk",
            class: DeviceClass::Storage,
            backend: "virtio/modern",
            maturity: DriverMaturity::Contract,
            notes: "Matches virtio PCI storage transports using modern or legacy access.",
        },
        DriverDescriptor {
            name: "usb-mass-storage",
            class: DeviceClass::Storage,
            backend: "usb/bulk-only",
            maturity: DriverMaturity::Contract,
            notes: "Matches USB mass-storage devices exposed through the transport layer.",
        },
    ]
}
