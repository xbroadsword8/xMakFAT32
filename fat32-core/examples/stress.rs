use fat32_core::device::FileDeviceIO;
use fat32_core::error::ConsoleProgress;
use fat32_core::format::{FormatConfig, Formatter};
use std::fs::File;
use std::time::Instant;

fn create_test_disk(name: &str, size_mb: usize) -> std::path::PathBuf {
    let dir = std::env::temp_dir();
    let path = dir.join(name);
    let _ = std::fs::remove_file(&path);

    let size = size_mb * 1024 * 1024;
    let file = File::create(&path).unwrap();
    file.set_len(size as u64).unwrap();
    path
}

fn main() {
    println!("開始耐力測試 (1GB 磁碟)...");
    
    // Quick Format Test
    let path = create_test_disk("fat32_stress_quick.bin", 1024);
    let dev = FileDeviceIO::new(path.to_str().unwrap()).unwrap();
    let config = FormatConfig {
        label: "QUICK".to_string(),
        cluster_size: None,
        quick_format: true,
        ..Default::default()
    };
    let progress = ConsoleProgress::new();
    let mut formatter = Formatter::new(dev, config, progress);
    
    let start = Instant::now();
    formatter.run().unwrap();
    let duration = start.elapsed();
    println!("快速格式化耗時: {:?}", duration);

    // Full Format Test
    let path = create_test_disk("fat32_stress_full.bin", 1024);
    let dev = FileDeviceIO::new(path.to_str().unwrap()).unwrap();
    let config = FormatConfig {
        label: "FULL".to_string(),
        cluster_size: None,
        quick_format: false, // 執行壞軌掃描
        ..Default::default()
    };
    let progress = ConsoleProgress::new();
    let mut formatter = Formatter::new(dev, config, progress);
    
    let start = Instant::now();
    formatter.run().unwrap();
    let duration = start.elapsed();
    println!("完整格式化 (含掃描) 耗時: {:?}", duration);
    
    println!("測試完成！");
}
