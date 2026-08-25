# Этап A — Пейджинг и user-mode (ring 3) 🔄

**Даты:** начат 2026-08-25
**Статус:** в работе (первая порция: страничные таблицы — см. лог).
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

---

*(шаблон порции: дата → Сделано/Тесты/Дальше; статусы: ⬜ planned,
🔄 in progress, ✅ done, ⚠️ blocked)*
