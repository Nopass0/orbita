# Orbita Runtime And Concurrency

This document defines the execution-layer contract for Orbita OS.

## Scope

The runtime layer is separate from `kernel`, `mm`, `video`, and `platform`.
It owns execution policy, synchronization, thread metadata, and async driving.

## Crate split

- `orbita-sync` provides synchronization primitives and one-time initialization.
- `orbita-threading` provides thread identity, stack bounds, lifecycle state, and registry contracts.
- `orbita-scheduler` provides ready-queue policy and next-thread selection.
- `orbita-async` provides futures execution and wake contracts.
- `orbita-runtime` ties the pieces together into a boot-time runtime facade.

## Invariants

- The runtime layer must remain `no_std`.
- Synchronization primitives must expose safe call-site APIs.
- The scheduler decides order, not hardware context switching.
- Async execution is cooperative until preemption is added.

## Growth path

- per-core run queues
- timer-driven preemption
- lock-free wake queues
- async I/O reactors
- user/kernel address-space separation
