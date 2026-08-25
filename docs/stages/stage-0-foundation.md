# Этап 0 — Фундамент ✅

**Даты:** 2026-08-24 … 2026-08-25
**Статус:** завершён. Всё ниже проверено в QEMU (headless smoke:
1 загрузка, 0 паник, все маркеры) и 72 host-тестами.

## Сделано

### Чистка и реструктуризация (2026-08-24)
- Удалены фейковые тулчейны shell (python/gcc/cargo-«симулянты»),
  `_p2.py`, крейт-сирота `orbita-bus`, фейковый `orbita_runtime::Executor`,
  дубли типов (CpuAffinity/ThreadPriority/…), ~700 строк мёртвой отрисовки.
- `orbita-kernel/src/main.rs` (3300 строк) разрезан на модули:
  `boot, console, config, disk, drivers, seed, ui, input, hosts, abi`.
- Оживлён богатый обработчик консоли (Tab/F1–F4/поиск/указатель),
  дублирующий простой удалён.
- ⚠️ Найден и исправлен **критический баг исходной ОС**:
  `OrbitaDiskFs::list_dir("/")` возвращал сам корень (parent==index) →
  бесконечная рекурсия `collect_disk_files` → stack overflow → triple
  fault **на каждой загрузке**. Фикс: фильтр самоссылки + guard
  `read_file` для директорий + `is_dir` API. Регрессионный тест добавлен.

### Драйверная платформа (2026-08-24)
- `orbita-drivers/src/driver.rs`: `Driver` (probe→attach→start→stop/irq),
  `DeviceProbe` (PCI+BARs / legacy), `DriverManager` (register, bind_all,
  assign_irq/dispatch_irq, by_name/downcast), `BindReport`. Тесты.
- Реальные драйверы: `ahci-storage` (порт 0 OrbitaFS + порт 1 pkg-диск),
  `ps2-keyboard` (legacy), `e1000`.

### Графика — подменяемый бэкенд (2026-08-24)
- `orbita-video/src/backend.rs`: `PresentBackend` (present_region
  dirty-rect), `SoftwareFramebuffer` (GOP, дефолт), реестр
  `register_backend(name, factory)`, `create_backend(name, scanout)`.
- `FrameCompositor` над `Box<dyn PresentBackend>`; выбор бэкенда —
  `/etc/orbita.conf` `gfx=<name>` (fallback software). Доки:
  `docs/graphics.md`.

### Сеть — живой стек + e1000 (2026-08-24)
- `orbita-hw/src/e1000.rs`: MMIO, EEPROM MAC, RX/TX-кольца (выравненные
  вручную буферы), poll-режим; тест-драйв в QEMU user-net.
- `NetworkStack`: `send_arp_request`, `send_icmp_echo_request`,
  `take_tx_frame`; ARP-автоответ, ICMP echo reply; главный цикл ядра
  качает RX/TX. `ping <ip>` и `netcfg` (живые счётчики) в shell.
  UDP-чексумма без 64КБ стека-буфера (багфикс).

### Нативные приложения — сквозной конвейер (2026-08-25) 🔑
- `orbita-abi`: sysv64 C-ABI таблица (stdout, fs×4, mem×2, time, os,
  net, report_exit), `AbiStatus`, v1.
- `orbita-sdk`: `entry!` (генерирует `orb_main`), `println!`,
  `sys::{fs,net,os,time,process}`, `#[global_allocator]` поверх ABI.
- `orbita-build` (host): `new|pack|pack-all` — cargo rustc (rust-lld,
  линкер-скрипт, база `0x1000_0000`, `-eorb_main`) → ORBEXEC-контейнер →
  FAT16-образ `target/orbita-pkg.img` (`orbita-fs::fat_writer`, LFN).
- Ядро: FAT12/16/32-ro драйвер (`orbita-fs::fat`) монтирует pkg-диск,
  стейджит `/pkg`, авто-инсталл+авторан `auto=1` бандлов; ELF64-лоадер;
  exec на выделенном стеке 256КБ с Win64↔SysV мостом (`call_with_stack`,
  сохранение rdi/rsi!), `report_exit` вместо rax.
- **Отлажено и задокументировано**: NX сброс (EFER.NXE) после
  ExitBootServices; резервирование app-региона ДО кучи; явный
  `-eorb_main` (rustc default entry ≠ orb_main); ABI-мост — грабли
  описаны в AGENTS.md.
- Приложения: `hello` (println+fs write/read — exit 0), `sysinfo`
  (os_info+time+net+list_dir — exit 0). Автозапуск на каждом буте.

### Память (2026-08-25)
- `orbita-mm::vm`: `RegionMap` (map/unmap_at/protect_at/find),
  `Protection{RW,RO,RX}`, `Backing::{Heap,Image,Shared}`,
  `SharedMemoryRegistry` (IPC). 3 host-теста.

### Инфраструктура (2026-08-25)
- dm: стадия `0. apps` + алиасы `pkgbuild/appnew/test/doc`.
- CI (GitHub Actions): host-тесты, kernel (uefi, -D warnings), apps
  (unknown-none + pack), docs (-D warnings), **QEMU smoke** с проверкой
  10 маркеров + boots==1 + паник-нет.
- Доки: README (честный), `docs/{drivers,graphics,abi-and-apps}.md`,
  architecture.md обновлён, rust-doc 0 предупреждений.
- `docs/roadmap.md` — мастер-план (622 строки), этапы A–K.

## Тесты на конец этапа

- 72 host-теста: net 25, fs 12, drivers 5, mm 8, std 11, video 6,
  process 5 — все зелёные.
- QEMU smoke: `boots=1`, `panic=0`, 12/12 маркеров (см. AGENTS.md).

## Известные ограничения (унаследованы в этап A)

- Приложения ring0/identity; паника app останавливает ядро.
- AP не поднимаются (только инвентаризация).
- TCP без state machine; Wi-Fi/BT — только модели.
