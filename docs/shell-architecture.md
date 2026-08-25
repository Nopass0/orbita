# Orbita Shell Contracts

The shell layer in Orbita is a contract crate, not a UI implementation.

## Scope

- parse command lines into a stable command model
- expose builtin command tables
- define TTY/session contracts
- preserve enough metadata for future GUI terminal and CLI layers

## Invariants

- `no_std` at the crate boundary
- parser stays allocation-light and explicit
- command dispatch is separated from terminal I/O
- session state owns cwd/history, not the parser

## Intended Growth

- builtin command execution
- command pipelines and redirection
- terminal line editing
- GUI terminal frontend
- external command resolver
