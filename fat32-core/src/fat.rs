//! FAT32 磁碟資料結構與輔助函式
//!
//! 定義 FAT32 檔案系統的核心資料結構：
//! - BootSector：開機扇區
//! - FsInfo：檔案系統資訊扇區
//! - DirEntry：目錄項目

use zerocopy::{AsBytes, FromBytes, Unaligned, FromZeroes};

// ============================================================================
// 常數定義
// ============================================================================

/// 每個 sector 的位元組數
pub const BYTES_PER_SECTOR: u16 = 512;

/// 保留的 sector 數量
pub const RESERVED_SECTORS: u16 = 32;

/// FAT 表的數量
pub const NUM_FATS: u8 = 2;

/// 媒體描述符 (0xF8 = 硬碟)
pub const MEDIA_DESCRIPTOR: u8 = 0xF8;

/// Root 目錄的第一個叢集
pub const ROOT_DIR_CLUSTER: u32 = 2;

/// FSInfo 扇區位置
pub const FSINFO_SECTOR: u16 = 1;

/// Backup boot sector 位置
pub const BACKUP_BOOT_SECTOR: u16 = 6;

/// FAT32 版本號
pub const FAT32_VERSION: u16 = 0x0000;

/// Extended boot signature
pub const EXTENDED_BOOT_SIGNATURE: u8 = 0x29;

/// Boot signature
pub const BOOT_SIGNATURE: u16 = 0xAA55;

// FAT 特殊值 (FAT32 只使用低 28 位)
/// End of chain 最小值
pub const FAT32_EOC_MIN: u32 = 0x0FFFFFF8;
/// End of chain 最大值
pub const FAT32_EOC_MAX: u32 = 0x0FFFFFFF;
/// End of chain 值
pub const FAT32_EOC: u32 = 0x0FFFFFFF;
/// Bad cluster 值
pub const FAT32_BAD_CLUSTER: u32 = 0x0FFFFFF7;
/// Free cluster 值
pub const FAT32_FREE_CLUSTER: u32 = 0x00000000;

// FSInfo signatures
/// FSInfo lead signature
pub const FSINFO_LEAD_SIG: u32 = 0x41615252;
/// FSInfo structure signature
pub const FSINFO_STRUC_SIG: u32 = 0x61417272;
/// FSInfo trail signature
pub const FSINFO_TRAIL_SIG: u32 = 0xAA550000;

// DirEntry attribute constants
/// Read-only
pub const ATTR_READ_ONLY: u8 = 0x01;
/// Hidden
pub const ATTR_HIDDEN: u8 = 0x02;
/// System file
pub const ATTR_SYSTEM: u8 = 0x04;
/// Volume ID (volume label)
pub const ATTR_VOLUME_ID: u8 = 0x08;
/// Directory
pub const ATTR_DIRECTORY: u8 = 0x10;
/// Archive
pub const ATTR_ARCHIVE: u8 = 0x20;
/// Long name entry
pub const ATTR_LONG_NAME: u8 = 0x0F;

// ============================================================================
// BootSector (512 bytes)
// ============================================================================

/// FAT32 Boot Sector
#[repr(C, packed)]
#[derive(Debug, Clone, Copy, AsBytes, FromBytes, Unaligned, FromZeroes)]
pub struct BootSector {
    /// Jump instruction (3 bytes)
    pub jmp_instruction: [u8; 3],
    /// OEM name (8 bytes)
    pub oem_name: [u8; 8],
    /// Bytes per sector (offset 11)
    pub bytes_per_sector: u16,
    /// Sectors per cluster (offset 13)
    pub sectors_per_cluster: u8,
    /// Reserved sectors (offset 14)
    pub reserved_sectors: u16,
    /// Number of FATs (offset 16)
    pub num_fats: u8,
    /// Root entries (offset 17) - FAT32 = 0
    pub root_entries: u16,
    /// Total sectors (16-bit, offset 19) - FAT32 = 0
    pub total_sectors_16: u16,
    /// Media descriptor (offset 21)
    pub media_descriptor: u8,
    /// Sectors per FAT (16-bit, offset 22) - FAT32 = 0
    pub sectors_per_fat_16: u16,
    /// Sectors per track (offset 24)
    pub sectors_per_track: u16,
    /// Number of heads (offset 26)
    pub num_heads: u16,
    /// Hidden sectors (offset 28)
    pub hidden_sectors: u32,
    /// Total sectors (32-bit, offset 32)
    pub total_sectors_32: u32,
    /// Sectors per FAT (32-bit, offset 36)
    pub sectors_per_fat_32: u32,
    /// Extended flags (offset 40)
    pub ext_flags: u16,
    /// FS version (offset 42)
    pub fs_version: u16,
    /// Root cluster (offset 44)
    pub root_cluster: u32,
    /// FSInfo sector (offset 48)
    pub fsinfo_sector: u16,
    /// Backup boot sector (offset 50)
    pub backup_boot_sector: u16,
    /// Reserved1 (offset 52)
    pub reserved1: [u8; 12],
    /// Drive number (offset 64)
    pub drive_number: u8,
    /// Reserved2 (offset 65)
    pub reserved2: u8,
    /// Extended boot signature (offset 66)
    pub extended_boot_sig: u8,
    /// Volume serial (offset 67)
    pub volume_serial: u32,
    /// Volume label (offset 71)
    pub volume_label: [u8; 11],
    /// FS type (offset 82)
    pub fs_type: [u8; 8],
    /// Boot code (offset 90)
    pub boot_code: [u8; 420],
    /// Boot signature (offset 510)
    pub boot_signature: u16,
}

impl BootSector {
    /// 建立預設的 FAT32 BootSector
    pub fn new() -> Self {
        // 標準的 FAT32 Boot Stub 機器碼 (印出 "Not a bootable disk..." 並重啟)
        let mut boot_code = [0x00; 420];
        let stub: &[u8] = &[
            0x0E, 0x1F, 0xBE, 0x77, 0x7C, 0xAC, 0x22, 0xC0, 0x74, 0x0B, 0x56, 0xB4, 0x0E, 0xBB, 0x07, 0x00,
            0xCD, 0x10, 0x5E, 0xEB, 0xF0, 0x32, 0xE4, 0xCD, 0x16, 0xCD, 0x19, 0xEB, 0xFE, 0x54, 0x68, 0x69,
            0x73, 0x20, 0x69, 0x73, 0x20, 0x6E, 0x6F, 0x74, 0x20, 0x61, 0x20, 0x62, 0x6F, 0x6F, 0x74, 0x61,
            0x62, 0x6C, 0x65, 0x20, 0x64, 0x69, 0x73, 0x6B, 0x2E, 0x20, 0x20, 0x50, 0x6C, 0x65, 0x61, 0x73,
            0x65, 0x20, 0x69, 0x6E, 0x73, 0x65, 0x72, 0x74, 0x20, 0x61, 0x20, 0x62, 0x6F, 0x6F, 0x74, 0x61,
            0x62, 0x6C, 0x65, 0x20, 0x66, 0x6C, 0x6F, 0x70, 0x70, 0x79, 0x20, 0x61, 0x6E, 0x64, 0x0D, 0x0A,
            0x70, 0x72, 0x65, 0x73, 0x73, 0x20, 0x61, 0x6E, 0x79, 0x20, 0x6B, 0x65, 0x79, 0x20, 0x74, 0x6F,
            0x20, 0x74, 0x72, 0x79, 0x20, 0x61, 0x67, 0x61, 0x69, 0x6E, 0x20, 0x2E, 0x2E, 0x2E, 0x20, 0x0D,
            0x0A, 0x00
        ];
        boot_code[..stub.len()].copy_from_slice(stub);

        BootSector {
            jmp_instruction: [0xEB, 0x58, 0x90],
            oem_name: *b"MSDOS5.0",
            bytes_per_sector: BYTES_PER_SECTOR,
            sectors_per_cluster: 8, // Default 4KB cluster
            reserved_sectors: RESERVED_SECTORS,
            num_fats: NUM_FATS,
            root_entries: 0,
            total_sectors_16: 0,
            media_descriptor: MEDIA_DESCRIPTOR,
            sectors_per_fat_16: 0,
            sectors_per_track: 63,
            num_heads: 255,
            hidden_sectors: 2048,
            total_sectors_32: 0,
            sectors_per_fat_32: 0,
            ext_flags: 0,
            fs_version: FAT32_VERSION,
            root_cluster: ROOT_DIR_CLUSTER,
            fsinfo_sector: FSINFO_SECTOR,
            backup_boot_sector: BACKUP_BOOT_SECTOR,
            reserved1: [0x00; 12],
            drive_number: 0x80,
            reserved2: 0x00,
            extended_boot_sig: EXTENDED_BOOT_SIGNATURE,
            volume_serial: 0,
            volume_label: *b"NO NAME    ",
            fs_type: *b"FAT32   ",
            boot_code,
            boot_signature: BOOT_SIGNATURE,
        }
    }

    /// Builder: 設定總 sector 數
    pub fn total_sectors(mut self, sectors: u32) -> Self {
        self.total_sectors_32 = sectors;
        self
    }

    /// Builder: 設定 Bytes per sector
    pub fn bytes_per_sector(mut self, bytes: u16) -> Self {
        self.bytes_per_sector = bytes;
        self
    }

    /// Builder: 設定保留磁區數量
    pub fn reserved_sectors(mut self, sectors: u16) -> Self {
        self.reserved_sectors = sectors;
        self
    }

    /// Builder: 設定每叢集 sector 數
    pub fn sectors_per_cluster(mut self, sectors: u8) -> Self {
        self.sectors_per_cluster = sectors;
        self
    }

    /// Builder: 設定每 FAT sector 數
    pub fn sectors_per_fat(mut self, sectors: u32) -> Self {
        self.sectors_per_fat_32 = sectors;
        self
    }

    /// Builder: 設定 root cluster
    pub fn root_cluster(mut self, cluster: u32) -> Self {
        self.root_cluster = cluster;
        self
    }

    /// Builder: 設定 volume label
    pub fn volume_label(mut self, label: [u8; 11]) -> Self {
        self.volume_label = label;
        self
    }

    /// Builder: 設定 volume serial
    pub fn volume_serial(mut self, serial: u32) -> Self {
        self.volume_serial = serial;
        self
    }

    /// 序列化為 512 bytes
    pub fn to_bytes(&self) -> [u8; 512] {
        let mut buf = [0u8; 512];
        buf.copy_from_slice(self.as_bytes());
        buf
    }

    /// 從 512 bytes 反序列化
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, &'static str> {
        if bytes.len() < 512 {
            return Err("bytes too short");
        }
        Self::read_from(&bytes[..512]).ok_or("failed to parse BootSector")
    }
}

impl Default for BootSector {
    fn default() -> Self {
        BootSector::new()
    }
}

// ============================================================================
// FsInfo (512 bytes)
// ============================================================================

/// FAT32 FSInfo 扇區
#[repr(C, packed)]
#[derive(Debug, Clone, Copy, AsBytes, FromBytes, Unaligned, FromZeroes)]
pub struct FsInfo {
    /// Lead signature (offset 0)
    pub lead_sig: u32,
    /// Reserved1 (offset 4)
    pub reserved1: [u8; 480],
    /// Structure signature (offset 484)
    pub struc_sig: u32,
    /// Free clusters count (offset 488)
    pub free_clusters: u32,
    /// Next free cluster (offset 492)
    pub next_free_cluster: u32,
    /// Reserved2 (offset 496)
    pub reserved2: [u8; 12],
    /// Trail signature (offset 508)
    pub trail_sig: u32,
}

impl FsInfo {
    /// 建立預設的 FsInfo
    pub fn new() -> Self {
        FsInfo {
            lead_sig: FSINFO_LEAD_SIG,
            reserved1: [0x00; 480],
            struc_sig: FSINFO_STRUC_SIG,
            free_clusters: 0xFFFFFFFF, // Unknown
            next_free_cluster: 0xFFFFFFFF,
            reserved2: [0x00; 12],
            trail_sig: FSINFO_TRAIL_SIG,
        }
    }

    /// 序列化為 512 bytes
    pub fn to_bytes(&self) -> [u8; 512] {
        let mut buf = [0u8; 512];
        buf.copy_from_slice(self.as_bytes());
        buf
    }

    /// 從 512 bytes 反序列化
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, &'static str> {
        if bytes.len() < 512 {
            return Err("bytes too short");
        }
        Self::read_from(&bytes[..512]).ok_or("failed to parse FsInfo")
    }
}

impl Default for FsInfo {
    fn default() -> Self {
        FsInfo::new()
    }
}

// ============================================================================
// DirEntry (32 bytes)
// ============================================================================

/// FAT32 目錄項目
#[repr(C, packed)]
#[derive(Debug, Clone, Copy, AsBytes, FromBytes, Unaligned, FromZeroes)]
#[derive(Default)]
pub struct DirEntry {
    /// 檔名 (8.3 format, offset 0)
    pub name: [u8; 11],
    /// 屬性 (offset 11)
    pub attributes: u8,
    /// NT 保留 (offset 12)
    pub reserved: u8,
    /// 建立時間 (十分秒, offset 13)
    pub creation_time_tenth: u8,
    /// 建立時間 (offset 14)
    pub creation_time: u16,
    /// 建立日期 (offset 16)
    pub creation_date: u16,
    /// 最後存取日期 (offset 18)
    pub last_access_date: u16,
    /// First cluster high (offset 20)
    pub first_cluster_high: u16,
    /// 最後寫入時間 (offset 22)
    pub last_write_time: u16,
    /// 最後寫入日期 (offset 24)
    pub last_write_date: u16,
    /// First cluster low (offset 26)
    pub first_cluster_low: u16,
    /// 檔案大小 (offset 28)
    pub file_size: u32,
}

impl DirEntry {
    /// 建立磁碟區標籤目錄項目
    pub fn new_volume_label(label: [u8; 11]) -> Self {
        DirEntry {
            name: label,
            attributes: ATTR_VOLUME_ID,
            reserved: 0,
            creation_time_tenth: 0,
            creation_time: 0,
            creation_date: 0,
            last_access_date: 0,
            first_cluster_high: 0,
            last_write_time: 0,
            last_write_date: 0,
            first_cluster_low: 0,
            file_size: 0,
        }
    }

    /// 建立一般檔案目錄項目
    pub fn new_file(
        name: [u8; 11],
        attributes: u8,
        first_cluster: u32,
        file_size: u32,
    ) -> Self {
        DirEntry {
            name,
            attributes,
            reserved: 0,
            creation_time_tenth: 0,
            creation_time: 0,
            creation_date: 0,
            last_access_date: 0,
            first_cluster_high: (first_cluster >> 16) as u16,
            last_write_time: 0,
            last_write_date: 0,
            first_cluster_low: (first_cluster & 0xFFFF) as u16,
            file_size,
        }
    }

    /// 建立目錄目錄項目
    pub fn new_directory(
        name: [u8; 11],
        first_cluster: u32,
    ) -> Self {
        DirEntry {
            name,
            attributes: ATTR_DIRECTORY,
            reserved: 0,
            creation_time_tenth: 0,
            creation_time: 0,
            creation_date: 0,
            last_access_date: 0,
            first_cluster_high: (first_cluster >> 16) as u16,
            last_write_time: 0,
            last_write_date: 0,
            first_cluster_low: (first_cluster & 0xFFFF) as u16,
            file_size: 0,
        }
    }

    /// 序列化為 32 bytes
    pub fn to_bytes(&self) -> [u8; 32] {
        let mut buf = [0u8; 32];
        buf.copy_from_slice(self.as_bytes());
        buf
    }

    /// 從 32 bytes 反序列化
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, &'static str> {
        if bytes.len() < 32 {
            return Err("bytes too short");
        }
        Self::read_from(&bytes[..32]).ok_or("failed to parse DirEntry")
    }
}


// ============================================================================
// 輔助函式
// ============================================================================

/// 根據總 sector 數與叢集大小計算每個 FAT 需要多少 sector
/// 
/// `sectors_per_fat = ceil((data_sectors / sec_per_clus + 2) * 4 / bytes_per_sector)`
/// 
/// 其中 `data_sectors = total_sectors - reserved_sectors`
pub fn compute_sectors_per_fat(total_sectors: u64, reserved_sectors: u16, sectors_per_cluster: u8, sector_size: u64) -> u32 {
    let data_sectors = total_sectors - reserved_sectors as u64;
    let total_clusters = data_sectors / sectors_per_cluster as u64;
    
    // 每個 cluster 需要 4 bytes，加上 2 個保留 cluster
    let fat_bytes = (total_clusters + 2) * 4;
    
    // 整數除法無條件進位
    let spf = fat_bytes.div_ceil(sector_size);
    spf as u32
}

/// 根據總 sector 數自動選擇最佳叢集大小
/// 
/// 回傳 `sectors_per_cluster`，必須是 2 的冪。
/// 
/// 參考標準 FAT32 對照表:
/// - 32MB - 256MB: 512B cluster (1 sector)
/// - 256MB - 8GB: 4KB cluster (8 sectors)
/// - 8GB - 16GB: 8KB cluster (16 sectors)
/// - 16GB - 32GB: 16KB cluster (32 sectors)
/// - 32GB - 2TB: 32KB cluster (64 sectors)
pub fn choose_cluster_size(total_sectors: u64, sector_size: u64) -> u8 {
    let total_bytes = total_sectors * sector_size;
    
    // 32MB - 256MB: 512B cluster (1 sector)
    if total_bytes < 256 * 1024 * 1024 {
        return 1;
    }
    
    // 256MB - 8GB: 4KB cluster (8 sectors)
    if total_bytes < 8 * 1024 * 1024 * 1024 {
        return 8;
    }
    
    // 8GB - 16GB: 8KB cluster (16 sectors)
    if total_bytes < 16 * 1024 * 1024 * 1024 {
        return 16;
    }
    
    // 16GB - 32GB: 16KB cluster (32 sectors)
    if total_bytes < 32 * 1024 * 1024 * 1024 {
        return 32;
    }
    
    // 32GB - 2TB: 32KB cluster (64 sectors)
    64
}

/// 驗證磁碟區標籤
/// 
/// - 最多 11 字元
/// - 只能 ASCII 大寫字母、數字、空格
/// - 回傳 `Ok(padded_label)` 其中 label 被空格填補到 11 bytes
pub fn validate_label(label: &str) -> Result<[u8; 11], &'static str> {
    // 空字串 → "NO NAME"
    if label.is_empty() {
        return Ok(*b"NO NAME    ");
    }
    
    // 長度檢查
    if label.len() > 11 {
        return Err("標籤長度超過 11 字元");
    }
    
    // 字元檢查
    for ch in label.chars() {
        if !ch.is_ascii_uppercase() && !ch.is_ascii_digit() && ch != ' ' {
            return Err("標籤含有無效字元 (只能使用大寫英數字元與空格)");
        }
    }
    
    // Padding to 11 bytes
    let mut result = [0x20; 11]; // 0x20 = space
    let bytes = label.as_bytes();
    result[..bytes.len()].copy_from_slice(bytes);
    
    Ok(result)
}

/// 最接近但不超過的 2 的冪
pub fn largest_power_of_two_upto(n: u32) -> u32 {
    if n == 0 {
        return 0;
    }
    
    let mut result = 1;
    while result <= n / 2 {
        result *= 2;
    }
    result
}

/// 確認數字是 2 的冪
pub fn is_power_of_two(n: u32) -> bool {
    n > 0 && (n & (n - 1)) == 0
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_boot_sector_size() {
        assert_eq!(std::mem::size_of::<BootSector>(), 512);
    }

    #[test]
    fn test_fsinfo_size() {
        assert_eq!(std::mem::size_of::<FsInfo>(), 512);
    }

    #[test]
    fn test_dir_entry_size() {
        assert_eq!(std::mem::size_of::<DirEntry>(), 32);
    }

    #[test]
    fn test_compute_sectors_per_fat() {
        // 1GB disk, 32 reserved, 8 sectors per cluster, 512 byte sector
        let sectors = compute_sectors_per_fat(1024 * 1024 * 1024 / 512, 32, 8, 512);
        assert!(sectors > 0);
    }

    #[test]
    fn test_choose_cluster_size() {
        // 100MB -> 1 sector
        assert_eq!(choose_cluster_size(100 * 1024 * 1024 / 512, 512), 1);
        
        // 1GB -> 8 sectors
        assert_eq!(choose_cluster_size(1024 * 1024 * 1024 / 512, 512), 8);
        
        // 100GB -> 64 sectors
        assert_eq!(choose_cluster_size(100 * 1024 * 1024 * 1024 / 512, 512), 64);
    }

    #[test]
    fn test_validate_label() {
        // Empty label
        assert_eq!(validate_label("").unwrap(), *b"NO NAME    ");
        
        // Valid label
        assert_eq!(validate_label("TEST").unwrap(), *b"TEST       ");
        
        // Too long
        assert!(validate_label("THIS IS TOO LONG").is_err());
        
        // Invalid chars
        assert!(validate_label("test!").is_err());
    }

    #[test]
    fn test_largest_power_of_two() {
        assert_eq!(largest_power_of_two_upto(10), 8);
        assert_eq!(largest_power_of_two_upto(16), 16);
        assert_eq!(largest_power_of_two_upto(1), 1);
        assert_eq!(largest_power_of_two_upto(0), 0);
    }

    #[test]
    fn test_is_power_of_two() {
        assert!(is_power_of_two(1));
        assert!(is_power_of_two(2));
        assert!(is_power_of_two(16));
        assert!(!is_power_of_two(3));
        assert!(!is_power_of_two(0));
    }
}
