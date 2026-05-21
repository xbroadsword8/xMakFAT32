//! FAT32 格式化工具 — 核心函式庫
//!
//! 提供 FAT32 檔案系統的資料結構定義、MBR 分割表操作、
//! 格式化引擎以及壞軌掃描功能。

pub mod error;
pub mod fat;
pub mod mbr;
pub mod scan;
pub mod format;

pub use error::FormatError;
pub use format::Formatter;
pub use scan::BadBlockScanner;
