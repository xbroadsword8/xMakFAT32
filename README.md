# FAT32 格式化工具

在 Windows 上把 USB 隨身碟、外接 SSD（最大 2TB）格式化成 **FAT32** 的小工具。

## 為什麼做這個工具

很多老舊器材（錄音介面、數位錄音機、相機、車用音響、遊戲機、工業設備等）**只讀得懂 FAT32**。但 Windows 內建的格式化工具不允許把 32GB 以上的磁碟格式化成 FAT32，只給 exFAT 或 NTFS。現在的隨身碟和 SSD 幾乎都超過 32GB，這些器材就沒辦法用。這個工具就是為了解決這個問題。

它會把整顆磁碟做成：

- **MBR** 分割表，**單一分割區**（類型 0x0C，從 1MB 處開始對齊）
- **FAT32**，叢集大小可選（預設 32KB）
- 同時清除舊的 GPT 表頭與磁碟尾端的備份 GPT，避免器材誤判

## ✅ RME DURec 實測可用

**RME UFX+ 的 DURec（直接錄音到 USB）實測可用**：1TB SanDisk Portable SSD 用本工具格式化後，UFX+ 能正常讀取並錄音。

DURec 對磁碟格式的要求和本工具的預設值一致：MBR、FAT32、使用第一個分割區、建議 32KB 叢集。UFX II、UFX III 等其他支援 DURec 的 RME 介面，格式要求相同，但尚未實測。

> ⚠️ **格式化會清除整顆磁碟的所有資料**，請先備份，並在選單中確認選對磁碟。

## 下載與使用

從 [Releases](https://github.com/xbroadsword8/fat32-formatter/releases) 下載 zip，解壓縮後：

1. 插上要格式化的磁碟。
2. 雙擊 `fat32-gui.exe`，Windows 會詢問是否允許變更（需要系統管理員權限才能寫入磁碟），按「是」。
3. 在「目標磁碟」選擇磁碟。清單只列出 USB／SD 外接磁碟，**不會列出系統碟**；沒看到就按「重新整理」。
4. 叢集大小用預設 32KB 即可；標籤可留空（英文大寫、數字，最多 11 字元）。
5. 按「開始格式化」，在確認框確認磁碟後按「是」。快速格式化只需幾秒。

執行檔沒有程式碼簽章，第一次執行時 Windows SmartScreen 可能出現警告，按「其他資訊 → 仍要執行」。

### 命令列版

`fat32-cli.exe` 使用同一套格式化程式碼，適合腳本或遠端操作（需在系統管理員終端機執行）：

```text
fat32-cli --list                       列出可格式化的磁碟
fat32-cli --format <磁碟編號> --yes    格式化
    [--label NAME]                     磁碟區標籤
    [--cluster-kb N]                   叢集大小（KB，預設自動；RME 建議 32）
    [--full]                           完整格式化（掃描壞軌，耗時很久）
```

## 限制

- 容量 64MB～2TB（MBR／FAT32 的上限）；目前只在 512-byte 磁區的磁碟上測試過。
- FAT32 單一檔案最大 4GB（檔案系統本身的限制）。
- 只支援 Windows 10／11（x64）。

## 建置

需要 Rust 1.77 以上。在 Windows 上：

```powershell
cargo build --release
```

或在 Linux 交叉編譯（需安裝 `mingw-w64`，`rustup target add x86_64-pc-windows-gnu`）：

```bash
cargo build --release --target x86_64-pc-windows-gnu
```

產出 `fat32-gui.exe` 與 `fat32-cli.exe`。`cargo test` 執行核心引擎（`fat32-core`）的測試。

### 專案結構

| 目錄 | 內容 |
|---|---|
| `fat32-core/` | 格式化引擎：MBR、FAT32 結構、壞軌掃描（不依賴 Windows，可在任何平台測試） |
| `fat32-gui/src/winio.rs` | Windows 磁碟存取：列舉磁碟、鎖定／卸載磁碟區、`\\.\PhysicalDriveN` 讀寫 |
| `fat32-gui/src/main.rs` | Win32 視窗介面 |
| `fat32-gui/src/bin/fat32-cli.rs` | 命令列介面 |

## 授權

[PolyForm Noncommercial License 1.0.0](LICENSE)：**可免費用於非商業用途**，包含個人、教學、研究、學校與非營利團體；可以修改、再散布，但必須附上授權條款。**不可用於商業用途**，需要商業授權請透過 GitHub Issues 聯絡作者。

本軟體依「現狀」提供，不負任何擔保；因使用本軟體造成的資料遺失，作者不負責任。

RME、UFX+、DURec 是 RME Audio 的商標，Windows 是 Microsoft 的商標，本專案與這兩家公司無關。
