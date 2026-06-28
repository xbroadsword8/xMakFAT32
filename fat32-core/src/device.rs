//! 裝置 I/O 抽象層
//!
//! 定義 `DeviceIO` trait，解耦核心格式化邏輯與平台特定的裝置操作。
//! 同時提供一個基礎的 `FileDeviceIO` 實作，用於 Linux 環境下的測試。

use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};

use crate::error::FormatError;

// ============================================================================
// DeviceIO trait
// ============================================================================

/// 區塊裝置 I/O 抽象
///
/// 所有對實體磁碟的讀寫都透過此 trait 進行，
/// 讓核心邏輯不依賴特定平台 API。
pub trait DeviceIO: Send {
    /// 每個 sector 的位元組數（通常為 512）
    fn sector_size(&self) -> u64;

    /// 裝置總 sector 數
    fn total_sectors(&self) -> u64;

    /// 從指定 LBA 讀取一個或多個 sector
    ///
    /// `buf` 長度必須是 `sector_size()` 的整數倍。
    fn read_sectors(&mut self, lba: u64, buf: &mut [u8]) -> Result<(), FormatError>;

    /// 從指定 LBA 寫入一個或多個 sector
    ///
    /// `buf` 長度必須是 `sector_size()` 的整數倍。
    fn write_sectors(&mut self, lba: u64, buf: &[u8]) -> Result<(), FormatError>;

    /// 鎖定裝置（卸載已掛載磁碟區）
    ///
    /// 預設為空操作；Windows 平台需實作 `FSCTL_LOCK_VOLUME`。
    fn lock(&mut self) -> Result<(), FormatError> {
        Ok(())
    }

    /// 解鎖裝置
    ///
    /// 預設為空操作；Windows 平台需實作 `FSCTL_UNLOCK_VOLUME`。
    fn unlock(&mut self) -> Result<(), FormatError> {
        Ok(())
    }
}

// ============================================================================
// FileDeviceIO — 基於 std::fs::File 的實作（Linux 測試用）
// ============================================================================

/// 以檔案模擬區塊裝置的 `DeviceIO` 實作
///
/// 適用於 Linux 環境的單元測試與整合測試，
/// 不依賴任何平台特定的裝置控制 API。
pub struct FileDeviceIO {
    file: File,
    sector_size: u64,
    total_sectors: u64,
}

impl FileDeviceIO {
    /// 開啟指定路徑的檔案作為區塊裝置
    ///
    /// 以讀寫模式開啟，並自動計算 sector 大小與總數。
    /// 預設 sector 大小為 512 bytes。
    pub fn new(path: &str) -> Result<Self, FormatError> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(path)
            .map_err(|e| {
                FormatError::DeviceOpen(format!("無法開啟檔案 {path}: {e}"))
            })?;

        let metadata = file
            .metadata()
            .map_err(FormatError::Io)?;

        let file_size = metadata.len();
        let sector_size: u64 = 512;
        let total_sectors = file_size / sector_size;

        Ok(FileDeviceIO {
            file,
            sector_size,
            total_sectors,
        })
    }
}

impl DeviceIO for FileDeviceIO {
    fn sector_size(&self) -> u64 {
        self.sector_size
    }

    fn total_sectors(&self) -> u64 {
        self.total_sectors
    }

    fn read_sectors(&mut self, lba: u64, buf: &mut [u8]) -> Result<(), FormatError> {
        let offset = lba * self.sector_size;
        self.file
            .seek(SeekFrom::Start(offset))
            .map_err(FormatError::Io)?;
        self.file
            .read_exact(buf)
            .map_err(FormatError::Io)?;
        Ok(())
    }

    fn write_sectors(&mut self, lba: u64, buf: &[u8]) -> Result<(), FormatError> {
        let offset = lba * self.sector_size;
        self.file
            .seek(SeekFrom::Start(offset))
            .map_err(FormatError::Io)?;
        self.file
            .write_all(buf)
            .map_err(FormatError::Io)?;
        Ok(())
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// 輔助函式：建立一個臨時檔案並寫入初始資料
    fn create_temp_file(name: &str, size_bytes: usize) -> std::path::PathBuf {
        let dir = std::env::temp_dir();
        let path = dir.join(name);

        // 先刪除可能殘留的舊檔案
        let _ = std::fs::remove_file(&path);

        let mut file = File::create(&path).unwrap();
        let data = vec![0xAAu8; size_bytes];
        file.write_all(&data).unwrap();
        file.sync_all().unwrap();

        path
    }

    #[test]
    fn test_file_device_io_basic() {
        let path = create_temp_file("fat32_test_basic.bin", 4096);
        let path_str = path.to_str().unwrap();

        let dev = FileDeviceIO::new(path_str).unwrap();
        assert_eq!(dev.sector_size(), 512);
        assert_eq!(dev.total_sectors(), 8); // 4096 / 512
    }

    #[test]
    fn test_file_device_io_read_write() {
        let path = create_temp_file("fat32_test_rw.bin", 2048);
        let path_str = path.to_str().unwrap();

        let mut dev = FileDeviceIO::new(path_str).unwrap();

        // 寫入 LBA 2
        let write_buf = [0x42u8; 512];
        dev.write_sectors(2, &write_buf).unwrap();

        // 讀回 LBA 2
        let mut read_buf = [0u8; 512];
        dev.read_sectors(2, &mut read_buf).unwrap();
        assert_eq!(read_buf, write_buf);

        // LBA 0 仍為初始值 0xAA
        let mut read_buf_lba0 = [0u8; 512];
        dev.read_sectors(0, &mut read_buf_lba0).unwrap();
        assert_eq!(read_buf_lba0, [0xAAu8; 512]);
    }

    #[test]
    fn test_file_device_io_multi_sector() {
        let path = create_temp_file("fat32_test_multi.bin", 4096);
        let path_str = path.to_str().unwrap();

        let mut dev = FileDeviceIO::new(path_str).unwrap();

        // 寫入 4 個 sector（2048 bytes）到 LBA 0
        let write_buf = [0xCDu8; 2048];
        dev.write_sectors(0, &write_buf).unwrap();

        // 讀回
        let mut read_buf = [0u8; 2048];
        dev.read_sectors(0, &mut read_buf).unwrap();
        assert_eq!(read_buf, write_buf);
    }

    #[test]
    fn test_file_device_io_lock_unlock_noop() {
        let path = create_temp_file("fat32_test_lock.bin", 512);
        let path_str = path.to_str().unwrap();

        let mut dev = FileDeviceIO::new(path_str).unwrap();

        // lock/unlock 應該總是成功（空操作）
        assert!(dev.lock().is_ok());
        dev.write_sectors(0, &[0xFFu8; 512]).unwrap();
        assert!(dev.unlock().is_ok());
    }
}
