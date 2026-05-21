# FAT32 格式化工具

Windows GUI 工具，可將 ≤2TB USB 隨身硬碟格式化為 FAT32 格式。

## 功能特色

- 支援 32GB ~ 2TB 儲存裝置
- MBR + FAT32 完整檔案系統寫入
- 叢集大小智慧選擇（自動/手動）
- 磁碟區標籤設定
- 壞軌掃描與標記
- 強制卸載已掛載磁碟區
- Windows 原生命令提示字元風格 GUI

## 建置需求

- Rust 1.77+
- Windows SDK（Win32 API）

## 建置

```powershell
cargo build --release
```
