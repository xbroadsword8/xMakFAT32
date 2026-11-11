//! FAT32 格式化工具 — 命令列版（測試與進階使用）
//!
//! 用法：
//!   fat32-cli --list [--include-virtual]
//!   fat32-cli --format <磁碟編號> [--label NAME] [--cluster-kb 32] [--full] [--include-virtual] --yes

#[cfg(windows)]
#[path = "../winio.rs"]
mod winio;

#[cfg(windows)]
fn main() {
    use fat32_core::{FormatConfig, ProgressCallback};
    use std::io::Write;

    struct Cli { last: String }
    impl ProgressCallback for Cli {
        fn on_progress(&mut self, phase: &str, current: u64, total: u64) {
            let pct = if total > 0 { current * 100 / total } else { 0 };
            let line = format!("{phase} {pct}%");
            if line != self.last {
                eprintln!("{line}");
                self.last = line;
            }
        }
        fn is_cancelled(&self) -> bool { false }
    }

    let args: Vec<String> = std::env::args().collect();
    let has = |f: &str| args.iter().any(|a| a == f);
    let val = |f: &str| args.iter().position(|a| a == f).and_then(|i| args.get(i + 1)).cloned();
    let virt = has("--include-virtual");
    let disks = winio::enumerate_external_disks(virt);

    if has("--list") || args.len() == 1 {
        if disks.is_empty() { println!("找不到外接磁碟"); }
        for d in &disks { println!("{}", d.display()); }
        return;
    }
    let Some(num) = val("--format").and_then(|s| s.parse::<u32>().ok()) else {
        eprintln!("用法：fat32-cli --list | --format <磁碟編號> [--label NAME] [--cluster-kb N] [--full] --yes");
        std::process::exit(2);
    };
    let Some(disk) = disks.iter().find(|d| d.number == num) else {
        eprintln!("磁碟 {num} 不在外接磁碟清單裡（不能格式化內接或系統磁碟）");
        std::process::exit(2);
    };
    if !has("--yes") {
        eprintln!("將清除 {} 的所有資料。確認請加上 --yes", disk.display());
        std::process::exit(2);
    }
    let cluster_size = val("--cluster-kb").and_then(|s| s.parse::<u32>().ok()).map(|kb| kb * 2);
    let config = FormatConfig {
        label: val("--label").unwrap_or_default().to_uppercase(),
        cluster_size,
        quick_format: !has("--full"),
        ..FormatConfig::default()
    };
    let t = std::time::Instant::now();
    match winio::format_disk(disk, config, Cli { last: String::new() }) {
        Ok(()) => println!("完成（{:.1} 秒）", t.elapsed().as_secs_f64()),
        Err(e) => { println!("失敗：{e}"); let _ = std::io::stdout().flush(); std::process::exit(1); }
    }
}

#[cfg(not(windows))]
fn main() {
    eprintln!("此工具僅能在 Windows 上執行");
    std::process::exit(1);
}
