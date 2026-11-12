//! xMakFAT32 — Windows GUI 應用程式
//!
//! 基於 Win32 API 的原生圖形化介面：選擇外接磁碟（隨身碟、SSD、行動硬碟）、
//! 設定標籤與叢集大小、顯示進度。磁碟 I/O 在 winio.rs。

#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(windows)]
mod winio;

#[cfg(windows)]
#[allow(static_mut_refs)]
mod windows_gui {
    use std::sync::{Arc, Mutex};
    use std::thread;

    use windows::core::{w, PCWSTR};
    use windows::Win32::Foundation::{GetLastError, HWND, LPARAM, LRESULT, WPARAM};
    use windows::Win32::Graphics::Gdi::{
        GetStockObject, UpdateWindow, COLOR_BTNFACE, DEFAULT_GUI_FONT, HBRUSH,
    };
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::Controls::{
        InitCommonControlsEx, ICC_PROGRESS_CLASS, ICC_STANDARD_CLASSES, INITCOMMONCONTROLSEX,
        PBM_SETPOS, PBM_SETRANGE32,
    };
    use windows::Win32::UI::Input::KeyboardAndMouse::EnableWindow;
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetDlgItem, GetMessageW,
        IsDialogMessageW, KillTimer, LoadCursorW, MessageBoxW, PostMessageW, PostQuitMessage,
        RegisterClassExW, SendMessageW, SetTimer, SetWindowTextW, ShowWindow, TranslateMessage,
        BS_AUTOCHECKBOX, BS_PUSHBUTTON, CBS_DROPDOWNLIST, CS_HREDRAW, CS_VREDRAW, CW_USEDEFAULT,
        ES_AUTOHSCROLL, ES_UPPERCASE, HMENU, IDC_ARROW, IDYES, MB_DEFBUTTON2, MB_ICONERROR,
        MB_ICONINFORMATION, MB_ICONWARNING, MB_OK, MB_YESNO, MESSAGEBOX_STYLE, SW_SHOW, WINDOW_STYLE,
        WM_CLOSE, WM_COMMAND, WM_CREATE, WM_DESTROY, WM_SETFONT, WM_TIMER, WM_USER, WNDCLASSEXW,
        WS_BORDER, WS_CAPTION, WS_CHILD, WS_EX_CLIENTEDGE, WS_MINIMIZEBOX, WS_OVERLAPPED,
        WS_SYSMENU, WS_TABSTOP, WS_VISIBLE, WS_VSCROLL,
    };

    use fat32_core::{FormatConfig, ProgressCallback};

    use crate::winio::{self, DiskInfo};

    // ---------------------------------------------------------------- 常數
    const WIN_WIDTH: i32 = 620;
    const WIN_HEIGHT: i32 = 330;

    const ID_COMBO_DRIVE: i32 = 1001;
    const ID_EDIT_LABEL: i32 = 1002;
    const ID_COMBO_CLUSTER: i32 = 1003;
    const ID_CHECK_FULL: i32 = 1004;
    const ID_BUTTON_START: i32 = 1005;
    const ID_BUTTON_REFRESH: i32 = 1006;
    const ID_PROGRESS_BAR: i32 = 1007;
    const ID_STATUS_TEXT: i32 = 1008;

    const TIMER_ID: usize = 1;
    const WM_FORMAT_DONE: u32 = WM_USER + 1;

    // CB_* / BM_* 訊息
    const CB_ADDSTRING: u32 = 0x0143;
    const CB_GETCURSEL: u32 = 0x0147;
    const CB_RESETCONTENT: u32 = 0x014B;
    const CB_SETCURSEL: u32 = 0x014E;
    const BM_GETCHECK: u32 = 0x00F0;
    const EM_SETLIMITTEXT: u32 = 0x00C5;
    const WM_GETTEXT: u32 = 0x000D;

    /// (顯示文字, 每叢集 sector 數)。預設 32KB：RME 建議 DURec 用 32K cluster。
    const CLUSTER_OPTIONS: &[(&str, Option<u32>)] = &[
        ("32 KB（建議，RME 錄音用）", Some(64)),
        ("自動", None),
        ("4 KB", Some(8)),
        ("8 KB", Some(16)),
        ("16 KB", Some(32)),
        ("64 KB", Some(128)),
    ];

    // ---------------------------------------------------------------- 狀態
    #[derive(Default)]
    struct SharedState {
        phase: String,
        current: u64,
        total: u64,
        finished: bool,
        result_msg: Option<String>,
        is_error: bool,
    }

    struct GuiProgress(Arc<Mutex<SharedState>>);

    impl ProgressCallback for GuiProgress {
        fn on_progress(&mut self, phase: &str, current: u64, total: u64) {
            if let Ok(mut s) = self.0.lock() {
                s.phase = phase.to_string();
                s.current = current;
                s.total = total;
            }
        }
        fn is_cancelled(&self) -> bool {
            false
        }
    }

    static mut G_STATE: Option<Arc<Mutex<SharedState>>> = None;
    static mut G_DISKS: Vec<DiskInfo> = Vec::new();
    static mut G_BUSY: bool = false;

    // ---------------------------------------------------------------- 工具
    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    unsafe fn msg_box(hwnd: HWND, text: &str, title: &str, style: MESSAGEBOX_STYLE) -> i32 {
        let t = wide(text);
        let c = wide(title);
        MessageBoxW(hwnd, PCWSTR::from_raw(t.as_ptr()), PCWSTR::from_raw(c.as_ptr()), style).0
    }

    unsafe fn item(hwnd: HWND, id: i32) -> HWND {
        GetDlgItem(hwnd, id).unwrap_or_default()
    }

    unsafe fn set_text(h: HWND, text: &str) {
        let t = wide(text);
        let _ = SetWindowTextW(h, PCWSTR::from_raw(t.as_ptr()));
    }

    unsafe fn add_string(combo: HWND, text: &str) {
        let t = wide(text);
        SendMessageW(combo, CB_ADDSTRING, WPARAM(0), LPARAM(t.as_ptr() as isize));
    }

    unsafe fn child(
        parent: HWND,
        class: PCWSTR,
        text: &str,
        style: WINDOW_STYLE,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        id: i32,
    ) -> HWND {
        let t = wide(text);
        let ex = if class == w!("EDIT") { WS_EX_CLIENTEDGE } else { Default::default() };
        let hwnd = CreateWindowExW(
            ex,
            class,
            PCWSTR::from_raw(t.as_ptr()),
            WS_CHILD | WS_VISIBLE | style,
            x,
            y,
            w,
            h,
            parent,
            HMENU(id as isize as *mut _),
            None,
            None,
        )
        .unwrap_or_default();
        let font = GetStockObject(DEFAULT_GUI_FONT);
        SendMessageW(hwnd, WM_SETFONT, WPARAM(font.0 as usize), LPARAM(1));
        hwnd
    }

    unsafe fn refresh_disks(hwnd: HWND) {
        let combo = item(hwnd, ID_COMBO_DRIVE);
        SendMessageW(combo, CB_RESETCONTENT, WPARAM(0), LPARAM(0));
        let virt = std::env::args().any(|a| a == "--include-virtual");
        G_DISKS = winio::enumerate_external_disks(virt);
        if G_DISKS.is_empty() {
            add_string(combo, "（找不到外接磁碟，請插上後按「重新整理」）");
        }
        for d in G_DISKS.iter() {
            add_string(combo, &d.display());
        }
        SendMessageW(combo, CB_SETCURSEL, WPARAM(0), LPARAM(0));
    }

    // ---------------------------------------------------------------- 視窗程序
    unsafe extern "system" fn wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        match msg {
            WM_CREATE => {
                let st = WINDOW_STYLE(0); // SS_LEFT
                child(hwnd, w!("STATIC"), "目標磁碟：", st, 20, 24, 90, 20, -1);
                child(
                    hwnd,
                    w!("COMBOBOX"),
                    "",
                    WINDOW_STYLE(CBS_DROPDOWNLIST as u32) | WS_VSCROLL | WS_TABSTOP,
                    110,
                    20,
                    380,
                    300,
                    ID_COMBO_DRIVE,
                );
                child(
                    hwnd,
                    w!("BUTTON"),
                    "重新整理",
                    WINDOW_STYLE(BS_PUSHBUTTON as u32) | WS_TABSTOP,
                    500,
                    19,
                    90,
                    26,
                    ID_BUTTON_REFRESH,
                );

                child(hwnd, w!("STATIC"), "磁碟區標籤：", st, 20, 64, 90, 20, -1);
                let edit = child(
                    hwnd,
                    w!("EDIT"),
                    "",
                    WINDOW_STYLE((ES_UPPERCASE | ES_AUTOHSCROLL) as u32) | WS_TABSTOP,
                    110,
                    60,
                    200,
                    24,
                    ID_EDIT_LABEL,
                );
                SendMessageW(edit, EM_SETLIMITTEXT, WPARAM(11), LPARAM(0));
                child(hwnd, w!("STATIC"), "（最多 11 個英數字，可留空）", st, 320, 64, 260, 20, -1);

                child(hwnd, w!("STATIC"), "叢集大小：", st, 20, 104, 90, 20, -1);
                let cc = child(
                    hwnd,
                    w!("COMBOBOX"),
                    "",
                    WINDOW_STYLE(CBS_DROPDOWNLIST as u32) | WS_VSCROLL | WS_TABSTOP,
                    110,
                    100,
                    240,
                    200,
                    ID_COMBO_CLUSTER,
                );
                for (label, _) in CLUSTER_OPTIONS {
                    add_string(cc, label);
                }
                SendMessageW(cc, CB_SETCURSEL, WPARAM(0), LPARAM(0));

                child(
                    hwnd,
                    w!("BUTTON"),
                    "完整格式化（掃描壞軌，耗時很久）",
                    WINDOW_STYLE(BS_AUTOCHECKBOX as u32) | WS_TABSTOP,
                    20,
                    140,
                    300,
                    22,
                    ID_CHECK_FULL,
                );

                child(
                    hwnd,
                    w!("BUTTON"),
                    "開始格式化",
                    WINDOW_STYLE(BS_PUSHBUTTON as u32) | WS_TABSTOP,
                    20,
                    176,
                    140,
                    32,
                    ID_BUTTON_START,
                );

                let bar = child(hwnd, w!("msctls_progress32"), "", WS_BORDER, 20, 222, 570, 22, ID_PROGRESS_BAR);
                SendMessageW(bar, PBM_SETRANGE32, WPARAM(0), LPARAM(1000));
                child(hwnd, w!("STATIC"), "就緒", st, 20, 254, 570, 20, ID_STATUS_TEXT);

                G_STATE = Some(Arc::new(Mutex::new(SharedState::default())));
                refresh_disks(hwnd);
                LRESULT(0)
            }

            WM_COMMAND => {
                match (wparam.0 & 0xFFFF) as i32 {
                    ID_BUTTON_START => start_format(hwnd),
                    ID_BUTTON_REFRESH if !G_BUSY => refresh_disks(hwnd),
                    _ => {}
                }
                LRESULT(0)
            }

            WM_TIMER => {
                update_ui(hwnd);
                LRESULT(0)
            }

            WM_FORMAT_DONE => {
                let _ = KillTimer(hwnd, TIMER_ID);
                update_ui(hwnd);
                G_BUSY = false;
                let _ = EnableWindow(item(hwnd, ID_BUTTON_START), true);
                let _ = EnableWindow(item(hwnd, ID_BUTTON_REFRESH), true);
                let (msg, is_err) = G_STATE
                    .as_ref()
                    .and_then(|s| s.lock().ok().map(|s| (s.result_msg.clone(), s.is_error)))
                    .unwrap_or((None, false));
                if let Some(m) = msg {
                    let icon = if is_err { MB_ICONERROR } else { MB_ICONINFORMATION };
                    msg_box(hwnd, &m, if is_err { "錯誤" } else { "完成" }, MB_OK | icon);
                }
                refresh_disks(hwnd);
                LRESULT(0)
            }

            WM_CLOSE => {
                if G_BUSY {
                    msg_box(
                        hwnd,
                        "格式化進行中，請等它完成再關閉。\n中途關閉會讓磁碟處於無法使用的狀態。",
                        "請稍候",
                        MB_OK | MB_ICONWARNING,
                    );
                } else {
                    let _ = DestroyWindow(hwnd);
                }
                LRESULT(0)
            }

            WM_DESTROY => {
                PostQuitMessage(0);
                LRESULT(0)
            }

            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }

    // ---------------------------------------------------------------- 格式化
    unsafe fn start_format(hwnd: HWND) {
        if G_BUSY {
            return;
        }
        let sel = SendMessageW(item(hwnd, ID_COMBO_DRIVE), CB_GETCURSEL, WPARAM(0), LPARAM(0)).0;
        let Some(disk) = (sel >= 0).then(|| G_DISKS.get(sel as usize).cloned()).flatten() else {
            msg_box(hwnd, "請先選擇目標磁碟", "錯誤", MB_OK | MB_ICONERROR);
            return;
        };
        if disk.size_bytes > winio::MAX_DISK_BYTES {
            msg_box(hwnd, "這顆磁碟超過 2TB，FAT32 無法使用", "錯誤", MB_OK | MB_ICONERROR);
            return;
        }

        let csel = SendMessageW(item(hwnd, ID_COMBO_CLUSTER), CB_GETCURSEL, WPARAM(0), LPARAM(0)).0;
        let cluster_size = CLUSTER_OPTIONS.get(csel.max(0) as usize).and_then(|c| c.1);

        let mut buf = [0u16; 32];
        let n = SendMessageW(item(hwnd, ID_EDIT_LABEL), WM_GETTEXT, WPARAM(buf.len()), LPARAM(buf.as_mut_ptr() as isize)).0;
        let label = String::from_utf16_lossy(&buf[..n.max(0) as usize]).trim().to_uppercase();
        if !label.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || " _-".contains(c)) {
            msg_box(hwnd, "標籤只能用英文字母、數字、空格、- 和 _", "錯誤", MB_OK | MB_ICONERROR);
            return;
        }
        let full = SendMessageW(item(hwnd, ID_CHECK_FULL), BM_GETCHECK, WPARAM(0), LPARAM(0)).0 == 1;

        let confirm = format!(
            "即將清除這顆磁碟上的「所有資料」，無法復原：\n\n{}\n\n確定要格式化嗎？",
            disk.display()
        );
        if msg_box(hwnd, &confirm, "確認格式化", MB_YESNO | MB_ICONWARNING | MB_DEFBUTTON2) != IDYES.0 {
            return;
        }

        let Some(state) = G_STATE.clone() else { return };
        *state.lock().unwrap() = SharedState::default();
        G_BUSY = true;
        let _ = EnableWindow(item(hwnd, ID_BUTTON_START), false);
        let _ = EnableWindow(item(hwnd, ID_BUTTON_REFRESH), false);
        SendMessageW(item(hwnd, ID_PROGRESS_BAR), PBM_SETPOS, WPARAM(0), LPARAM(0));
        SetTimer(hwnd, TIMER_ID, 100, None);

        let hwnd_raw = hwnd.0 as isize; // HWND 不是 Send，用整數傳過去
        thread::spawn(move || {
            let config = FormatConfig { label, cluster_size, quick_format: !full, ..FormatConfig::default() };
            let result = winio::format_disk(&disk, config, GuiProgress(state.clone()));
            {
                let mut s = state.lock().unwrap();
                s.finished = true;
                match result {
                    Ok(()) => {
                        s.result_msg = Some(format!(
                            "格式化完成！\n\n{}\n已格式化為 FAT32（MBR、單一分割區）。",
                            disk.display()
                        ));
                    }
                    Err(e) => {
                        s.is_error = true;
                        s.result_msg = Some(format!("格式化失敗：{e}"));
                    }
                }
            }
            // PostMessage（非 SendMessage）：不阻塞，也不會和 UI 執行緒互鎖
            let _ = PostMessageW(HWND(hwnd_raw as *mut _), WM_FORMAT_DONE, WPARAM(0), LPARAM(0));
        });
    }

    unsafe fn update_ui(hwnd: HWND) {
        let Some(state) = G_STATE.as_ref() else { return };
        let (phase, cur, total, finished, is_err) = {
            let s = state.lock().unwrap();
            (s.phase.clone(), s.current, s.total, s.finished, s.is_error)
        };
        let pos = if finished && !is_err { 1000 } else if total > 0 { cur * 1000 / total } else { 0 };
        SendMessageW(item(hwnd, ID_PROGRESS_BAR), PBM_SETPOS, WPARAM(pos as usize), LPARAM(0));
        let text = if finished {
            if is_err { "失敗".to_string() } else { "完成".to_string() }
        } else if phase.is_empty() {
            "準備中...".to_string()
        } else if total > 1 {
            format!("{phase}：{}%", cur * 100 / total)
        } else {
            format!("{phase}...")
        };
        set_text(item(hwnd, ID_STATUS_TEXT), &text);
    }

    // ---------------------------------------------------------------- 進入點
    pub fn run() {
        unsafe {
            let icc = INITCOMMONCONTROLSEX {
                dwSize: std::mem::size_of::<INITCOMMONCONTROLSEX>() as u32,
                dwICC: ICC_STANDARD_CLASSES | ICC_PROGRESS_CLASS,
            };
            let _ = InitCommonControlsEx(&icc);

            let instance = GetModuleHandleW(None).unwrap_or_default();
            let class_name = w!("FAT32FormatterWindow");
            let wc = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                style: CS_HREDRAW | CS_VREDRAW,
                lpfnWndProc: Some(wnd_proc),
                hInstance: instance.into(),
                hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
                hbrBackground: HBRUSH((COLOR_BTNFACE.0 + 1) as isize as *mut _),
                lpszClassName: class_name,
                ..Default::default()
            };
            if RegisterClassExW(&wc) == 0 {
                return;
            }

            let hwnd = match CreateWindowExW(
                Default::default(),
                class_name,
                w!("xMakFAT32 — FAT32 格式化工具"),
                WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                WIN_WIDTH,
                WIN_HEIGHT,
                None,
                None,
                instance,
                None,
            ) {
                Ok(h) => h,
                Err(_) => {
                    let _ = GetLastError();
                    return;
                }
            };

            let _ = ShowWindow(hwnd, SW_SHOW);
            let _ = UpdateWindow(hwnd);

            let mut msg = std::mem::zeroed();
            while GetMessageW(&mut msg, None, 0, 0).0 > 0 {
                if !IsDialogMessageW(hwnd, &msg).as_bool() {
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }
        }
    }
}

#[cfg(windows)]
fn main() {
    windows_gui::run();
}

#[cfg(not(windows))]
fn main() {
    eprintln!("錯誤：此應用程式僅能在 Windows 上執行。");
    std::process::exit(1);
}
