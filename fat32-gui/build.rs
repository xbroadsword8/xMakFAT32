//! 把 app.manifest（要求系統管理員權限、Common Controls v6）嵌進 Windows 執行檔。
//! 只在 GNU 工具鏈（mingw windres）下處理；其他情況略過並提示。
use std::{env, path::PathBuf, process::Command};

fn main() {
    println!("cargo:rerun-if-changed=app.rc");
    println!("cargo:rerun-if-changed=app.manifest");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    if env::var("CARGO_CFG_TARGET_ENV").as_deref() != Ok("gnu") {
        println!("cargo:warning=非 GNU 工具鏈：未嵌入 manifest，執行時請手動「以系統管理員身分執行」");
        return;
    }
    let out = PathBuf::from(env::var("OUT_DIR").unwrap()).join("app_res.o");
    let windres = if cfg!(windows) { "windres" } else { "x86_64-w64-mingw32-windres" };
    let ok = Command::new(windres)
        .args(["app.rc", "-O", "coff", "-o"])
        .arg(&out)
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if ok {
        println!("cargo:rustc-link-arg-bins={}", out.display());
    } else {
        println!("cargo:warning=找不到 windres：未嵌入 manifest");
    }
}
