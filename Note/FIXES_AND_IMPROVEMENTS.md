# 修復與效能優化筆記

## 發現的問題
1. **壞軌叢集未去重 (Deduplication Bug)**：在 `format.rs` 中，將壞軌的 LBA 轉換為叢集編號時，未去重直接放入陣列，導致 `bad_clusters.len()` 被錯誤放大，進而導致 `free_clusters` 計算錯誤。
2. **512 Byte 硬編碼 (Hardcoded Sector Size)**：核心邏輯大量依賴 `BYTES_PER_SECTOR = 512`，與實體裝置的 `device.sector_size()` 不一致，若遇到 4Kn 磁碟將導致崩潰。
3. **效能瓶頸：單一磁區讀取**：壞軌掃描每次僅讀取 512 Bytes，這在真實隨身碟上會造成巨大的 I/O 延遲，完整格式化將耗時極長。
4. **架構瑕疵：迴避 Scanner 模組**：開發者因所有權問題，將掃描邏輯寫死在 `format.rs` 內，未使用專為此設計的 `BadBlockScanner`。

## 修復計畫
1. **重構 Scanner 與所有權**：
   修改 `BadBlockScanner::scan`，讓它接受 `&mut D` 而非奪取所有權，或者將 `device` 的所有權於掃描後正確歸還，使 `format.rs` 能夠乾淨地呼叫。
2. **實作分塊讀取 (Chunked Reading)**：
   在 `BadBlockScanner` 中，預設以 1MB (例如 2048 個 512 bytes 磁區) 為單位進行快取讀取。若讀取失敗，才退回針對該區塊進行逐一磁區掃描。
3. **修復叢集去重邏輯**：
   在 `format.rs` 處理壞軌叢集時，使用 `bad_clusters.sort_unstable()` 與 `bad_clusters.dedup()` 去除重複項目，確保每個叢集只被計算一次。
4. **統一磁區大小參數**：
   將所有使用 `BYTES_PER_SECTOR` 的地方替換或支援動態傳入 `sector_size`。對於 FAT 結構（如 BootSector），需允許建立時指定 `bytes_per_sector`。

## 執行紀錄
- 建立此筆記檔案。
- 準備套用修復程式碼。
- 已完成 I/O 效能、所有權與 Bug 修正。

## 擴充規格驗證與深度相容性修復 (對齊成熟標準)
經過進一步查閱微軟官方規範以及成熟產品 (如 Rufus、`mkfs.fat`) 的底層實作，補齊了以下進階規格：
1. **資料區對齊 (Data Area Alignment)**：
   不再硬編碼保留磁區 (Reserved Sectors) 為 32。現在會動態計算 FAT 表大小，並自動擴充/補齊 `reserved_sectors`，確保「資料區」(Data Area) 的起始 LBA 完美對齊叢集 (Cluster) 的實體邊界。這對於 SD 卡與隨身碟的 Flash 寫入放大與效能有極大的幫助。
2. **FSInfo 備份磁區**：
   依據 FAT32 規範，不僅 Boot Sector 需要備份在 LBA 6，FSInfo 也必須備份在 LBA 7。現在已加入同步寫入 LBA 7 備份磁區的邏輯，避免 chkdsk 無法修復受損的 FSInfo。
3. **注入 Boot Stub 程式碼**：
   在 Boot Sector 的 420 bytes `boot_code` 中注入了標準的 x86 組合語言機器碼 (會印出 "This is not a bootable disk... " 並卡住/重啟)，防止不慎由該隨身碟開機時導致 BIOS 崩潰。
4. **MBR LBA 標準指示**：
   將 MBR 內的 `starting_chs` 與 `ending_chs` 修正為 `[0xFE, 0xFF, 0xFF]`，此為向舊版主機板或特殊韌體宣告強制使用 LBA 模式的標準防呆寫法。
