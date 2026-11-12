# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Windows-only GUI + CLI tool for formatting USB drives and SSDs (64MB–2TB) to FAT32 (MBR, single partition). Originally built for RME DURec recording media. Built with Rust and raw Win32 API — no third-party GUI framework.

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

> **Note:** The codebase targets Windows (`cfg(windows)`). Compilation on Linux will skip the `windows` crate dependencies and any Win32-dependent code — so `cargo test` on Linux being green says nothing about the Windows build (until 2026-11-11 the GUI had never compiled for Windows). Always also cross-compile from Linux:
>
> ```bash
> cargo build --release --target x86_64-pc-windows-gnu   # → target/x86_64-pc-windows-gnu/release/{xMakFAT32,xMakFAT32-cli}.exe
> ```
>
> `fat32-gui/build.rs` embeds `app.manifest` (requireAdministrator + Common Controls v6) via mingw `windres`.
> Real-device testing: on a Windows test machine (admin shell) create a VHD with diskpart (`create vdisk ... type=expandable`, `attach vdisk`) and run `xMakFAT32-cli.exe --list --include-virtual` / `--format N --include-virtual --yes`; verify with `chkdsk`. Never target a real disk in tests.

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

Pure Win32 API GUI (`windows` crate bindings). `src/winio.rs` holds all Windows disk I/O (enumerate USB/SD disks incl. SSDs while excluding the system disk, lock+dismount volumes, `\\.\PhysicalDriveN` read/write, `format_disk`). `src/main.rs` is the GUI; `src/bin/fat32-cli.rs` is a CLI over the same layer (used for remote testing).

## Key Constraints

- FAT32 range: 64MB minimum, 2TB maximum (`DeviceTooSmall` / `DeviceTooLarge` errors).
- Cluster sizes must be powers of 2 between 512B and 64KB.
- Volume labels: max 11 chars, ASCII uppercase only (FAT32 spec).
- All direct disk I/O requires administrator privileges on Windows.
