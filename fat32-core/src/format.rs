//! 頂層格式化引擎
//!
//! 協調完整的 FAT32 格式化流程：
//! 驗證 → MBR → Boot Sector → FSInfo → FAT 表 → 根目錄 → 壞軌掃描。
//!
//! 透過 `DeviceIO` 與 `ProgressCallback` 解耦平台操作與 UI。

use crate::device::DeviceIO;
use crate::error::{FormatError, ProgressCallback};
use crate::scan::BadBlockScanner;
use crate::fat::{
    self, BootSector, FsInfo, DirEntry, RESERVED_SECTORS,
    FAT32_EOC, FAT32_BAD_CLUSTER, FAT32_FREE_CLUSTER,
    MEDIA_DESCRIPTOR, FSINFO_SECTOR, BACKUP_BOOT_SECTOR,
};
use crate::mbr::Mbr;

// ============================================================================
// FormatConfig
// ============================================================================

/// 格式化參數設定
#[derive(Debug, Clone)]
pub struct FormatConfig {
    /// 磁碟區標籤（最多 11 字元，ASCII 大寫）
    pub label: String,
    /// 每叢集 sector 數（`None` = 自動選擇）
    pub cluster_size: Option<u32>,
    /// 是否快速格式化（跳過壞軌掃描）
    pub quick_format: bool,
    /// 壞軌容忍率（0.0 ~ 1.0，超過則報錯）
    pub bad_block_threshold: f64,
}

impl Default for FormatConfig {
    fn default() -> Self {
        FormatConfig {
            label: String::new(),
            cluster_size: None,
            quick_format: false,
            bad_block_threshold: 0.05,
        }
    }
}

// ============================================================================
// Formatter
// ============================================================================

/// FAT32 格式化器
///
/// 泛型參數：
/// - `D: DeviceIO` — 裝置 I/O 實作
/// - `P: ProgressCallback` — 進度回呼實作
pub struct Formatter<D: DeviceIO, P: ProgressCallback> {
    device: D,
    config: FormatConfig,
    progress: P,
}

impl<D: DeviceIO, P: ProgressCallback> Formatter<D, P> {
    // 裝置大小範圍常數
    const MIN_BYTES: u64 = 64 * 1024 * 1024;   // 64MB
    // MBR 分割區表使用 32-bit sector 計數，最大值為 u32::MAX
    // 考慮 1MB 分割區偏移：最大安全裝置大小 = (u32::MAX + 2048) * 512
    const MAX_BYTES: u64 = (u32::MAX as u64 + 2048) * 512; // ≈ 2TB

    // LBA 佈局常數
    const MBR_LBA: u64 = 0;
    const PARTITION_START_LBA: u64 = 2048; // 1MB 對齊

    /// 建立新的格式化器
    pub fn new(device: D, config: FormatConfig, progress: P) -> Self {
        Formatter {
            device,
            config,
            progress,
        }
    }

    /// 執行完整格式化流程
    ///
    /// # 步驟
    /// 1. 驗證裝置大小（64MB ~ 2TB）
    /// 2. 決定叢集大小
    /// 3. 驗證磁碟區標籤
    /// 4. 鎖定裝置
    /// 5. 寫入 MBR
    /// 6. 寫入 Boot Sector（主 + 備份）
    /// 7. 寫入 FSInfo
    /// 8. 清除保留 sector
    /// 9. 建立並寫入 FAT 表（兩份）
    /// 10. 寫入根目錄
    /// 11. 壞軌掃描（非快速格式化）
    /// 12. 更新 FSInfo 空閒叢集計數
    /// 13. 解鎖裝置
    pub fn run(&mut self) -> Result<(), FormatError> {
        // ---- 步驟 1: 驗證裝置大小 ----
        self.report("驗證裝置", 0, 1);
        let total_sectors = self.device.total_sectors();
        let sector_size = self.device.sector_size();
        let total_bytes = total_sectors * sector_size;

        if total_bytes < Self::MIN_BYTES {
            return Err(FormatError::DeviceTooSmall(total_bytes));
        }
        if total_bytes > Self::MAX_BYTES {
            return Err(FormatError::DeviceTooLarge(total_bytes));
        }
        self.report("驗證裝置", 1, 1);

        // ---- 步驟 2: 決定叢集大小 ----
        self.report("決定叢集大小", 0, 1);
        let sectors_per_cluster = match self.config.cluster_size {
            Some(n) => {
                // 手動指定：必須是 2 的冪，且在 1~128 之間
                let n_u8 = n as u8;
                if !(1..=128).contains(&n) || !fat::is_power_of_two(n) {
                    return Err(FormatError::InvalidParameter(
                        format!("叢集大小 {n} 無效，必須是 2 的冪且 ≤ 128"),
                    ));
                }
                n_u8
            }
            None => {
                // 自動選擇（以分割區大小為依據）
                let partition_sectors = total_sectors - Self::PARTITION_START_LBA;
                fat::choose_cluster_size(partition_sectors, sector_size)
            }
        };
        self.report("決定叢集大小", 1, 1);

        // ---- 步驟 3: 驗證磁碟區標籤 ----
        let volume_label = fat::validate_label(&self.config.label)
            .map_err(|e| FormatError::InvalidLabel(e.to_string()))?;

        // ---- 計算 FAT 相關參數並對齊 Data 區域 ----
        let partition_sectors = total_sectors - Self::PARTITION_START_LBA;
        let mut reserved_sectors: u16 = RESERVED_SECTORS;
        let mut sectors_per_fat;
        let mut data_start_lba;

        // 計算對齊（讓 Data 區域對齊叢集邊界，避免 Flash 寫入放大）
        loop {
            sectors_per_fat = fat::compute_sectors_per_fat(partition_sectors, reserved_sectors, sectors_per_cluster, sector_size);
            data_start_lba = Self::PARTITION_START_LBA + reserved_sectors as u64 + (sectors_per_fat as u64 * 2);
            let offset = data_start_lba - Self::PARTITION_START_LBA;
            let remainder = offset % sectors_per_cluster as u64;
            if remainder == 0 {
                break;
            }
            reserved_sectors += (sectors_per_cluster as u64 - remainder) as u16;
        }

        let total_data_sectors = total_sectors.saturating_sub(data_start_lba);
        let total_clusters = total_data_sectors / sectors_per_cluster as u64;

        // ---- 步驟 4: 鎖定裝置 ----
        self.report("鎖定裝置", 0, 1);
        self.device.lock()?;
        self.report("鎖定裝置", 1, 1);

        // ---- 步驟 5: 寫入 MBR ----
        self.report("寫入 MBR", 0, 1);
        let mbr = Mbr::new(partition_sectors);
        self.device.write_sectors(Self::MBR_LBA, &mbr.to_bytes())?;
        self.report("寫入 MBR", 1, 1);

        // ---- 步驟 6: 寫入 Boot Sector ----
        self.report("寫入 Boot Sector", 0, 2);

        let volume_serial = Self::generate_volume_serial();
        let boot_sector = BootSector::new()
            .total_sectors(partition_sectors as u32)
            .bytes_per_sector(sector_size as u16)
            .sectors_per_cluster(sectors_per_cluster)
            .reserved_sectors(reserved_sectors)
            .sectors_per_fat(sectors_per_fat)
            .volume_label(volume_label)
            .volume_serial(volume_serial);

        let boot_bytes = boot_sector.to_bytes();
        let bs_lba = Self::PARTITION_START_LBA;

        // 主 Boot Sector
        self.device.write_sectors(bs_lba, &boot_bytes)?;
        // 備份 Boot Sector
        self.device.write_sectors(bs_lba + BACKUP_BOOT_SECTOR as u64, &boot_bytes)?;
        self.report("寫入 Boot Sector", 2, 2);

        // ---- 步驟 7: 寫入 FSInfo ----
        self.report("寫入 FSInfo", 0, 1);
        let fsinfo = FsInfo::new();
        let fsinfo_bytes = fsinfo.to_bytes();
        // 主 FSInfo
        self.device.write_sectors(bs_lba + FSINFO_SECTOR as u64, &fsinfo_bytes)?;
        // 備份 FSInfo (微軟規範需備份至 LBA 7)
        self.device.write_sectors(bs_lba + BACKUP_BOOT_SECTOR as u64 + 1, &fsinfo_bytes)?;
        self.report("寫入 FSInfo", 1, 1);

        // ---- 步驟 8: 清除保留 sector（LBA 2 ~ reserved_sectors，boot sector 後的保留區）----
        self.report("初始化保留區", 0, 1);
        let zero_sector = vec![0u8; sector_size as usize];
        // 保留 sector 中，LBA 0 = boot sector, LBA 1 = FSInfo → 已寫入
        // 備份區：LBA 6 = backup boot, LBA 7 = backup FSInfo → 已寫入
        // LBA 8 = backup boot code (由於我們 boot code 在 LBA 0 已完成，LBA 2 是空的，備份到 LBA 8 也是寫零)
        for i in 2..reserved_sectors as u64 {
            if i == BACKUP_BOOT_SECTOR as u64 || i == (BACKUP_BOOT_SECTOR as u64 + 1) {
                continue; // 備份 boot sector 與 FSInfo 已寫入
            }
            self.device.write_sectors(bs_lba + i, &zero_sector)?;
        }
        self.report("初始化保留區", 1, 1);

        // ---- 步驟 9: 建立並寫入 FAT 表 ----
        self.report("寫入 FAT 表", 0, sectors_per_fat as u64);

        let fat_entries = total_clusters + 2; // cluster 0, 1 保留
        let mut fat = vec![0u8; sectors_per_fat as usize * sector_size as usize];

        // 初始化 FAT: 每個 entry 4 bytes (little-endian)
        // Entry 0: media descriptor
        Self::set_fat_entry(&mut fat, 0, MEDIA_DESCRIPTOR as u32 | 0x0FFFFFF00);
        // Entry 1: EOC (dirty volume flag / reserved)
        Self::set_fat_entry(&mut fat, 1, FAT32_EOC);
        // Entry 2 (root dir): EOC
        Self::set_fat_entry(&mut fat, 2, FAT32_EOC);
        // Entry 3+: free
        for cluster in 3..fat_entries {
            Self::set_fat_entry(&mut fat, cluster, FAT32_FREE_CLUSTER);
        }

        // 寫入 FAT #1
        let fat1_lba = bs_lba + reserved_sectors as u64;
        self.device.write_sectors(fat1_lba, &fat)?;

        // 寫入 FAT #2 (相同內容)
        let fat2_lba = fat1_lba + sectors_per_fat as u64;
        self.device.write_sectors(fat2_lba, &fat)?;

        self.report("寫入 FAT 表", sectors_per_fat as u64, sectors_per_fat as u64);

        // ---- 步驟 10: 寫入根目錄 ----
        self.report("寫入根目錄", 0, 1);

        let root_dir_lba = data_start_lba;
        let cluster_bytes = sectors_per_cluster as usize * sector_size as usize;
        let mut root_dir = vec![0u8; cluster_bytes];

        // 根目錄第一項：磁碟區標籤
        let vol_entry = DirEntry::new_volume_label(volume_label);
        let vol_bytes = vol_entry.to_bytes();
        root_dir[0..32].copy_from_slice(&vol_bytes);

        self.device.write_sectors(root_dir_lba, &root_dir)?;
        self.report("寫入根目錄", 1, 1);

        // ---- 步驟 11: 壞軌掃描（非快速格式化）----
        let mut bad_clusters: Vec<u64> = Vec::new();

        if !self.config.quick_format {
            self.report("掃描壞軌", 0, 1);

            // 掃描資料區域
            let scan_end_lba = data_start_lba + total_data_sectors - 1;
            
            let mut scanner = BadBlockScanner::new(&mut self.device, &mut self.progress);
            let bad_lbas = scanner.scan(data_start_lba, scan_end_lba)?;

            // 將壞軌 LBA 轉為 cluster 編號
            for lba in &bad_lbas {
                if *lba >= data_start_lba {
                    let cluster = (*lba - data_start_lba) / sectors_per_cluster as u64 + 2;
                    bad_clusters.push(cluster);
                }
            }

            // 去重，避免同一個叢集被算作多個壞軌
            bad_clusters.sort_unstable();
            bad_clusters.dedup();

            // 檢查是否超過容許閾值
            let bad_ratio = bad_clusters.len() as f64 / total_clusters as f64;
            if bad_ratio > self.config.bad_block_threshold {
                return Err(FormatError::TooManyBadBlocks {
                    total: total_clusters,
                    bad: bad_clusters.len() as u64,
                    threshold: self.config.bad_block_threshold,
                });
            }

            // 標記壞叢集到 FAT
            if !bad_clusters.is_empty() {
                self.report("標記壞叢集", 0, 1);
                for &cluster in &bad_clusters {
                    Self::set_fat_entry(&mut fat, cluster, FAT32_BAD_CLUSTER);
                }
                // 重新寫入兩份 FAT
                self.device.write_sectors(fat1_lba, &fat)?;
                self.device.write_sectors(fat2_lba, &fat)?;
                self.report("標記壞叢集", 1, 1);
            }

            self.report("掃描壞軌", 1, 1);
        }

        // ---- 步驟 12: 更新 FSInfo ----
        self.report("更新 FSInfo", 0, 1);

        let used_clusters = 1 + bad_clusters.len() as u64; // root dir + bad clusters
        let free_clusters = total_clusters.saturating_sub(used_clusters);
        let next_free = if bad_clusters.is_empty() {
            3 // cluster 2 = root dir, next = 3
        } else {
            // 找第一個非壞叢集
            (3u64..)
                .find(|c| !bad_clusters.contains(c))
                .unwrap_or(0xFFFFFFFF)
        };

        let fsinfo_updated = FsInfo {
            free_clusters: free_clusters as u32,
            next_free_cluster: next_free as u32,
            ..FsInfo::new()
        };
        let updated_bytes = fsinfo_updated.to_bytes();

        self.device.write_sectors(bs_lba + FSINFO_SECTOR as u64, &updated_bytes)?;
        self.device.write_sectors(bs_lba + BACKUP_BOOT_SECTOR as u64 + 1, &updated_bytes)?;
        self.report("更新 FSInfo", 1, 1);

        // ---- 步驟 13: 解鎖裝置 ----
        self.report("解鎖裝置", 0, 1);
        self.device.unlock()?;
        self.report("解鎖裝置", 1, 1);

        Ok(())
    }

    /// 消耗格式化器，回傳內部的裝置、設定與進度回呼
    pub fn into_inner(self) -> (D, FormatConfig, P) {
        (self.device, self.config, self.progress)
    }

    // ========================================================================
    // 內部輔助方法
    // ========================================================================

    /// 回報進度，同時檢查取消
    fn report(&mut self, phase: &str, current: u64, total: u64) {
        self.progress.on_progress(phase, current, total);
    }

    /// 設定 FAT 表中指定 cluster 的值（4 bytes, little-endian）
    fn set_fat_entry(fat: &mut [u8], cluster: u64, value: u32) {
        let offset = cluster as usize * 4;
        if offset + 4 <= fat.len() {
            fat[offset] = (value & 0xFF) as u8;
            fat[offset + 1] = ((value >> 8) & 0xFF) as u8;
            fat[offset + 2] = ((value >> 16) & 0xFF) as u8;
            fat[offset + 3] = ((value >> 24) & 0xFF) as u8;
        }
    }

    /// 產生隨機的磁碟區序號
    fn generate_volume_serial() -> u32 {
        use std::time::{SystemTime, UNIX_EPOCH};
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos() as u32)
            .unwrap_or(0);
        // 與 PID 混合增加隨機性
        let pid = std::process::id();
        nanos.wrapping_mul(1103515245).wrapping_add(pid.wrapping_mul(12345))
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device::FileDeviceIO;
    use crate::error::ConsoleProgress;

    use std::fs::File;

    /// 建立一個模擬磁碟的臨時檔案
    fn create_test_disk(name: &str, size_mb: usize) -> std::path::PathBuf {
        let dir = std::env::temp_dir();
        let path = dir.join(name);
        let _ = std::fs::remove_file(&path);

        let size = size_mb * 1024 * 1024;
        let file = File::create(&path).unwrap();
        // 寫入最後一個 byte 讓檔案有正確大小
        file.set_len(size as u64).unwrap();
        path
    }

    #[test]
    fn test_device_too_small() {
        let path = create_test_disk("fat32_fmt_small.bin", 32); // 32MB < 64MB
        let dev = FileDeviceIO::new(path.to_str().unwrap()).unwrap();
        let config = FormatConfig::default();
        let progress = ConsoleProgress::new();
        let mut formatter = Formatter::new(dev, config, progress);

        let result = formatter.run();
        assert!(matches!(result, Err(FormatError::DeviceTooSmall(_))));
    }

    #[test]
    fn test_invalid_cluster_size() {
        let path = create_test_disk("fat32_fmt_badclus.bin", 128);
        let dev = FileDeviceIO::new(path.to_str().unwrap()).unwrap();
        let config = FormatConfig {
            cluster_size: Some(3), // 非 2 的冪
            quick_format: true,
            ..Default::default()
        };
        let progress = ConsoleProgress::new();
        let mut formatter = Formatter::new(dev, config, progress);

        let result = formatter.run();
        assert!(matches!(result, Err(FormatError::InvalidParameter(_))));
    }

    #[test]
    fn test_quick_format_basic() {
        let path = create_test_disk("fat32_fmt_quick.bin", 128); // 128MB
        let dev = FileDeviceIO::new(path.to_str().unwrap()).unwrap();
        let config = FormatConfig {
            label: "TEST".to_string(),
            cluster_size: None, // auto
            quick_format: true,
            ..Default::default()
        };
        let progress = ConsoleProgress::new();
        let mut formatter = Formatter::new(dev, config, progress);

        let result = formatter.run();
        assert!(result.is_ok(), "快速格式化應成功: {:?}", result.err());
    }

    #[test]
    fn test_quick_format_verify_structure() {
        let path = create_test_disk("fat32_fmt_verify.bin", 256);
        let path_str = path.to_str().unwrap().to_string();
        let dev = FileDeviceIO::new(&path_str).unwrap();
        let config = FormatConfig {
            label: "MYDISK".to_string(),
            cluster_size: None,
            quick_format: true,
            ..Default::default()
        };
        let progress = ConsoleProgress::new();
        let mut formatter = Formatter::new(dev, config, progress);
        formatter.run().unwrap();

        // 重新開啟驗證
        let mut dev = FileDeviceIO::new(&path_str).unwrap();

        // 驗證 MBR signature
        let mut mbr_buf = [0u8; 512];
        dev.read_sectors(0, &mut mbr_buf).unwrap();
        let sig = u16::from_le_bytes([mbr_buf[510], mbr_buf[511]]);
        assert_eq!(sig, 0xAA55, "MBR signature 錯誤");

        // 驗證 Boot Sector signature
        let mut bs_buf = [0u8; 512];
        dev.read_sectors(2048, &mut bs_buf).unwrap();
        let bs_sig = u16::from_le_bytes([bs_buf[510], bs_buf[511]]);
        assert_eq!(bs_sig, 0xAA55, "Boot Sector signature 錯誤");

        // 驗證 FSInfo lead signature
        let mut fsi_buf = [0u8; 512];
        dev.read_sectors(2049, &mut fsi_buf).unwrap();
        let lead_sig = u32::from_le_bytes([fsi_buf[0], fsi_buf[1], fsi_buf[2], fsi_buf[3]]);
        assert_eq!(lead_sig, 0x41615252, "FSInfo lead signature 錯誤");
    }

    #[test]
    fn test_full_format_with_scan() {
        let path = create_test_disk("fat32_fmt_full.bin", 128);
        let dev = FileDeviceIO::new(path.to_str().unwrap()).unwrap();
        let config = FormatConfig {
            label: "FULLTEST".to_string(),
            cluster_size: Some(4), // 2KB cluster
            quick_format: false,    // 完整格式化（含壞軌掃描）
            ..Default::default()
        };
        let progress = ConsoleProgress::new();
        let mut formatter = Formatter::new(dev, config, progress);

        let result = formatter.run();
        assert!(result.is_ok(), "完整格式化應成功: {:?}", result.err());
    }

    // ========================================================================
    // 結構完整性測試
    // ========================================================================

    /// 格式化後重新讀取並驗證所有磁碟結構
    fn run_and_verify(name: &str, size_mb: usize, label: &str, spc: Option<u32>, quick: bool) {
        let path = create_test_disk(name, size_mb);
        let path_str = path.to_str().unwrap().to_string();

        // 第一次開啟：執行格式化
        let dev = FileDeviceIO::new(&path_str).unwrap();
        let total_sectors = dev.total_sectors();
        let config = FormatConfig {
            label: label.to_string(),
            cluster_size: spc,
            quick_format: quick,
            ..Default::default()
        };
        let progress = ConsoleProgress::new();
        let mut formatter = Formatter::new(dev, config, progress);
        formatter.run().unwrap();

        // 第二次開啟：驗證結構
        let mut dev = FileDeviceIO::new(&path_str).unwrap();

        // ---- 1. 驗證 MBR ----
        let mut buf = [0u8; 512];
        dev.read_sectors(0, &mut buf).unwrap();
        assert_eq!(u16::from_le_bytes([buf[510], buf[511]]), 0xAA55, "MBR signature");

        // 分割區類型 = 0x0C (FAT32 LBA)
        let part_type = buf[450]; // partition 1 type offset
        assert_eq!(part_type, 0x0C, "MBR partition type 應為 0x0C");

        // 起始 LBA = 2048
        let start_lba = u32::from_le_bytes([buf[454], buf[455], buf[456], buf[457]]);
        assert_eq!(start_lba, 2048, "MBR start LBA 應為 2048");

        // 分割區 sector 數 = total - 2048 (截斷到 u32)
        let expected_part_sectors = (total_sectors - 2048).min(u32::MAX as u64) as u32;
        let part_sectors = u32::from_le_bytes([buf[458], buf[459], buf[460], buf[461]]);
        assert_eq!(part_sectors, expected_part_sectors, "MBR partition sectors");

        // ---- 2. 驗證 Boot Sector ----
        let mut bs = [0u8; 512];
        dev.read_sectors(2048, &mut bs).unwrap();
        assert_eq!(u16::from_le_bytes([bs[510], bs[511]]), 0xAA55, "Boot signature");

        // Jump instruction
        assert_eq!(&bs[0..3], &[0xEB, 0x58, 0x90], "Boot jump instruction");

        // OEM name
        assert_eq!(&bs[3..11], b"MSDOS5.0", "OEM name");

        // Bytes per sector
        let bps = u16::from_le_bytes([bs[11], bs[12]]);
        assert_eq!(bps, 512, "Bytes per sector");

        // Sectors per cluster
        let actual_spc = bs[13];
        assert!(actual_spc > 0 && actual_spc <= 128, "Sectors per cluster: {actual_spc}");
        assert!(actual_spc.is_power_of_two(), "Sectors per cluster 應為 2 的冪: {actual_spc}");

        // Reserved sectors >= 32
        let rs = u16::from_le_bytes([bs[14], bs[15]]);
        assert!(rs >= 32, "Reserved sectors");

        // Number of FATs = 2
        assert_eq!(bs[16], 2, "Number of FATs");

        // Media descriptor = 0xF8
        assert_eq!(bs[21], 0xF8, "Media descriptor");

        // Sectors per FAT (32-bit)
        let spf = u32::from_le_bytes([bs[36], bs[37], bs[38], bs[39]]);
        assert!(spf > 0, "Sectors per FAT 應 > 0: {spf}");

        // Root cluster = 2
        let rc = u32::from_le_bytes([bs[44], bs[45], bs[46], bs[47]]);
        assert_eq!(rc, 2, "Root cluster");

        // FSInfo sector = 1
        let fsi_loc = u16::from_le_bytes([bs[48], bs[49]]);
        assert_eq!(fsi_loc, 1, "FSInfo sector location");

        // Backup boot sector = 6
        let bkp = u16::from_le_bytes([bs[50], bs[51]]);
        assert_eq!(bkp, 6, "Backup boot sector");

        // Extended boot signature = 0x29
        assert_eq!(bs[66], 0x29, "Extended boot signature");

        // Volume label (11 bytes, space-padded)
        let expected_label = if label.is_empty() { "NO NAME    " } else { label };
        let label_bytes: Vec<u8> = expected_label
            .as_bytes()
            .iter()
            .chain(std::iter::repeat(&b' '))
            .take(11)
            .copied()
            .collect();
        assert_eq!(&bs[71..82], label_bytes.as_slice(), "Volume label");

        // FS type = "FAT32   "
        assert_eq!(&bs[82..90], b"FAT32   ", "FS type");

        // ---- 3. 驗證 FSInfo ----
        let mut fsi = [0u8; 512];
        dev.read_sectors(2049, &mut fsi).unwrap();

        let lead = u32::from_le_bytes([fsi[0], fsi[1], fsi[2], fsi[3]]);
        assert_eq!(lead, 0x41615252, "FSInfo lead signature");

        let struc = u32::from_le_bytes([fsi[484], fsi[485], fsi[486], fsi[487]]);
        assert_eq!(struc, 0x61417272, "FSInfo structure signature");

        let trail = u32::from_le_bytes([fsi[508], fsi[509], fsi[510], fsi[511]]);
        assert_eq!(trail, 0xAA550000, "FSInfo trail signature");

        // ---- 4. 驗證備份 Boot Sector (LBA 2048 + 6 = 2054) ----
        let mut bbs = [0u8; 512];
        dev.read_sectors(2054, &mut bbs).unwrap();
        assert_eq!(&bbs[..], &bs[..], "備份 Boot Sector 與主 Boot Sector 應相同");

        // ---- 5. 驗證 FAT 表 ----
        let fat1_lba = 2048 + rs as u64; // partition start + reserved
        let fat2_lba = fat1_lba + spf as u64;
        let fat_size = spf as usize * 512;

        let mut fat1 = vec![0u8; fat_size];
        dev.read_sectors(fat1_lba, &mut fat1).unwrap();
        let mut fat2 = vec![0u8; fat_size];
        dev.read_sectors(fat2_lba, &mut fat2).unwrap();

        // FAT#1 == FAT#2
        assert_eq!(&fat1[..], &fat2[..], "FAT#1 與 FAT#2 應相同");

        // Entry 0: media descriptor (low 8 bits = 0xF8, high bits set)
        let entry0 = u32::from_le_bytes([fat1[0], fat1[1], fat1[2], fat1[3]]);
        assert_eq!(entry0 & 0xFF, 0xF8, "FAT entry 0 media descriptor");

        // Entry 1: EOC
        let entry1 = u32::from_le_bytes([fat1[4], fat1[5], fat1[6], fat1[7]]);
        assert!(entry1 >= 0x0FFFFFF8, "FAT entry 1 應為 EOC");

        // Entry 2 (root dir): EOC
        let entry2 = u32::from_le_bytes([fat1[8], fat1[9], fat1[10], fat1[11]]);
        assert!(entry2 >= 0x0FFFFFF8, "FAT entry 2 (root dir) 應為 EOC");

        // Entry 3+: free (0x00000000)
        if fat_size >= 16 {
            let entry3 = u32::from_le_bytes([fat1[12], fat1[13], fat1[14], fat1[15]]);
            assert_eq!(entry3, 0x00000000, "FAT entry 3 應為 free");
        }

        // ---- 6. 驗證根目錄 ----
        let data_start = 2048 + rs as u64 + spf as u64 * 2;
        let cluster_bytes = actual_spc as usize * 512;
        let mut root = vec![0u8; cluster_bytes];
        dev.read_sectors(data_start, &mut root).unwrap();

        // 第一項應為 volume label
        let attr = root[11];
        assert_eq!(attr, 0x08, "Root dir entry 0 應為 volume label (attr=0x08)");

        // Name 應與 boot sector label 相同
        assert_eq!(&root[0..11], &bs[71..82], "Root dir volume label name");
    }

    #[test]
    fn test_format_structure_small_auto() {
        run_and_verify("fat32_struct_small.bin", 100, "SMALL", None, true);
    }

    #[test]
    fn test_format_structure_medium_manual() {
        run_and_verify("fat32_struct_med.bin", 512, "MEDIUM", Some(8), true);
    }

    #[test]
    fn test_format_structure_large_auto() {
        run_and_verify("fat32_struct_large.bin", 1024, "LARGEDISK", None, true);
    }

    #[test]
    fn test_format_structure_no_label() {
        run_and_verify("fat32_struct_nolbl.bin", 128, "", None, true);
    }

    #[test]
    fn test_format_structure_full_with_scan() {
        run_and_verify("fat32_struct_full.bin", 200, "SCANNED", Some(1), false);
    }
}
