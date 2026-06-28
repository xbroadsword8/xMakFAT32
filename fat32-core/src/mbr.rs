//! MBR 分割表定義與操作
//!
//! 定義主開機記錄 (Master Boot Record) 的資料結構，
//! 包含分割區項目與完整的 512-byte MBR。

use zerocopy::{AsBytes, FromBytes, Unaligned, FromZeroes};

// ============================================================================
// MbrPartitionEntry (16 bytes)
// ============================================================================

/// MBR 分割區項目
#[repr(C, packed)]
#[derive(Debug, Default, Clone, Copy, AsBytes, FromBytes, Unaligned, FromZeroes)]
pub struct MbrPartitionEntry {
    /// 啟動指示符 (0x00 = 非啟動, 0x80 = 啟動)
    pub boot_indicator: u8,
    /// 起始 CHS 地址 (3 bytes)
    pub starting_chs: [u8; 3],
    /// 分割區類型 (0x0C = FAT32 LBA)
    pub partition_type: u8,
    /// 結束 CHS 地址 (3 bytes)
    pub ending_chs: [u8; 3],
    /// 起始 LBA
    pub starting_lba: u32,
    /// 分割區內的 sector 數
    pub sectors_in_partition: u32,
}

impl MbrPartitionEntry {
    /// 建立新的分割區項目
    pub fn new(starting_lba: u32, sectors: u32) -> Self {
        MbrPartitionEntry {
            boot_indicator: 0x00,
            starting_chs: [0xFE, 0xFF, 0xFF], // LBA 模式的標準填充值
            partition_type: 0x0C, // FAT32 LBA
            ending_chs: [0xFE, 0xFF, 0xFF],   // LBA 模式的標準填充值
            starting_lba,
            sectors_in_partition: sectors,
        }
    }
}

// ============================================================================
// Mbr (512 bytes)
// ============================================================================

/// MBR 主開機記錄
#[repr(C, packed)]
#[derive(Debug, Clone, Copy, AsBytes, FromBytes, Unaligned, FromZeroes)]
pub struct Mbr {
    /// Bootstrap code (446 bytes)
    pub bootstrap: [u8; 446],
    /// 分割區表 (4 個項目)
    pub partitions: [MbrPartitionEntry; 4],
    /// Signature (0xAA55)
    pub signature: u16,
}

impl Mbr {
    /// 建立新的 MBR，包含單一 FAT32 分割區
    ///
    /// `partition_sectors` 為分割區內的 sector 總數（不含 MBR 區域）。
    /// 由於 MBR 使用 32-bit 欄位，超過 `u32::MAX` 的值會被截斷。
    pub fn new(partition_sectors: u64) -> Self {
        let mut mbr = Mbr {
            bootstrap: [0x00; 446],
            partitions: [MbrPartitionEntry::default(); 4],
            signature: 0xAA55,
        };

        // 安全轉換：超過 u32::MAX 則截斷
        let sectors = if partition_sectors > u32::MAX as u64 {
            u32::MAX
        } else {
            partition_sectors as u32
        };

        // 只設定第一個分割區
        mbr.partitions[0] = MbrPartitionEntry {
            boot_indicator: 0x80, // 啟動
            partition_type: 0x0C, // FAT32 LBA
            starting_lba: 2048,   // 1MB alignment
            sectors_in_partition: sectors,
            ..Default::default()
        };

        mbr
    }

    /// 序列化為 bytes
    pub fn to_bytes(&self) -> [u8; 512] {
        let mut buf = [0u8; 512];
        buf.copy_from_slice(self.as_bytes());
        buf
    }

    /// 從 bytes 反序列化
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, &'static str> {
        if bytes.len() < 512 {
            return Err("bytes too short");
        }
        Self::read_from(&bytes[..512]).ok_or("failed to parse Mbr")
    }
}

impl Default for Mbr {
    fn default() -> Self {
        Mbr::new(0)
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mbr_partition_entry_size() {
        assert_eq!(std::mem::size_of::<MbrPartitionEntry>(), 16);
    }

    #[test]
    fn test_mbr_size() {
        assert_eq!(std::mem::size_of::<Mbr>(), 512);
    }
}
