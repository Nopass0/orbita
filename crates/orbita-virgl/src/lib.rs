//! # Orbita virgl — интерфейс на реальной видеокарте внутри QEMU
//!
//! QEMU с устройством `virtio-vga-gl` (Windows/Linux) отдаёт гостю
//! паравиртуальный GPU: команды Gallium из гостя исполняет virglrenderer
//! на настоящей видеокарте хоста. Крейт содержит всё, что для этого нужно
//! на стороне ОС, и не зависит от железа:
//!
//! * [`encoder`] — кодировщик потока команд virgl;
//! * [`shaders`] — TGSI-шейдеры дизайн-системы (SDF, градиенты, тени, стекло);
//! * [`renderer`] — превращение кадра `orbita-ui` в вершины и команды.
//!
//! Связь с устройством — трейт [`renderer::GpuDevice`]; в ядре его реализует
//! драйвер `orbita_hw::virtio_gpu::VirtioGpu`, в тестах — запись команд.

#![no_std]
#![warn(missing_docs)]

pub mod encoder;
pub mod renderer;
pub mod shaders;

pub use renderer::{GpuDevice, GpuRenderer, GpuStats, ResourceDesc};
