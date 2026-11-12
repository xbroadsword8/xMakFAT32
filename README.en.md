# xMakFAT32

[English](README.en.md) | [繁體中文](README.md)

[![Download xMakFAT32](https://img.shields.io/github/v/release/xbroadsword8/xMakFAT32?label=Download%20for%20Windows&style=for-the-badge&logo=windows)](https://github.com/xbroadsword8/xMakFAT32/releases/latest)

Portable, no installation: download the zip, extract it, and double-click `xMakFAT32.exe`.

**xMakFAT32** is a small Windows tool that formats USB flash drives and external SSDs (up to 2TB) as **FAT32**.

## Why this tool exists

A lot of older gear (audio interfaces, field recorders, cameras, car stereos, game consoles, industrial equipment, …) **can only read FAT32**. But the format tool built into Windows won't format a drive larger than 32GB as FAT32; it only offers exFAT or NTFS. Almost every flash drive and SSD sold today is larger than 32GB, so they can't be used with that gear. This tool fixes that.

It formats the whole drive as:

- an **MBR** partition table with a **single partition** (type 0x0C, aligned at 1MB)
- **FAT32**, with a selectable cluster size (default 32KB)
- It also wipes any old GPT header and the backup GPT at the end of the drive, so devices don't misdetect the layout

## ✅ Tested with RME DURec

**Tested and working with DURec (Direct USB Recording) on the RME UFX+**: a 1TB SanDisk Portable SSD formatted with this tool is recognized by the UFX+ and records normally.

DURec's requirements match this tool's defaults: MBR, FAT32, first partition, 32KB clusters recommended. Other RME interfaces with DURec (UFX II, UFX III, …) have the same requirements but have not been tested yet.

> ⚠️ **Formatting erases all data on the entire drive.** Back up first, and double-check that you picked the right drive.

## Download and usage

Download the zip from [Releases](https://github.com/xbroadsword8/xMakFAT32/releases) and extract it. The interface is in Traditional Chinese; the labels are given below.

1. Plug in the drive you want to format.
2. Double-click `xMakFAT32.exe`. Windows asks for permission (administrator rights are needed to write to a disk); click "Yes".
3. Pick the drive under「目標磁碟」(target disk). Only external USB/SD disks are listed; **the system disk is never shown**. If your drive is missing, click「重新整理」(refresh).
4. Keep the default cluster size of 32KB. The volume label (「磁碟區標籤」) can be left empty (uppercase letters and digits, up to 11 characters).
5. Click「開始格式化」(start formatting), check the drive shown in the confirmation box, and click "Yes". A quick format takes only a few seconds.

The executables are not code-signed, so Windows SmartScreen may show a warning the first time. Click "More info → Run anyway".

### Command-line version

`xMakFAT32-cli.exe` uses the same formatting code and is handy for scripts or remote use (run it from an administrator terminal):

```text
xMakFAT32-cli --list                        list disks that can be formatted
xMakFAT32-cli --format <disk number> --yes  format a disk
    [--label NAME]                          volume label
    [--cluster-kb N]                        cluster size in KB (default: auto; RME recommends 32)
    [--full]                                full format (scans for bad blocks, takes a long time)
```

## Limitations

- Drive size 64MB–2TB (the MBR/FAT32 limit). So far only tested on drives with 512-byte sectors.
- A single file on FAT32 can be at most 4GB (a limit of the file system itself).
- Windows 10/11 (x64) only.

## Building

Requires Rust 1.77 or later. On Windows:

```powershell
cargo build --release
```

Or cross-compile on Linux (install `mingw-w64`, then `rustup target add x86_64-pc-windows-gnu`):

```bash
cargo build --release --target x86_64-pc-windows-gnu
```

This produces `xMakFAT32.exe` and `xMakFAT32-cli.exe`. `cargo test` runs the tests for the core engine (`fat32-core`).

### Project layout

| Path | Contents |
|---|---|
| `fat32-core/` | Formatting engine: MBR, FAT32 structures, bad-block scanning (no Windows dependency, testable on any platform) |
| `fat32-gui/src/winio.rs` | Windows disk access: enumerate disks, lock/dismount volumes, read/write `\\.\PhysicalDriveN` |
| `fat32-gui/src/main.rs` | Win32 GUI |
| `fat32-gui/src/bin/fat32-cli.rs` | Command-line interface |

## License

[PolyForm Noncommercial License 1.0.0](LICENSE): **free for any noncommercial use**, including personal use, teaching, research, schools and nonprofit organizations. You may modify and redistribute it, as long as you include the license terms. **Commercial use is not permitted.** For a commercial license, contact the author through GitHub Issues.

This software is provided "as is", without warranty of any kind. The author is not responsible for any data loss caused by using it.

RME, UFX+ and DURec are trademarks of RME Audio, and Windows is a trademark of Microsoft. This project is not affiliated with either company.
