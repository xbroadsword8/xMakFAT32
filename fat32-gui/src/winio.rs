//! Windows 磁碟存取層：列舉外接磁碟、卸載磁碟區、實體磁碟讀寫、執行格式化。
//!
//! GUI（main.rs）與 CLI（bin/fat32-cli.rs）共用。

#![allow(clippy::missing_safety_doc)]

use std::ffi::c_void;

use windows::core::PCWSTR;
use windows::Win32::Foundation::{CloseHandle, GENERIC_READ, GENERIC_WRITE, HANDLE};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FlushFileBuffers, ReadFile, SetFilePointerEx, WriteFile, BusTypeMmc, BusTypeSd,
    BusTypeUsb, FILE_BEGIN, FILE_FLAGS_AND_ATTRIBUTES, FILE_SHARE_READ, FILE_SHARE_WRITE,
    OPEN_EXISTING,
};
use windows::Win32::System::Ioctl::{
    PropertyStandardQuery, StorageDeviceProperty, DISK_GEOMETRY_EX, FSCTL_DISMOUNT_VOLUME,
    FSCTL_LOCK_VOLUME, FSCTL_UNLOCK_VOLUME, GET_LENGTH_INFORMATION, IOCTL_DISK_GET_DRIVE_GEOMETRY_EX,
    IOCTL_DISK_GET_LENGTH_INFO, IOCTL_DISK_UPDATE_PROPERTIES, IOCTL_STORAGE_GET_DEVICE_NUMBER,
    IOCTL_STORAGE_QUERY_PROPERTY, STORAGE_DEVICE_DESCRIPTOR, STORAGE_DEVICE_NUMBER,
    STORAGE_PROPERTY_QUERY,
};
use windows::Win32::System::SystemInformation::GetWindowsDirectoryW;
use windows::Win32::System::IO::DeviceIoControl;

use fat32_core::{DeviceIO, FormatConfig, FormatError, Formatter, ProgressCallback};

/// FAT32 上限（與 fat32-core 的 MAX_BYTES 一致）
pub const MAX_DISK_BYTES: u64 = (u32::MAX as u64 + 2048) * 512;

fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn win_err(e: windows::core::Error) -> std::io::Error {
    std::io::Error::from_raw_os_error(e.code().0 & 0xFFFF)
}

/// 開啟裝置路徑（\\.\PhysicalDriveN 或 \\.\X:）
fn open_device(path: &str, access: u32) -> windows::core::Result<HANDLE> {
    let w = to_wide(path);
    unsafe {
        CreateFileW(
            PCWSTR::from_raw(w.as_ptr()),
            access,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            None,
            OPEN_EXISTING,
            FILE_FLAGS_AND_ATTRIBUTES(0),
            HANDLE::default(),
        )
    }
}

unsafe fn ioctl_out<T: Default>(h: HANDLE, code: u32, input: Option<(*const c_void, u32)>) -> Option<T> {
    let mut out = T::default();
    let mut ret = 0u32;
    let (ip, il) = match input {
        Some((p, l)) => (Some(p), l),
        None => (None, 0),
    };
    DeviceIoControl(
        h,
        code,
        ip,
        il,
        Some(&mut out as *mut T as *mut c_void),
        std::mem::size_of::<T>() as u32,
        Some(&mut ret),
        None,
    )
    .ok()
    .map(|_| out)
}

fn device_number(h: HANDLE) -> Option<u32> {
    unsafe { ioctl_out::<STORAGE_DEVICE_NUMBER>(h, IOCTL_STORAGE_GET_DEVICE_NUMBER, None) }
        .map(|d| d.DeviceNumber)
}

fn disk_length(h: HANDLE) -> Option<u64> {
    // IOCTL_DISK_GET_LENGTH_INFO 需要讀取權限；DRIVE_GEOMETRY_EX 不需要，當備援
    unsafe { ioctl_out::<GET_LENGTH_INFORMATION>(h, IOCTL_DISK_GET_LENGTH_INFO, None) }
        .map(|l| l.Length as u64)
        .or_else(|| {
            unsafe { ioctl_out::<DISK_GEOMETRY_EX>(h, IOCTL_DISK_GET_DRIVE_GEOMETRY_EX, None) }
                .map(|g| g.DiskSize as u64)
        })
}

fn sector_size(h: HANDLE) -> Option<u32> {
    unsafe { ioctl_out::<DISK_GEOMETRY_EX>(h, IOCTL_DISK_GET_DRIVE_GEOMETRY_EX, None) }
        .map(|g| g.Geometry.BytesPerSector)
}

/// 回傳 (bus type, 廠牌型號)
fn storage_descriptor(h: HANDLE) -> Option<(i32, String)> {
    let query = STORAGE_PROPERTY_QUERY {
        PropertyId: StorageDeviceProperty,
        QueryType: PropertyStandardQuery,
        ..Default::default()
    };
    let mut buf = vec![0u8; 1024];
    let mut ret = 0u32;
    unsafe {
        DeviceIoControl(
            h,
            IOCTL_STORAGE_QUERY_PROPERTY,
            Some(&query as *const _ as *const c_void),
            std::mem::size_of::<STORAGE_PROPERTY_QUERY>() as u32,
            Some(buf.as_mut_ptr() as *mut c_void),
            buf.len() as u32,
            Some(&mut ret),
            None,
        )
        .ok()?;
        let d = &*(buf.as_ptr() as *const STORAGE_DEVICE_DESCRIPTOR);
        let read_str = |off: u32| -> String {
            if off == 0 || off as usize >= buf.len() {
                return String::new();
            }
            let tail = &buf[off as usize..];
            let end = tail.iter().position(|&b| b == 0).unwrap_or(tail.len());
            String::from_utf8_lossy(&tail[..end]).trim().to_string()
        };
        let name = format!("{} {}", read_str(d.VendorIdOffset), read_str(d.ProductIdOffset))
            .trim()
            .to_string();
        Some((d.BusType.0, name))
    }
}

/// Windows 所在磁碟的實體編號（絕對不能格式化）
fn system_disk_number() -> Option<u32> {
    let mut buf = [0u16; 260];
    let n = unsafe { GetWindowsDirectoryW(Some(&mut buf)) } as usize;
    if n < 2 {
        return None;
    }
    let letter = String::from_utf16_lossy(&buf[..2]); // "C:"
    let h = open_device(&format!("\\\\.\\{letter}"), 0).ok()?;
    let r = device_number(h);
    unsafe { let _ = CloseHandle(h); }
    r
}

/// 一顆可供選擇的外接磁碟
#[derive(Debug, Clone)]
pub struct DiskInfo {
    pub number: u32,
    pub model: String,
    pub size_bytes: u64,
    pub letters: Vec<char>,
    pub sector_size: u32,
}

impl DiskInfo {
    pub fn display(&self) -> String {
        let gb = self.size_bytes as f64 / 1e9;
        let size = if gb >= 1000.0 { format!("{:.2} TB", gb / 1000.0) } else { format!("{gb:.1} GB") };
        let letters = if self.letters.is_empty() {
            "無磁碟機代號".to_string()
        } else {
            self.letters.iter().map(|c| format!("{c}:")).collect::<Vec<_>>().join(" ")
        };
        let mut s = format!("磁碟 {}：{}  {}  [{}]", self.number, self.model, size, letters);
        if self.sector_size != 512 {
            s.push_str(&format!("  （sector {} bytes，不支援）", self.sector_size));
        }
        if self.size_bytes > MAX_DISK_BYTES {
            s.push_str("  （超過 2TB，FAT32 無法使用）");
        }
        s
    }
}

/// 列出所有 USB / SD 匯流排上的磁碟（含 SSD、行動硬碟），排除 Windows 系統碟。
/// `include_virtual` 只供測試用：一併列出 VHD 虛擬磁碟。
pub fn enumerate_external_disks(include_virtual: bool) -> Vec<DiskInfo> {
    let sys = system_disk_number();
    // 先建立 磁碟機代號 → 實體編號 對照
    let mut letter_map: Vec<(char, u32)> = Vec::new();
    for c in 'A'..='Z' {
        if let Ok(h) = open_device(&format!("\\\\.\\{c}:"), 0) {
            if let Some(n) = device_number(h) {
                letter_map.push((c, n));
            }
            unsafe { let _ = CloseHandle(h); }
        }
    }
    let mut out = Vec::new();
    for n in 0..64u32 {
        let path = format!("\\\\.\\PhysicalDrive{n}");
        let Ok(h) = open_device(&path, GENERIC_READ.0).or_else(|_| open_device(&path, 0)) else { continue };
        let desc = storage_descriptor(h);
        let len = disk_length(h);
        let ss = sector_size(h).unwrap_or(512);
        unsafe { let _ = CloseHandle(h); }
        if std::env::var_os("FAT32_DEBUG").is_some() {
            eprintln!("PhysicalDrive{n}: desc={desc:?} len={len:?} ss={ss} sys={sys:?}");
        }
        let (Some((bus, model)), Some(len)) = (desc, len) else { continue };
        let external = bus == BusTypeUsb.0 || bus == BusTypeSd.0 || bus == BusTypeMmc.0;
        // BusTypeFileBackedVirtual = 15（VHD）
        let virt = include_virtual && bus == 15;
        if !(external || virt) || Some(n) == sys || len == 0 {
            continue;
        }
        let letters = letter_map.iter().filter(|(_, d)| *d == n).map(|(c, _)| *c).collect();
        out.push(DiskInfo { number: n, model, size_bytes: len, letters, sector_size: ss });
    }
    out
}

// ============================================================================
// 磁碟區鎖定（格式化期間持有，結束後釋放）
// ============================================================================

struct VolumeLock {
    handle: HANDLE,
}

impl Drop for VolumeLock {
    fn drop(&mut self) {
        unsafe {
            let mut r = 0u32;
            let _ = DeviceIoControl(self.handle, FSCTL_UNLOCK_VOLUME, None, 0, None, 0, Some(&mut r), None);
            let _ = CloseHandle(self.handle);
        }
    }
}

fn lock_and_dismount(letter: char) -> Result<VolumeLock, FormatError> {
    let h = open_device(&format!("\\\\.\\{letter}:"), GENERIC_READ.0 | GENERIC_WRITE.0)
        .map_err(|e| FormatError::LockFailed(format!("無法開啟 {letter}:（{e}）")))?;
    let lock = VolumeLock { handle: h };
    unsafe {
        let mut r = 0u32;
        // 有程式開著檔案時鎖定會失敗：重試幾次
        let mut locked = false;
        for _ in 0..10 {
            if DeviceIoControl(h, FSCTL_LOCK_VOLUME, None, 0, None, 0, Some(&mut r), None).is_ok() {
                locked = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(300));
        }
        if !locked {
            return Err(FormatError::LockFailed(format!(
                "無法鎖定 {letter}:，請關閉正在使用這個磁碟的程式（檔案總管視窗、播放器等）後再試"
            )));
        }
        DeviceIoControl(h, FSCTL_DISMOUNT_VOLUME, None, 0, None, 0, Some(&mut r), None)
            .map_err(|e| FormatError::LockFailed(format!("無法卸載 {letter}:（{e}）")))?;
    }
    Ok(lock)
}

// ============================================================================
// 實體磁碟 DeviceIO
// ============================================================================

pub struct WindowsDeviceIO {
    handle: HANDLE,
    sector_size: u64,
    total_sectors: u64,
}

// HANDLE 只是作業系統控制代碼；同一時間只有格式化執行緒使用它。
unsafe impl Send for WindowsDeviceIO {}

impl WindowsDeviceIO {
    pub fn open(disk_number: u32) -> Result<Self, FormatError> {
        let path = format!("\\\\.\\PhysicalDrive{disk_number}");
        let handle = open_device(&path, GENERIC_READ.0 | GENERIC_WRITE.0).map_err(|e| {
            FormatError::DeviceOpen(format!("無法開啟 {path}：{e}\n請以系統管理員身分執行"))
        })?;
        // 先包進 struct，讓任何錯誤返回時都由 Drop 關閉 handle（且只關一次）
        let mut dev = WindowsDeviceIO { handle, sector_size: 512, total_sectors: 0 };
        let ss = sector_size(handle).unwrap_or(512) as u64;
        if ss != 512 {
            return Err(FormatError::InvalidParameter(format!(
                "這顆磁碟的 sector 大小是 {ss} bytes，目前只支援 512 bytes"
            )));
        }
        let len = disk_length(handle)
            .ok_or_else(|| FormatError::DeviceOpen("無法查詢磁碟容量".into()))?;
        dev.sector_size = ss;
        dev.total_sectors = len / ss;
        Ok(dev)
    }

    fn seek(&mut self, lba: u64) -> Result<(), FormatError> {
        unsafe {
            SetFilePointerEx(self.handle, (lba * self.sector_size) as i64, None, FILE_BEGIN)
                .map_err(|e| FormatError::Io(win_err(e)))
        }
    }

    /// 通知 Windows 重新讀取分割表
    pub fn refresh_partitions(&mut self) {
        unsafe {
            let _ = FlushFileBuffers(self.handle);
            let mut r = 0u32;
            let _ = DeviceIoControl(self.handle, IOCTL_DISK_UPDATE_PROPERTIES, None, 0, None, 0, Some(&mut r), None);
        }
    }
}

const IO_CHUNK: usize = 4 * 1024 * 1024;

impl DeviceIO for WindowsDeviceIO {
    fn sector_size(&self) -> u64 {
        self.sector_size
    }
    fn total_sectors(&self) -> u64 {
        self.total_sectors
    }

    fn read_sectors(&mut self, lba: u64, buf: &mut [u8]) -> Result<(), FormatError> {
        self.seek(lba)?;
        for chunk in buf.chunks_mut(IO_CHUNK) {
            let mut n = 0u32;
            unsafe { ReadFile(self.handle, Some(chunk), Some(&mut n), None) }
                .map_err(|e| FormatError::Io(win_err(e)))?;
            if n as usize != chunk.len() {
                return Err(FormatError::Io(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    format!("只讀到 {n} bytes，預期 {}", chunk.len()),
                )));
            }
        }
        Ok(())
    }

    fn write_sectors(&mut self, lba: u64, buf: &[u8]) -> Result<(), FormatError> {
        self.seek(lba)?;
        for chunk in buf.chunks(IO_CHUNK) {
            let mut n = 0u32;
            unsafe { WriteFile(self.handle, Some(chunk), Some(&mut n), None) }
                .map_err(|e| FormatError::Io(win_err(e)))?;
            if n as usize != chunk.len() {
                return Err(FormatError::Io(std::io::Error::new(
                    std::io::ErrorKind::WriteZero,
                    format!("只寫入 {n} bytes，預期 {}", chunk.len()),
                )));
            }
        }
        Ok(())
    }
    // lock/unlock：實體磁碟本身不用鎖；磁碟區的鎖定在 format_disk() 處理。
}

impl Drop for WindowsDeviceIO {
    fn drop(&mut self) {
        unsafe { let _ = CloseHandle(self.handle); }
    }
}

/// 完整流程：鎖定並卸載該磁碟上所有磁碟區 → 格式化 → 通知 Windows 重新掛載。
pub fn format_disk<P: ProgressCallback>(
    disk: &DiskInfo,
    config: FormatConfig,
    progress: P,
) -> Result<(), FormatError> {
    if disk.size_bytes > MAX_DISK_BYTES {
        return Err(FormatError::DeviceTooLarge(disk.size_bytes));
    }
    // 安全檢查：再確認一次不是系統碟
    if Some(disk.number) == system_disk_number() {
        return Err(FormatError::InvalidParameter("拒絕格式化 Windows 系統磁碟".into()));
    }
    let mut locks = Vec::new();
    for &c in &disk.letters {
        locks.push(lock_and_dismount(c)?);
    }
    let device = WindowsDeviceIO::open(disk.number)?;
    let mut formatter = Formatter::new(device, config, progress);
    let result = formatter.run();
    let (mut device, _, _) = formatter.into_inner();
    device.refresh_partitions();
    drop(device);
    drop(locks);
    result
}
