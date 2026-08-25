use crate::{
    DriverDescriptor, builtin_block_drivers, builtin_gpu_drivers, builtin_input_drivers,
    builtin_net_drivers, builtin_sound_drivers, builtin_storage_drivers,
};

#[derive(Debug, Copy, Clone)]
pub struct RegistrySummary {
    pub block: usize,
    pub gpu: usize,
    pub input: usize,
    pub net: usize,
    pub sound: usize,
    pub storage: usize,
    pub total: usize,
}

pub struct DriverRegistry;

impl DriverRegistry {
    pub const fn new() -> Self {
        Self
    }

    pub fn summary(&self) -> RegistrySummary {
        let block = builtin_block_drivers().len();
        let gpu = builtin_gpu_drivers().len();
        let input = builtin_input_drivers().len();
        let net = builtin_net_drivers().len();
        let sound = builtin_sound_drivers().len();
        let storage = builtin_storage_drivers().len();
        RegistrySummary {
            block,
            gpu,
            input,
            net,
            sound,
            storage,
            total: block + gpu + input + net + sound + storage,
        }
    }

    pub fn all(&self) -> [&'static [DriverDescriptor]; 6] {
        [
            builtin_block_drivers(),
            builtin_gpu_drivers(),
            builtin_input_drivers(),
            builtin_net_drivers(),
            builtin_sound_drivers(),
            builtin_storage_drivers(),
        ]
    }
}
