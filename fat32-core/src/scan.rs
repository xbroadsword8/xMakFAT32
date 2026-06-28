//! 壞軌掃描器
//!
//! 逐 sector 讀取磁碟以檢測無法讀取的損壞區域。
//! 支援進度回報與取消操作。
//! 使用分塊讀取 (Chunked Reading) 大幅增進實體裝置掃描效能。

use crate::device::DeviceIO;
use crate::error::{FormatError, ProgressCallback};

// ============================================================================
// BadBlockScanner
// ============================================================================

/// 壞軌掃描器
///
/// 對指定 LBA 範圍進行分塊讀取，回傳讀取失敗的 sector LBA 清單。
pub struct BadBlockScanner<'a, D: DeviceIO, P: ProgressCallback> {
    device: &'a mut D,
    progress: &'a mut P,
}

impl<'a, D: DeviceIO, P: ProgressCallback> BadBlockScanner<'a, D, P> {
    /// 建立新的壞軌掃描器
    pub fn new(device: &'a mut D, progress: &'a mut P) -> Self {
        BadBlockScanner { device, progress }
    }

    /// 掃描指定 LBA 範圍，回傳壞軌 LBA 清單
    ///
    /// # 參數
    /// - `start_lba`: 起始 LBA（包含）
    /// - `end_lba`: 結束 LBA（包含）
    pub fn scan(
        &mut self,
        start_lba: u64,
        end_lba: u64,
    ) -> Result<Vec<u64>, FormatError> {
        if end_lba < start_lba {
            return Err(FormatError::InvalidParameter(
                "end_lba 必須 >= start_lba".into(),
            ));
        }

        let total = end_lba - start_lba + 1;
        let sector_size = self.device.sector_size() as u64;
        
        // 分塊讀取，預設 1MB 區塊，若總空間過小則調整
        let chunk_sectors = (1024 * 1024 / sector_size).max(1);
        let mut chunk_buf = vec![0u8; (chunk_sectors * sector_size) as usize];
        let mut single_buf = vec![0u8; sector_size as usize];
        
        let mut bad_blocks = Vec::new();
        let mut current_lba = start_lba;

        // 回報頻率
        const PROGRESS_INTERVAL: u64 = 128;
        let mut last_reported = 0;

        while current_lba <= end_lba {
            if self.progress.is_cancelled() {
                return Err(FormatError::Cancelled);
            }

            let remain = end_lba - current_lba + 1;
            let current_chunk = remain.min(chunk_sectors);
            let chunk_bytes = (current_chunk * sector_size) as usize;

            // 嘗試大區塊讀取
            match self.device.read_sectors(current_lba, &mut chunk_buf[..chunk_bytes]) {
                Ok(()) => {
                    // 區塊讀取成功，全部無壞軌
                    current_lba += current_chunk;
                }
                Err(_) => {
                    // 區塊讀取失敗，退回逐一磁區掃描該區塊
                    for lba in current_lba..(current_lba + current_chunk) {
                        if self.progress.is_cancelled() {
                            return Err(FormatError::Cancelled);
                        }
                        if self.device.read_sectors(lba, &mut single_buf).is_err() {
                            bad_blocks.push(lba);
                        }
                        // 退回逐一掃描時也更新進度
                        let current_scanned = lba - start_lba + 1;
                        if current_scanned - last_reported >= PROGRESS_INTERVAL || current_scanned == total {
                            self.progress.on_progress("掃描壞軌", current_scanned, total);
                            last_reported = current_scanned;
                        }
                    }
                    current_lba += current_chunk;
                }
            }

            // 更新進度
            let current_scanned = current_lba.saturating_sub(start_lba);
            if current_scanned - last_reported >= PROGRESS_INTERVAL || current_scanned == total {
                self.progress.on_progress("掃描壞軌", current_scanned, total);
                last_reported = current_scanned;
            }
        }

        Ok(bad_blocks)
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ConsoleProgress;
    use crate::device::FileDeviceIO;
    use std::io::Write;
    use std::fs::File;

    fn create_temp_file(name: &str, size_bytes: usize) -> std::path::PathBuf {
        let dir = std::env::temp_dir();
        let path = dir.join(name);
        let _ = std::fs::remove_file(&path);
        let mut file = File::create(&path).unwrap();
        file.write_all(&vec![0x00u8; size_bytes]).unwrap();
        file.sync_all().unwrap();
        path
    }

    #[test]
    fn test_scan_clean_disk() {
        let path = create_temp_file("fat32_scan_clean.bin", 4096);
        let mut dev = FileDeviceIO::new(path.to_str().unwrap()).unwrap();
        let mut progress = ConsoleProgress::new();
        let mut scanner = BadBlockScanner::new(&mut dev, &mut progress);

        let bad = scanner.scan(0, 7).unwrap();
        assert!(bad.is_empty(), "乾淨磁碟不應有壞軌");
    }

    #[test]
    fn test_scan_invalid_range() {
        let path = create_temp_file("fat32_scan_range.bin", 1024);
        let mut dev = FileDeviceIO::new(path.to_str().unwrap()).unwrap();
        let mut progress = ConsoleProgress::new();
        let mut scanner = BadBlockScanner::new(&mut dev, &mut progress);

        let result = scanner.scan(5, 3);
        assert!(result.is_err());
    }

    #[test]
    fn test_scan_single_sector() {
        let path = create_temp_file("fat32_scan_single.bin", 512);
        let mut dev = FileDeviceIO::new(path.to_str().unwrap()).unwrap();
        let mut progress = ConsoleProgress::new();
        let mut scanner = BadBlockScanner::new(&mut dev, &mut progress);

        let bad = scanner.scan(0, 0).unwrap();
        assert!(bad.is_empty());
    }

    #[test]
    fn test_scan_cancelled() {
        let path = create_temp_file("fat32_scan_cancel.bin", 4096);
        let mut dev = FileDeviceIO::new(path.to_str().unwrap()).unwrap();
        let mut progress = ConsoleProgress::new();

        progress.cancel();

        let mut scanner = BadBlockScanner::new(&mut dev, &mut progress);
        let result = scanner.scan(0, 7);

        assert!(matches!(result, Err(FormatError::Cancelled)));
    }
}
