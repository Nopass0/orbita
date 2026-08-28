# Этап A — Пейджинг и user-mode (ring 3) 🔄

**Даты:** начат 2026-08-25
**Статус:** в работе (порции 1–5: таблицы, hugepages, dry-run, CR3-переключение
стабильно; дальше — kernel-half и ring 3).
**Цель этапа:** реальные page tables, изоляция приложений, syscalls.

## Цель из roadmap (кратко)

1. Менеджер страничных таблиц (PML4→PDPT→PD→PT) в `orbita-mm`.
2. Свой CR3 на процесс; kernel-half/user-half.
3. Загрузка ELF в user-адреса; user-стек; TSS/RSP0.
4. Syscall-шлюз (syscall/sysret), перехват паник приложений.
5. `run` исполняет приложение в ring3; краш приложения ≠ крах ОС.

## Лог прогресса

### 2026-08-25 — порция 1: модуль пейджинга (таблицы + тесты) 🔄

**Сделано:**
- `crates/orbita-mm/src/paging.rs` — модель 4-уровневых таблиц
  x86_64 над абстрактной «физической памятью» (`FrameAllocator`
  трейт + `Phys`/`Virt` newtype):
  - записи с флагами PRESENT/WRITE/USER/NOEXEC (битовые);
  - `PageTableMapper::map_page(virt, phys, flags)` — прохождение
    PML4→PDPT→PD→PT с ленивым созданием промежуточных таблиц;
  - `translate(virt) -> Option<(Phys, Flags)>`;
  - `unmap_page(virt)`;
  - ошибки (`MapError::{AlreadyMapped, FrameExhausted, Unmapped}`).
- Хост-тесты (7): мап/трансляция раунд-трип, ремап-ошибка, анмап,
  ленивые таблицы создаются по пути, флаги сохраняются, границы
  512-записей, translate несмапленного = None.
- Интеграция с `vm::RegionMap` — план следующей порции (таблицы
  строятся ИЗ регионов).

**Тесты:** +7 host (`cargo test -p orbita-mm` → 15 passed, 0 failed).

**Дальше (порция 2):**
- `RamFrameAllocator` поверх `BootstrapFrameAllocator` (ядро);
- identity-мап всей памяти в новый CR3 + переключение после
  ExitBootServices (осторожно: firmware-таблицы, GOP, APIC MMIO);
- kernel-half (0xFFFF8000_00000000+) для ядра, user — низ;
- hugepages (2MiB) для кучи ядра.


### 2026-08-25 — порция 2: hugepages 2 MiB + identity-маппер 🔄

**Сделано:**
- `orbita-mm/src/paging.rs`:
  - константы `HUGE` (PS-бит), `ADDR_MASK`, `HUGE_ADDR_MASK`,
    `PAGE_SIZE_2M`;
  - `map_2mib(virt, phys, flags)` — huge-запись в PD (без аллокации PT),
    проверка 2 MiB-выравнивания;
  - `translate`/`unmap_page` понимают huge-записи (level PD);
  - `map_identity_2mib(start, end, flags)` — идентичное отображение
    региона огромными страницами, пропуск уже замапленного
    (для инкрементальной пересборки firmware-карты).
- CI-фиксы (тот же день): QEMU smoke — путь pkg-образа (`mv` в
  target/), перебор вариантов OVMF (4M/обычный), guard пустого
  smoke.log; ORBEXEC-тест стал самодостаточным (без артефакта сборки);
  убран дубль `#[test]` в diskfs.

**Тесты:** mm 15→19 (huge мап/транс/анмап, невыравнивание, конфликт
huge↔4K, identity+skip-existing). CI: host-tests снова зелёные.

**Дальше (порция 3):**
- `KernelFrameMemory` — реализация `FrameMemory` поверх реального
  `BootstrapFrameAllocator` ядра (физические фреймы таблиц);
- построение полной identity-карты (0..RAM top + MMIO) в новом PML4;
- переключение CR3 после ExitBootServices + smoke (критическая точка —
  задействовать QEMU-маркеры; откат по бэкап-ветке при triple fault).

### 2026-08-25 — порция 3: KernelFrameMemory + dry-run identity-мап 🔄

**Сделано:**
- `orbita-kernel/src/paging_setup.rs`:
  - `KernelFrameMemory` — `FrameMemory` поверх `BootstrapFrameAllocator`
    (identity-доступ к фреймам таблиц, контракт задокументирован);
  - `dry_run_identity_map` — строит PML4 + identity 0..usable-top (≤1GiB)
    в 2 MiB huge-страницах, спот-проверка translate середины карты,
    **БЕЗ переключения CR3**;
  - `maybe_run_dry_run` — гейт по `/etc/orbita.conf`
    `paging_dry_run=on` (включён в default-конфиг).
- Интеграция в `kernel_main` после применения живого конфига.

**Тесты:** QEMU smoke — `paging dry-run ok: pml4=0x8000 huge_pages=256
span=512 MiB`; boots=1, hello/sysinfo exit=0, panic=0 (ничего не сломано).
Host: workspace 84 passed / 0 failed.

### 2026-08-25 — порция 4: переключение CR3 (реализовано, ВЫКЛ по умолчанию) 🔄

**Сделано:**
- `paging_setup.rs`:
  - `build_full_address_space` — identity-мап ВСЕХ дескрипторов boot map
    (RAM/reserved/ACPI/MMIO, округление границ до 2 MiB) + явные extra
    (GOP-фреймбуфер, LAPIC, IOAPIC);
  - `switch_cr3` (naked-asm `mov cr3`), `maybe_switch_cr3` с гейтом
    `paging_cr3=on|off` в orbita.conf.

**QEMU факты:** переключение выполняется (`paging: cr3 switched to
0xb000`), ОС продолжает часть бута — но крашится дальше. Причина:
покрытие MMIO неполное/некорректное (нужно пройти полный firmware map
с правками атрибутов и точными границами). **Вывод: код готов,
выключен по умолчанию** (`paging_cr3=off`) — ОС работает на таблицах
прошивки, пока порция 5 не закроет карту.

**Тесты:** boots=1, автораны ок (switch off). Со switch — краш после
`config from disk` (см. выше), ничего не маскируем.

**Дальше (порция 5):** полная карта атрибутов дескрипторов
(RX/RW/NOEXEC по типам), ACPI/HPET/PCI-ECAM добавки, последовательная
включаемость + QEMU-перебор, затем user-half и ring 3.

### 2026-08-28 — порция 5: CR3-переключение стабильно (вкл. по умолчанию) ✅

**Диагноз краша порции 4 (два бага):**
1. **Грязные фреймы таблиц.** `BootstrapFrameAllocator::allocate_frame`
   не обнуляет фреймы, а контракт `FrameMemory::alloc_frame` требует
   zeroed. Прохождение PML4→PDPT→PD по мусорным «present»-записям
   писало листья в случайные физические фреймы. Симптомы: на warm-boot
   dry-run рапортовал `huge_pages=0` (таблицы прошлого бута!), MMIO-запись
   в устройство уходила в RAM (AHCI-таймауты `is=0 ci=0 tfd=0`).
2. **Дыры между дескрипторами карты.** Фирменный memory-map не описывает
   весь 32-битный PCI-hole (ECAM/BAR'ы AHCI+e1000 живут в промежутках) —
   первое обращение драйвера после switch = #PF → (без обработчика)
   triple fault → молчаливый ребут.

**Сделано:**
- `orbita-arch-x86_64/src/lib.rs` (cpu):
  - asm-стабы `double_fault/general_protection/page_fault` (Win64: rcx/rdx,
    shadow 32) + `FaultFrame` (err/rip/cs/rflags/rsp/ss);
  - `orbita_x86_64_on_cpu_fault` — печать в serial (#PF добавляет CR2),
    затем halt: fault теперь виден в логе, а не маскируется ресетом;
  - читалки `read_cr2/read_cr3/read_cr4` (диагностика пейджинга).
- `orbita-hw/src/irq.rs`: векторы 8/13/14 в IDT → стабы диагностики;
  `IdtInstallReport.fault_vectors`.
- `orbita-kernel/src/paging_setup.rs`:
  - `KernelFrameMemory::alloc_frame` обнуляет фрейм (контракт соблюдён);
  - `build_full_address_space`: **0..4 GiB целиком** (2048 huge pages —
    все MMIO-дыры закрыты по построению) + дескрипторы >4 GiB (high RAM,
    64-бит MMIO-окна, округление наружу до 2 MiB) + явные extra
    (GOP fb, LAPIC, IOAPIC); skip-existing для пересечений;
    отчёт `AddressSpaceReport` (pml4/huge/span) в serial.
- `config.rs`: `paging_cr3=on` — валидировано, включено по умолчанию.
- CI: маркер `paging: cr3 switched` в обязательные smoke-проверки.

**Тесты (QEMU, q35/512M/smp4/OVMF, свежий + warm диск):**
- cold boot: `cr3 switched to 0xb000 huge_pages=8192 span=16384 MiB`,
  boots=1, все 13 smoke-маркеров, panic=0, fault=0, AHCI-таймаутов=0;
  запись `/bin/orbita-shell.orbexec` на диск (ранее крашившая) проходит;
- warm boot (boots=2): dry-run снова `huge_pages=256` (обнуление
  работает), switch без fault — фреймы прошлого бута перезаписаны чисто.
- Host: workspace 84 passed / 0 failed; kernel build 0 warnings;
  rust-doc (`-D warnings`) чист.

**Дальше (порция 6):** ядро → hi-half (0xFFFF8000…): kernel-half
копируется в каждый новый PML4, identity-low остаётся на переход;
затем GDT/TSS (IST1-3, user-сегменты) и syscall-шлюз (roadmap A.4/A.5).

### 2026-08-28 — порция 6: GDT/TSS + syscall-шлюз + ring 3 self-test ✅

**Сделано:**
- `orbita-arch-x86_64/src/gdt.rs` (новый):
  - кодировщик дескрипторов `encode_segment` (чистая логика, 5 host-тестов,
    включая проверку аппаратных смещений TSS — тест поймал пропущенный
    u64-reserved: sizeof был 96 вместо 104!);
  - селекторы 0x08/0x10 (совместимы с bootstrap-IDT), 0x18/0x20/0x28
    (user32/data/code64, RPL3: 0x23/0x2B — STAR-конвенция sysret),
    0x30 (TSS64);
  - `TaskStateSegment` (rsp0 + IST1..3 на статик-стеках 16K/4K);
  - `install_kernel_gdt()`: lgdt + перезагрузка DS/ES/SS + CS через
    `retfq` + ltr. **Грабли:** `push {0:x}` пушит 2 байта, а `retfq`
    извлекает 16 — RSP уезжал на 6 байтов, стек каллера разрушался
    (мусор в последующих println). Нужен qword-push.
- `orbita-arch-x86_64/src/syscall.rs` (новый):
  - MSR STAR/LSTAR/FMASK + EFER.SCE (rmw — не затирает NXE-состояние);
  - asm-вход: сохранение user rsp/rcx/r11, переключение на отдельный
    kernel-стек (16 KiB .bss), перемешивание аргументов SysV→Win64
    (rdi/rsi → rdx/r8), вызов Rust-диспетчера, `sysretq`;
  - SYSCALL_DONE-путь: восстановление сохранённого kernel-rsp + `ret`
    (ring3-roundtrip возвращается в Rust-вызвавшего);
  - `enter_ring3(rip, rsp)`: iretq-трамплин (SS=0x23, CS=0x2B, IF=1);
  - v1-диспетчер: ECHO (0x1000) / DONE (0x1001) — SDK-миграция на
    syscall-номера следующей порцией.
- `orbita-kernel/src/paging_setup.rs`: `maybe_ring3_selftest` —
  ремап app-региона 0x10000000 на 4 KiB USER-страницы (unmap huge +
  256×map_page над ЖИВЫМ CR3), запись 25-байтового stub
  (mov rax,ECHO; mov rdi,magic; syscall; …DONE; syscall), вход в
  ring 3, `sti` после возврата; гейт `ring3_test=on` (default).
- `main.rs`: GDT сразу после bootstrap-IDT + строка в boot-лог.

**Тесты (QEMU q35/512M/smp4, 3 бута подряд):** все 16 smoke-маркеров ×3,
`ring3: syscall echo received` → `done syscall — resuming kernel context`
→ `ring3: roundtrip ok=true syscalls=2`, бут продолжается (orbitafs/
process/vfs), boots=1, panic=0, fault=0. Host: 89 passed / 0 failed
(+5 GDT). **Ring 3 + syscall/sysret доказаны в живой ОС** — вход iretq
CS=0x2B, возврат sysretq, ядро продолжает бут.

**Честно о граблях этой порции:** первый вариант фризился посреди
println после roundtrip (рандомный RIP в PCI-дыре, кодген-чувствительно:
ушло при выносе atomic-чтения из format_args + sti + доп. печати;
root-cause до конца не доказан — оставлено под наблюдением, #PF/#GP-
диагностика на страже). Intel-синтаксис LLVM: `;`-комментарии в
global_asm невалидны (нужен `#`), `o64`-префикс не принимается
(суффиксы `iretq`/`sysretq`/`retfq` работают).

**Дальше (порция 7):** SDK-миграция на syscall-номера (ABI v2),
ELF в user-адреса (база 0x400000, user-стек 0x7FFF…), `run` в ring3,
#PF в user → kill процесса (roadmap A.5/A.6/A.7).

---

*(шаблон порции: дата → Сделано/Тесты/Дальше; статусы: ⬜ planned,
🔄 in progress, ✅ done, ⚠️ blocked)*
