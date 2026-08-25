#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum DeviceClass {
    Bus,
    Gpu,
    Input,
    Net,
    Sound,
    Storage,
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum DriverMaturity {
    Contract,
    Bootstrap,
    Experimental,
    Stable,
}

#[derive(Debug, Copy, Clone)]
pub struct DriverDescriptor {
    pub name: &'static str,
    pub class: DeviceClass,
    pub backend: &'static str,
    pub maturity: DriverMaturity,
    pub notes: &'static str,
}
