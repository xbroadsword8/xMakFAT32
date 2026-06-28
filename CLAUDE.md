# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Windows-only GUI tool for formatting USB drives (32GB–2TB) to FAT32. Built with Rust and raw Win32 API — no third-party GUI framework.

## Build Commands

```powershell
# Build (requires Windows SDK)
cargo build --release

# Check without building
cargo check

# Run tests
cargo test

# Run tests for a single crate
cargo test -p fat32-core

# Run a specific test
cargo test -p fat32-core test_name
```

> **Note:** The codebase targets Windows (`cfg(windows)`). Compilation on Linux will skip the `windows` crate dependencies and any Win32-dependent code.

## Architecture

This is a Cargo workspace with two crates:

```
fat32-core/   — pure logic library, no GUI dependency
fat32-gui/    — Win32 GUI binary, depends on fat32-core
```

### fat32-core modules

| Module | Purpose |
|--------|---------|
| `error` | `FormatError` enum + `ProgressCallback` trait |
| `fat`   | FAT32 on-disk data structures |
| `mbr`   | MBR partition table read/write |
| `scan`  | Bad-block scanner (`BadBlockScanner`) |
| `format`| Top-level formatting engine (`Formatter`) |

`ProgressCallback` is the interface between the core engine and the GUI — the GUI implements it to drive the progress bar and handle cancellation.

### fat32-gui

Pure Win32 API GUI (`windows` crate bindings). Entry point is `fat32-gui/src/main.rs`. Logging via `env_logger` (set `RUST_LOG=debug` to enable).

## Key Constraints

- FAT32 range: 64MB minimum, 2TB maximum (`DeviceTooSmall` / `DeviceTooLarge` errors).
- Cluster sizes must be powers of 2 between 512B and 64KB.
- Volume labels: max 11 chars, ASCII uppercase only (FAT32 spec).
- All direct disk I/O requires administrator privileges on Windows.
