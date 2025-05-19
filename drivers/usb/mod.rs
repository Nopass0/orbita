pub mod uhci;
pub mod ohci;
pub mod ehci;
pub mod xhci;
pub mod mass_storage;
pub mod hid;

/// Common USB error type
#[derive(Debug, Clone, Copy)]
pub enum UsbError {
    ControllerNotFound,
    InitializationFailed,
    TransferError,
}
