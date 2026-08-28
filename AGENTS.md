# AGENTS.md — инструкция для ИИ-агентов, работающих с Orbita OS

Этот документ — контракт для любых агентов (или людей), вносящих изменения
в этот репозиторий. Прочитай целиком перед первой правкой.

## Что это за проект

Orbita OS — модульная `no_std` ОС на Rust для x86_64 (UEFI). 23 крейта в
`crates/`, приложения в `apps/`, сборка через `dm` (dev_manager).
Мастер-план: `docs/roadmap.md` (этапы A–K). **Статус этапов ведётся в
`docs/stages/`** — см. «Ведение прогресса» ниже.

## Золотые правила

1. **Ничего не ломать.** Каждое изменение заканчивается зелёной
   верификацией (см. «Верификация»). Красное — откат или фикс, а не пуш.
2. **Тесты обязательны.** Новая логика = новые `#[cfg(test)]` host-тесты
   в том же крейте (чистая логика — парсеры, контейнеры, форматы — всегда
   тестируется на хосте). Железный код покрывается контракт-тестами на
   состояниях/чистых частях. Цель — не снижать долю протестированного.
3. **Ведение прогресса по этапам.** У каждого этапа roadmap есть файл
   `docs/stages/stage-<буква>-<слаг>.md`. Перед началом работы на этапе —
   создай/открой файл; после каждой содержательной порции — допиши:
   дата, что сделано (файлы/функции), тесты (сколько/какие), статус
   (✅/🔄/⬜), что дальше. Формат — см. существующие файлы в `docs/stages/`.
4. **Диаграммы.** В документации используем mermaid-блоки (GitHub
   рендерит): `flowchart`, `sequenceDiagram`, `stateDiagram-v2`. Новая
   подсистема или неочевидный поток → диаграмма в соответствующем
   `docs/*.md`.
5. **Документация и rust-doc.** Новые публичные API снабжаются `///`/`//!`
   (0 предупреждений: `dm alias doc`). Для крупных фич — раздел в docs/.
6. **Бэкап при больших изменениях.** Перед массовыми удалениями/перестройкой
   — `zip`-бэкап исходников (без `target/`), удалить после зелёной
   верификации. В git-эпоху — достаточно ветки, но для одношаговых
   конвейеров правило остаётся.
7. **Честность.** В логах/доках/коммитах фиксируй реальные результаты:
   «не реализовано», «падает», «пропущено» — лучше, чем умолчание.

## Команды (все через dm)

| Команда | Что делает |
|---|---|
| `dm build` | полный конвейер: apps → kernel (uefi) → ESP → firmware |
| `dm start` | QEMU с hot-reload (правка → ребут) |
| `dm alias run` | rebuild + запуск |
| `dm alias test` | host-тесты workspace (`--exclude orbita-kernel`) |
| `dm alias doc` | rust-doc workspace |
| `dm alias pkgbuild` | собрать приложения + образ доставки |
| `dm alias appnew <name>` | скаффолд приложения |

Ручная сборка (CI/агенты без dm):
```bash
cargo test --workspace --exclude orbita-kernel
cargo build --release -p orbita-kernel --target x86_64-unknown-uefi
cargo check -p orbita-sdk --target x86_64-unknown-none
cargo run --release -p orbita-build -- pack-all
cargo doc --no-deps --workspace --exclude orbita-kernel
```

## Smoke-тест ОС в QEMU (headless, критерий готовности)

Маркеры в serial-логе (все должны присутствовать, `boots == 1`, `panic == 0`):
```
UEFI entry reached | drivers registered=3 bound=3 | e1000 up |
esp fat mounted | pkg delivery staged 2 | [app] hello from a native rust app |
[app] read back: | == orbita sysinfo == | autorun hello exit=0 |
autorun sysinfo exit=0 | paging: cr3 switched | gdt installed |
ring3: roundtrip ok=true | fault-kill ok=true |
autorun3 hello exit=0 ring3 | autorun3 sysinfo exit=0 ring3 |
vfs bridge up | process spawned
```
(этап A: собственные таблицы ядра в CR3, ring-3 приложения на syscall-шлюзе,
намеренная #PF-проба убивает процесс — ядро продолжает.)
Команда QEMU — см. `.github/workflows/ci.yml` (job `qemu-smoke`) или
`scripts/qemu-run.cmd` (без `-display none`).

## Архитектурные границы (не нарушать без причины)

```
kernel ──► все крейты          drivers ──► (ничего из ядра)
sdk ──► abi (+ через ABI-таблицу к ядру — только косвенно)
net ──► std | fs ──► (чистый) | video ──► (чистый + spin)
hw ──► arch-x86_64 | drivers ──► hw (через DeviceProbe, без прямых зависимостей)
```
- Драйверы реализуют `orbita_drivers::Driver`; регистрация — в
  `kernel/src/drivers.rs` (`bind_builtin_drivers`).
- Графика: новые движки — `impl PresentBackend` + `register_backend`,
  не трогая потребителей.
- Приложения: только `orbita-sdk`; ABI-таблица — `orbita-abi`
  (все записи `extern "sysv64"`; ядро — Win64-таргет: мост уже есть в
  `kernel/src/abi.rs::call_with_stack` — при правках трамплина сохрани
  сохранение rdi/rsi!).

## Опасные места (проверено кровью)

- **Win64↔SysV мост**: ядро (uefi) и приложения (unknown-none) имеют
  разные ABI. Любой прямой вызов между ними — только через
  `call_with_stack`/ABI-таблицу.
- **App-регион `0x1000_0000`** резервируется ДО выделения кучи
  (UEFI отдаёт его иначе куче). База зашита в линкер-скрипт orbita-build.
- **`OrbitaDiskFs::list_dir`**: корень имеет parent==свой индекс — не
  «чинь» фильтр `*i != parent` (баг бесконечной рекурсии уже был).
- **ORBEXEC magic** — `ORBEXEC\0` (null, не пробел).
- **e1000**: буферы выравниваются вручную (`aligned_buffer`), poll-режим;
  дескрипторы — `repr(align(16))`.

## Определение готовности (Definition of Done)

Изменение считается завершённым, когда:
1. `cargo test --workspace --exclude orbita-kernel` — зелёный;
2. `cargo build --release -p orbita-kernel --target x86_64-unknown-uefi` — 0 ошибок/warnings;
3. Изменения в приложение/ABI/загрузку — QEMU smoke (маркеры выше);
4. Обновлён docs (rust-doc/раздел/диаграмма при необходимости);
5. Обновлён прогресс-файл этапа в `docs/stages/`;
6. Коммит: `область: суть` (напр. `net: TCP state machine (SYN-SENT→ESTABLISHED)`).

## Приоритеты (из roadmap)

Этап A (paging/user-mode) → B (SMP) → C (virtio-gpu) → D (TCP/сокеты) →
F (HTML/CSS UI) параллельно с E (драйверы). Полный порядок и
зависимости — `docs/roadmap.md`, раздел «Порядок исполнения».

## Скриптуемость (контракт платформы)

Скриптовый язык = команды встроенного shell'а (Linux-совместимые +
расширенные Orbita); `if/while/for/test/&&/||/exit`, запуск `sh x.sh`
и `./x.sh`. **Каждая возможность ОС должна управляться командами и
скриптами.** При добавлении модуля проверь манипулируемость его
возможностей из shell'а; если нет — добавь команду в
`orbita-shell/src/runtime.rs`, задокументируй в `docs/scripting.md`,
покрой host-тестом. Полный справочник: `docs/scripting.md`.
