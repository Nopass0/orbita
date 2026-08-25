# Orbita OS Architecture

## Design goals

Orbita OS is structured to support:

- `no_std` execution from the earliest boot stage
- a clean separation between boot code, kernel core, memory management, graphics, and platform drivers
- native framebuffer graphics from real video modes, including UEFI-provided framebuffers
- future hardware driver integration without leaking device assumptions into the kernel core
- a custom `std-like` layer named `std-orbita` that offers ergonomic APIs without depending on the host `std`

The key constraint is that each functional layer may depend only on the layers below it. `platform` is a backend layer:
it implements machine-specific details behind narrow interfaces and is not a place for arbitrary upward coupling.

## Layer model

### `boot`

The boot layer owns the initial transition into the OS.

Responsibilities:

- enter the machine from UEFI or another firmware path
- collect boot-time tables and memory maps
- select or preserve the graphics mode and framebuffer information
- build a compact boot handoff structure for the next layer

Rules:

- keep this layer as small as possible
- do not allocate heap memory here
- do not depend on kernel services
- do not expose firmware-specific types to higher layers

### `proto`

The proto layer is the first Rust-controlled runtime after boot.

Responsibilities:

- normalize boot data into internal types
- validate the framebuffer description and memory map
- expose a minimal panic/reporting path
- prepare control flow for the kernel core entry

Rules:

- no device policy
- no scheduler
- no filesystems
- no direct dependence on higher-level APIs

### `core`

The core layer contains the minimal kernel runtime.

Responsibilities:

- initialization sequencing
- logging and diagnostics
- interrupt-independent kernel services
- coordination between memory, video, and future subsystems

Rules:

- core must not know hardware details beyond abstracted contracts
- core should only talk to `mm`, `video`, and `platform` through stable interfaces

### `mm`

The memory layer owns virtual and physical memory policy.

Responsibilities:

- physical memory map representation
- page allocation and frame tracking
- virtual address abstractions
- heap initialization and allocator wiring
- future page table management

Rules:

- memory types must be explicit and strongly typed
- allocator logic must be separable from page-table logic
- the higher layers should request memory through interfaces, not direct globals

### `video`

The video layer owns the graphics contract.

Responsibilities:

- framebuffer discovery and validation
- pixel format normalization
- resolution and stride metadata
- drawing primitives for early boot graphics
- future driver-backed display routing

Rules:

- the core must not draw directly through raw firmware structures
- the video layer may provide a framebuffer path and later a device-backed path behind the same API
- drawing APIs should be safe at the call site and contain the necessary internal invariants

### `std-orbita`

`std-orbita` is the custom standard-library-like layer.

Responsibilities:

- ergonomic collections and utility types
- formatting and string handling
- result/error convenience wrappers
- common traits and adapters for kernel subsystems

Rules:

- no dependency on the host `std`
- may depend on `core`, `mm`, `video`, and `platform` abstractions
- must not introduce hidden platform assumptions

### `platform`

The platform layer groups architecture- and board-specific support.

Responsibilities:

- CPU/ISA abstractions
- chipset and firmware-specific shims
- future device driver bindings
- platform feature detection and capability reporting

Rules:

- platform code is the only layer allowed to touch machine-specific implementation details directly
- platform exports capabilities upward through narrow interfaces

Note:

- `platform` is a support backend, not a general-purpose feature layer
- higher layers consume platform capabilities through contracts instead of direct calls into implementation details

## Dependency policy

Allowed dependency direction for the functional stack:

```text
boot -> proto -> core -> mm -> video -> std-orbita -> platform
```

More practical interpretation:

- `boot` depends on the firmware environment only
- `proto` depends on `boot`
- `core` depends on `proto`
- `mm` depends on `core`
- `video` depends on `mm` and boot handoff data
- `std-orbita` depends on the lower runtime abstractions
- `platform` provides implementation details behind contracts, not arbitrary cross-layer access

Topology note:

- `platform` is intentionally modeled as a backend implementation layer, not as a policy layer that other layers freely depend on for behavior

## Memory model

Orbita OS should treat memory in three explicit forms:

- physical frames
- mapped kernel virtual memory
- user-facing or subsystem-owned heap allocations

The memory manager should expose:

- a page/frame descriptor type
- explicit ownership and lifetime boundaries
- a kernel heap initializer
- a future extension point for DMA-safe or device-specific memory pools

The goal is that no subsystem guesses how memory works. Every allocation request should go through the memory API.

## Graphics model

Early graphics should start with the framebuffer supplied by firmware or bootloader.

Requirements:

- preserve the selected native mode where possible
- render directly to the framebuffer in pixel coordinates
- support format-aware writes rather than assuming a hardcoded layout
- keep the drawing API compatible with future GPU or display-driver backends

The initial target is a simple but correct framebuffer path:

- validate geometry, pitch, and pixel format
- expose a pixel writer
- provide primitive fill and text-friendly blitting hooks later

## `std-orbita` intent

`std-orbita` is not a copy of Rust `std`.

It should provide:

- stable OS-facing convenience APIs
- thin wrappers around kernel services
- types that reduce boilerplate for internal kernel code

It should not provide:

- process spawning logic before the process model exists
- host I/O assumptions
- APIs that bypass kernel ownership rules

## Expansion strategy

New subsystems should be added as separate modules or manifests, not by widening existing interfaces.

Recommended sequence:

1. solidify boot handoff data
2. finish physical and virtual memory primitives
3. establish framebuffer output
4. add logging, panic, and diagnostics
5. introduce driver registration through `platform`
6. grow `std-orbita` from internal helpers into a reusable OS API layer


---

# Обновление архитектуры (2026-08)

## Драйверная платформа

`orbita-drivers::driver` — контракт `Driver` (probe→attach→start→stop/irq)
и `DriverManager` (динамическая регистрация, `bind_all`, IRQ-таблица,
downcast по имени). Ядро больше не инициализирует железо inline:
`kernel/src/drivers.rs` регистрирует `ahci-storage` (порт 0 — OrbitaFS,
порт 1 — pkg-диск), `ps2-keyboard`, `e1000`; пайплайн привязки печатает
отчёт. См. `docs/drivers.md`.

## Графика с подменяемым бэкендом

`orbita-video::backend` — `PresentBackend` (present_region dirty-rect) +
`SoftwareFramebuffer` (дефолт, GOP) + глобальный реестр
`register_backend(name, factory)`; выбор бэкенда — `/etc/orbita.conf`
`gfx=<name>` (fallback — software). `FrameCompositor` держит
`Box<dyn PresentBackend>`. См. `docs/graphics.md`.

## ABI и нативные приложения

`orbita-abi` — версионированная sysv64 C-ABI таблица сервисов;
`orbita-sdk` — публичный API приложений (`entry!`, `println!`,
`sys::{fs,net,os,time}`); `orbita-build` — хост-сборка+упаковка
(ELF x86_64-unknown-none → ORBEXEC → FAT16-образ); ядро монтирует
/pkg (свой FAT-драйвер ro), `pkg install`/`run` ставит и исполняет
приложения (ELF-лоадер, отдельный стек, Win64↔SysV-мост).
См. `docs/abi-and-apps.md`.

## Сеть

`orbita-net` — чистые протоколы + живой `NetworkStack`; e1000-драйвер
кормит стек кадрами (poll), `pending_tx` уходит обратно в NIC;
`ping`/`netcfg` в shell. Wi-Fi/BT — контракты данных (транспорты —
roadmap).

## Память

`orbita-mm` — фреймы, kernel heap, и новый `vm`-модуль: `RegionMap`
(map/unmap/protect, `Protection`, `Backing::{Heap,Image,Shared}`) и
`SharedMemoryRegistry` — контракт mmap/IPC до появления аппаратного
пейджинга (roadmap: user-mode).

## Известные ограничения v1

ring0-приложения (identity), паника app останавливает ядро, AP не
поднимаются, TCP без state machine, USB/аудио нет — всё в
`docs/roadmap.md`.
