# Orbita OS

Orbita — модульная `no_std` операционная система на Rust для `x86_64`
(UEFI), построенная вокруг четырёх принципов:

1. **Максимальная скорость.** Всё написано in-tree — от аллокатора до
   сетевого стека — чтобы ничто generic не стояло между железом и задачей.
   Dirty-rect рендер, DMA-пути, O(1) горячие участки.
2. **Комфорт уровня Windows.** Рендерящийся рабочий стол, мгновенный
   отклик ввода, связный визуал — из коробки.
3. **Модульность уровня Linux.** Малое ядро, вокруг — заменяемые модули:
   драйверы через единый контракт, графические бэкенды через реестр,
   приложения как пакеты.
4. **Широкая поддержка железа.** Цель — все современные x86_64 CPU;
   минимальный билд должен грузиться и с крошечного ESP.

## Что уже работает (проверено в QEMU)

- **Нативные Rust-приложения** — полный конвейер:
  ```
  apps/examples/hello  →  cargo (x86_64-unknown-none, rust-lld)
                      →  hello.orbpkg (ORBEXEC-контейнер)
                      →  FAT16-образ (target/orbita-pkg.img)
                      →  QEMU диск → ядро монтирует /pkg (свой FAT-драйвер)
                      →  pkg install hello → run hello
                      →  ELF-лоадер → orb_main(&ABI) → [app] …  →  exit=0
  ```
  Приложение пишет на «stdout» (терминал+serial), читает/пишет файлы
  (`sys::fs`), видит сеть и ОС (`sys::net`, `sys::os`, `sys::time`) —
  всё через версионированную C-ABI таблицу `orbita-abi`.
  Демо `hello` и `sysinfo` запускаются автоматически на каждом буте.
- **Сеть** — e1000 драйвер (MMIO RX/TX кольца) + живой стек:
  ARP, ICMP (машина отвечает на ping и сама пингует `ping 10.0.2.2`),
  IPv4/UDP/TCP parse/build, netcfg со счётчиками трафика.
- **Драйверная платформа** — `trait Driver` (probe→attach→start),
  динамический `DriverManager`; реальные драйверы: AHCI (2 диска),
  PS/2, e1000.
- **Графика** — двойная буферизация + dirty-rect; `PresentBackend`
  трейт + реестр бэкендов (`gfx=vulkan` из `/etc/orbita.conf` подхватит
  будущий GPU-драйвер без изменений потребителей).
- **Файловая система** — OrbitaFS (свои inode/extents/bitmap), живой
  конфиг, FAT12/16/32 read-only для канала доставки пакетов.
- **Процессы** — ORBEXEC-формат, pid/fds, `ps`; нативный exec на
  выделенном стеке (v1: ring0, identity-mapped — см. roadmap).

## Сборка и запуск (dev_manager)

Всё через [dm](https://github.com/Nopass0/dev_manager) (`dm.yaml`):

```bash
dm build        # apps → kernel → ESP → firmware
dm start        # QEMU с hot-reload (правка → ребут)
dm alias run    # полный rebuild + запуск
```

Стадии: `0. apps` (orbita-build пакует приложения в FAT-образ доставки),
`1. kernel` (x86_64-unknown-uefi), `2. esp` (+pkg-образ), `3. firmware`.

Алиасы: `pkgbuild` (собрать приложения+образ), `appnew <name>` (скаффолд
нового приложения), `test` (host-тесты), `doc` (rust-doc), `os`, `doctor`.

### Написать своё приложение

```bash
dm alias appnew myapp          # создаст apps/myapp
# редактируешь apps/myapp/src/main.rs:
#   orbita_sdk::entry! { fn main() -> i32 { println!("hi"); 0 } }
dm alias pkgbuild              # соберёт и положит в /pkg
dm start                       # в ОС: pkg install myapp; run myapp
```

### Изнутри ОС (терминал)

```
pkg list | install <name> | remove <name> | info <name>
run <app> [args]        ps              # нативные приложения
ping 10.0.2.2           netcfg          # живая сеть
ls cat write mkdir rm mv cp df …        # файлы (+пайпы, редиректы, env)
```

## Архитектура (23 крейта)

| Крейт | Роль |
|---|---|
| `orbita-kernel` | UEFI-точка входа, boot, модули console/config/disk/drivers/ui/input/abi/hosts |
| `orbita-abi` | версионированная C-ABI таблица сервисов (sysv64) |
| `orbita-sdk` | публичное API приложений: `entry!`, `println!`, `sys::{fs,net,os,time}` |
| `orbita-build` | хост-инструмент: сборка приложений, ORBEXEC, FAT-образ доставки |
| `orbita-drivers` | драйверная платформа (`Driver`, `DriverManager`), PCI-классификация |
| `orbita-hw` | AHCI, e1000, PCI, APIC/IOAPIC, PS/2, SMP-инвентаризация |
| `orbita-net` | Ethernet/ARP/IPv4/ICMP/UDP/TCP + NetworkStack, Wi-Fi/BT модели |
| `orbita-fs` | OrbitaFS, MemoryVolume, FAT reader + FAT writer (доставка) |
| `orbita-mm` | frame allocator, kernel heap, `vm` (RegionMap/mmap-контракт, shm) |
| `orbita-video` | PresentBackend-трейт + реестр, композитор, шрифты, 2D |
| `orbita-desktop` | рендер рабочего стола (сцены, dirty-rect scopes) |
| `orbita-process` | ORBEXEC, ProcessEngine (pid, fds) |
| `orbita-shell` | парсер (пайпы/редиректы/env) + runtime команд, pkg |
| `orbita-runtime/async/scheduler/threading/sync` | исполнение: cooperative executor, policy, потоки, примитивы |
| `orbita-core/platform/proto/std/arch-x86_64` | общее состояние, serial, boot-протокол, std-фасад, asm |

## Тесты и документация

```bash
dm alias test    # cargo test --workspace --exclude orbita-kernel (~75 тестов)
dm alias doc     # rust-doc всего workspace
```

Host-тестами покрыты: сетевой стек (25), OrbitaFS+FAT раунд-трип (12),
аллокатор и vm (8), SDK/драйверы/видео/процессы.

## Документация

- `docs/architecture.md` — слои и правила границ
- `docs/roadmap.md` — мастер-план: paging/user-mode, SMP, virtio-gpu→Vulkan,
  полный TCP/сокеты, HTML/CSS+Rust UI-платформа, производительность, безопасность
- `docs/drivers.md` — как писать драйверы
- `docs/graphics.md` — как подменить графический бэкенд
- `docs/abi-and-apps.md` — ABI, приложения, пакеты

## Известные ограничения v1

Приложения исполняются в ring0 (identity-mapped, отдельный стек,
SysV-мост); паника приложения останавливает систему; AP-ядра не
поднимаются (только инвентаризация); TCP без state machine; USB/аудио
отсутствуют. Всё это — этапы `docs/roadmap.md`.
