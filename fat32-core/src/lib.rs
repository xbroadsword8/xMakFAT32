//! FAT32 格式化工具 — 核心函式庫
//!
//! 提供 FAT32 檔案系統的資料結構定義、MBR 分割表操作、
//! 格式化引擎、壞軌掃描功能，以及裝置 I/O 抽象層。

pub mod error;
pub mod fat;
pub mod mbr;
pub mod scan;
pub mod format;
pub mod device;

pub use error::{FormatError, ProgressCallback, ConsoleProgress};
pub use format::{Formatter, FormatConfig};
pub use scan::BadBlockScanner;
pub use device::DeviceIO;
