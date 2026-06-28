//! FAT32 格式化錯誤類型

use std::fmt;

/// FAT32 格式化相關錯誤
#[derive(Debug)]
pub enum FormatError {
    /// 無法開啟裝置（權限不足、不存在或已被佔用）
    DeviceOpen(String),
    /// 裝置太小（< 64MB 無法格式化為 FAT32）
    DeviceTooSmall(u64),
    /// 裝置超過 2TB 限制
    DeviceTooLarge(u64),
    /// 無法鎖定裝置（卸載失敗）
    LockFailed(String),
    /// 讀寫失敗
    Io(std::io::Error),
    /// 無效的參數（叢集大小、標籤等）
    InvalidParameter(String),
    /// 壞軌數超過容忍上限
    TooManyBadBlocks { total: u64, bad: u64, threshold: f64 },
    /// 磁碟區標籤無效（長度或字元限制）
    InvalidLabel(String),
    /// MBR 寫入失敗
    MbrWrite(String),
    /// Boot sector 寫入失敗
    BootSectorWrite(String),
    /// FAT 表寫入失敗
    FatWrite(String),
    /// 檔案系統結構不一致
    Inconsistent(String),
    /// 取消操作
    Cancelled,
}

impl fmt::Display for FormatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FormatError::DeviceOpen(s) => write!(f, "無法開啟裝置: {s}"),
            FormatError::DeviceTooSmall(size) => {
                write!(f, "裝置太小 ({size} bytes)，FAT32 需要至少 64MB")
            }
            FormatError::DeviceTooLarge(size) => {
                write!(f, "裝置過大 ({size} bytes)，FAT32 最大支援 2TB")
            }
            FormatError::LockFailed(s) => write!(f, "無法鎖定/卸載裝置: {s}"),
            FormatError::Io(e) => write!(f, "I/O 錯誤: {e}"),
            FormatError::InvalidParameter(s) => write!(f, "無效參數: {s}"),
            FormatError::TooManyBadBlocks { total, bad, threshold } => {
                let pct = *bad as f64 / *total as f64 * 100.0;
                write!(f, "壞軌過多 ({bad}/{total}, {pct:.1}%)，超過 {:.0}% 容忍上限", threshold * 100.0)
            }
            FormatError::InvalidLabel(s) => write!(f, "無效磁碟區標籤: {s}"),
            FormatError::MbrWrite(s) => write!(f, "MBR 寫入失敗: {s}"),
            FormatError::BootSectorWrite(s) => write!(f, "Boot sector 寫入失敗: {s}"),
            FormatError::FatWrite(s) => write!(f, "FAT 表寫入失敗: {s}"),
            FormatError::Inconsistent(s) => write!(f, "檔案系統不一致: {s}"),
            FormatError::Cancelled => write!(f, "操作已取消"),
        }
    }
}

impl std::error::Error for FormatError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            FormatError::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for FormatError {
    fn from(e: std::io::Error) -> Self {
        FormatError::Io(e)
    }
}

/// 格式化進度回呼
pub trait ProgressCallback: Send {
    fn on_progress(&mut self, phase: &str, current: u64, total: u64);
    fn is_cancelled(&self) -> bool;
}

/// 一個簡單的進度回呼實作（用於測試或 CLI）
pub struct ConsoleProgress {
    cancelled: std::sync::atomic::AtomicBool,
}

impl Default for ConsoleProgress {
    fn default() -> Self {
        Self::new()
    }
}

impl ConsoleProgress {
    pub fn new() -> Self {
        ConsoleProgress {
            cancelled: std::sync::atomic::AtomicBool::new(false),
        }
    }

    pub fn cancel(&self) {
        self.cancelled
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }
}

impl ProgressCallback for ConsoleProgress {
    fn on_progress(&mut self, phase: &str, current: u64, total: u64) {
        let pct = if total > 0 {
            current as f64 / total as f64 * 100.0
        } else {
            0.0
        };
        eprintln!("[{phase}] {current}/{total} ({pct:.1}%)");
    }

    fn is_cancelled(&self) -> bool {
        self.cancelled.load(std::sync::atomic::Ordering::SeqCst)
    }
}
