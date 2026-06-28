//! FAT32 格式化工具 — Windows GUI 應用程式
//!
//! 基於 Win32 API 的原生圖形化介面。
//! 提供磁碟選擇、格式化參數設定、進度顯示與取消操作。

// ============================================================================
// 條件編譯：Windows 實作 vs 非 Windows stub
// ============================================================================

#[cfg(windows)]
mod windows_gui {
    use std::sync::{Arc, Mutex};
    use std::thread;

    use windows::core::{PCWSTR, s};
    use windows::Win32::Foundation::{
        CloseHandle, GetLastError, HANDLE, HWND, LPARAM, LRESULT, WPARAM,
    };
    use windows::Win32::Graphics::Gdi::{
        GetStockObject, GRAY_BRUSH,
    };
    use windows::Win32::Storage::FileSystem::{
        CreateFileW, GetDiskFreeSpaceW, GetDriveTypeW, GetLogicalDriveStringsW,
        ReadFile, WriteFile, SetFilePointerEx,
        FILE_SHARE_READ, FILE_SHARE_WRITE,
        OPEN_EXISTING, FILE_ATTRIBUTE_NORMAL, DRIVE_REMOVABLE,
        FILE_BEGIN,
    };
    use windows::Win32::System::IO::{
        DeviceIoControl, FSCTL_LOCK_VOLUME, FSCTL_UNLOCK_VOLUME,
        IOCTL_STORAGE_GET_DEVICE_NUMBER, STORAGE_DEVICE_NUMBER,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW,
        LoadCursorW, PostQuitMessage, RegisterClassExW, SetTimer, KillTimer,
        ShowWindow, UpdateWindow, SendMessageW, EnableWindow, SetWindowTextW,
        MessageBoxW, GetDlgItem,
        CW_USEDEFAULT, CS_HREDRAW, CS_VREDRAW, IDC_ARROW, MB_ICONERROR, MB_ICONINFORMATION, MB_OK,
        SW_SHOW, WM_CLOSE, WM_COMMAND, WM_CREATE, WM_CTLCOLORSTATIC, WM_DESTROY,
        WM_TIMER, WM_USER, WNDCLASSEXW, WS_BORDER, WS_CHILD,
        WS_TABSTOP, WS_VISIBLE, WS_VSCROLL,
        BS_CHECKBOX, BS_PUSHBUTTON,
        CBS_DROPDOWNLIST, ES_LEFT, ES_UPPERCASE, PBS_SMOOTH, SS_LEFT,
    };
    use windows::Win32::UI::Controls::{
        PBM_SETPOS, PBM_SETRANGE32, InitCommonControlsEx, INITCOMMONCONTROLSEX,
        ICC_PROGRESS_CLASS, ICC_STANDARD_CLASSES,
    };
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;

    use fat32_core::{
        DeviceIO, Formatter, FormatConfig, FormatError, ProgressCallback,
    };

    // ========================================================================
    // 常數與控制項 ID
    // ========================================================================

    const WIN_WIDTH: i32 = 500;
    const WIN_HEIGHT: i32 = 380;

    const ID_COMBO_DRIVE: u16 = 1001;
    const ID_EDIT_LABEL: u16 = 1002;
    const ID_COMBO_CLUSTER: u16 = 1003;
    const ID_CHECK_QUICK: u16 = 1004;
    const ID_BUTTON_START: u16 = 1005;
    const ID_BUTTON_CANCEL: u16 = 1006;
    const ID_PROGRESS_BAR: u16 = 1007;
    const ID_STATUS_TEXT: u16 = 1008;

    const TIMER_ID: usize = 1;
    const TIMER_INTERVAL_MS: u32 = 100;

    const WM_FORMAT_DONE: u32 = WM_USER + 1;

    // ========================================================================
    // SharedState & GuiProgress
    // ========================================================================

    #[derive(Debug, Clone)]
    struct SharedState {
        phase: String,
        current: u64,
        total: u64,
        cancelled: bool,
        finished: bool,
        result_msg: Option<String>,
        is_error: bool,
    }

    impl SharedState {
        fn new() -> Self {
            SharedState {
                phase: String::new(),
                current: 0,
                total: 0,
                cancelled: false,
                finished: false,
                result_msg: None,
                is_error: false,
            }
        }
    }

    struct GuiProgress {
        state: Arc<Mutex<SharedState>>,
    }

    impl GuiProgress {
        fn new(state: Arc<Mutex<SharedState>>) -> Self {
            GuiProgress { state }
        }
    }

    impl ProgressCallback for GuiProgress {
        fn on_progress(&mut self, phase: &str, current: u64, total: u64) {
            if let Ok(mut s) = self.state.lock() {
                s.phase = phase.to_string();
                s.current = current;
                s.total = total;
            }
        }

        fn is_cancelled(&self) -> bool {
            self.state
                .lock()
                .map(|s| s.cancelled)
                .unwrap_or(false)
        }
    }

    // ========================================================================
    // WindowsDeviceIO
    // ========================================================================

    struct WindowsDeviceIO {
        handle: HANDLE,
        sector_size: u64,
        total_sectors: u64,
    }

    impl WindowsDeviceIO {
        fn new(path: &str) -> Result<Self, FormatError> {
            let path_wide: Vec<u16> = path
                .encode_utf16()
                .chain(std::iter::once(0))
                .collect();

            let handle = unsafe {
                CreateFileW(
                    PCWSTR::from_raw(path_wide.as_ptr()),
                    0xC0000000, // GENERIC_READ | GENERIC_WRITE
                    FILE_SHARE_READ | FILE_SHARE_WRITE,
                    None,
                    OPEN_EXISTING,
                    FILE_ATTRIBUTE_NORMAL,
                    HANDLE::default(),
                )
            };

            if handle.is_invalid() {
                let err = unsafe { GetLastError() };
                return Err(FormatError::DeviceOpen(format!(
                    "無法開啟裝置 {path} (錯誤碼: {err:?})"
                )));
            }

            let sector_size: u64 = 512;
            let total_sectors = Self::get_disk_size(handle)?;

            Ok(WindowsDeviceIO {
                handle,
                sector_size,
                total_sectors,
            })
        }

        fn get_disk_size(handle: HANDLE) -> Result<u64, FormatError> {
            // 使用 SetFilePointerEx + FILE_END 取得裝置大小
            let mut size: i64 = 0;
            unsafe {
                SetFilePointerEx(handle, 0, Some(&mut size), 2) // FILE_END = 2
                    .map_err(|_| FormatError::DeviceOpen(
                        "無法查詢磁碟大小，請確認有管理員權限".into()
                    ))?;
            }
            Ok(size as u64 / 512)
        }
    }

    impl DeviceIO for WindowsDeviceIO {
        fn sector_size(&self) -> u64 {
            self.sector_size
        }

        fn total_sectors(&self) -> u64 {
            self.total_sectors
        }

        fn read_sectors(&mut self, lba: u64, buf: &mut [u8]) -> Result<(), FormatError> {
            let offset = (lba * self.sector_size) as i64;
            unsafe {
                let mut pos = offset;
                SetFilePointerEx(self.handle, offset, Some(&mut pos), FILE_BEGIN)
                    .map_err(|e| FormatError::Io(
                        std::io::Error::from_raw_os_error(e.code().0 as i32)
                    ))?;

                let mut bytes_read: u32 = 0;
                ReadFile(self.handle, Some(buf), Some(&mut bytes_read), None)
                    .map_err(|e| FormatError::Io(
                        std::io::Error::from_raw_os_error(e.code().0 as i32)
                    ))?;

                if bytes_read as usize != buf.len() {
                    return Err(FormatError::Io(std::io::Error::new(
                        std::io::ErrorKind::UnexpectedEof,
                        format!("只讀到 {bytes_read} bytes，預期 {}", buf.len()),
                    )));
                }
            }
            Ok(())
        }

        fn write_sectors(&mut self, lba: u64, buf: &[u8]) -> Result<(), FormatError> {
            let offset = (lba * self.sector_size) as i64;
            unsafe {
                let mut pos = offset;
                SetFilePointerEx(self.handle, offset, Some(&mut pos), FILE_BEGIN)
                    .map_err(|e| FormatError::Io(
                        std::io::Error::from_raw_os_error(e.code().0 as i32)
                    ))?;

                let mut bytes_written: u32 = 0;
                WriteFile(self.handle, buf, Some(&mut bytes_written), None)
                    .map_err(|e| FormatError::Io(
                        std::io::Error::from_raw_os_error(e.code().0 as i32)
                    ))?;

                if bytes_written as usize != buf.len() {
                    return Err(FormatError::Io(std::io::Error::new(
                        std::io::ErrorKind::WriteZero,
                        format!("只寫入 {bytes_written} bytes，預期 {}", buf.len()),
                    )));
                }
            }
            Ok(())
        }

        fn lock(&mut self) -> Result<(), FormatError> {
            unsafe {
                let mut _bytes: u32 = 0;
                DeviceIoControl(
                    self.handle, FSCTL_LOCK_VOLUME,
                    None, 0, None, 0,
                    Some(&mut _bytes), None,
                )
                .map_err(|e| FormatError::LockFailed(format!("鎖定失敗 ({e:?})")))?;
            }
            Ok(())
        }

        fn unlock(&mut self) -> Result<(), FormatError> {
            unsafe {
                let mut _bytes: u32 = 0;
                DeviceIoControl(
                    self.handle, FSCTL_UNLOCK_VOLUME,
                    None, 0, None, 0,
                    Some(&mut _bytes), None,
                )
                .map_err(|e| FormatError::LockFailed(format!("解鎖失敗 ({e:?})")))?;
            }
            Ok(())
        }
    }

    impl Drop for WindowsDeviceIO {
        fn drop(&mut self) {
            if !self.handle.is_invalid() {
                unsafe { let _ = CloseHandle(self.handle); }
            }
        }
    }

    // ========================================================================
    // 輔助函式
    // ========================================================================

    fn to_wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn enumerate_removable_drives() -> Vec<(String, String)> {
        let mut drives = Vec::new();
        let mut buf = vec![0u16; 512];

        let len = unsafe { GetLogicalDriveStringsW(Some(&mut buf)) };
        if len == 0 {
            return drives;
        }

        let buf = &buf[..len as usize];
        let mut start = 0;
        for i in 0..buf.len() {
            if buf[i] == 0 && i > start {
                let drive_str = String::from_utf16_lossy(&buf[start..i]);
                if drive_str.len() >= 2 {
                    let root = &drive_str[..3];
                    let root_wide = to_wide(root);
                    let dtype = unsafe {
                        GetDriveTypeW(PCWSTR::from_raw(root_wide.as_ptr()))
                    };
                    if dtype == DRIVE_REMOVABLE {
                        let label = format!("{} (可移除)", &drive_str[..2]);
                        drives.push((drive_str[..2].to_string(), label));
                    }
                }
                start = i + 1;
            }
        }

        drives
    }

    fn get_physical_drive_number(drive_letter: &str) -> Option<u32> {
        let path = format!("\\\\.\\{drive_letter}");
        let path_wide = to_wide(&path);

        let handle = unsafe {
            CreateFileW(
                PCWSTR::from_raw(path_wide.as_ptr()),
                0,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                None,
                OPEN_EXISTING,
                0,
                HANDLE::default(),
            )
        };

        if handle.is_invalid() {
            return None;
        }

        let mut dev_num = STORAGE_DEVICE_NUMBER::default();
        let mut bytes_returned: u32 = 0;

        let result = unsafe {
            DeviceIoControl(
                handle,
                IOCTL_STORAGE_GET_DEVICE_NUMBER,
                None, 0,
                Some(&mut dev_num as *mut _ as *mut std::ffi::c_void),
                std::mem::size_of::<STORAGE_DEVICE_NUMBER>() as u32,
                Some(&mut bytes_returned),
                None,
            )
        };

        unsafe { let _ = CloseHandle(handle); }

        if result.is_ok() && bytes_returned > 0 {
            Some(dev_num.DeviceNumber)
        } else {
            None
        }
    }

    // 叢集大小選項
    const CLUSTER_OPTIONS: &[(&str, Option<u32>)] = &[
        ("自動", None),
        ("512 B", Some(1)),
        ("1 KB", Some(2)),
        ("2 KB", Some(4)),
        ("4 KB", Some(8)),
        ("8 KB", Some(16)),
        ("16 KB", Some(32)),
        ("32 KB", Some(64)),
        ("64 KB", Some(128)),
    ];

    // ========================================================================
    // 全域狀態
    // ========================================================================

    static mut G_HWND_PROGRESS: Option<HWND> = None;
    static mut G_HWND_STATUS: Option<HWND> = None;
    static mut G_HWND_START: Option<HWND> = None;
    static mut G_HWND_CANCEL: Option<HWND> = None;
    static mut G_HWND_COMBO_DRIVE: Option<HWND> = None;
    static mut G_SHARED_STATE: Option<Arc<Mutex<SharedState>>> = None;
    static mut G_FORMAT_THREAD: Option<thread::JoinHandle<()>> = None;

    // ========================================================================
    // 視窗程序
    // ========================================================================

    unsafe extern "system" fn wnd_proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        match msg {
            WM_CREATE => {
                let instance = std::mem::zeroed();

                // --- 目標裝置標籤 ---
                CreateWindowExW(
                    Default::default(), s!("STATIC"), s!("目標裝置:"),
                    WS_CHILD | WS_VISIBLE | SS_LEFT,
                    20, 22, 100, 20,
                    hwnd, None, instance, None,
                );

                // --- 裝置下拉選單 ---
                let combo = CreateWindowExW(
                    Default::default(), s!("COMBOBOX"), PCWSTR::null(),
                    WS_CHILD | WS_VISIBLE | CBS_DROPDOWNLIST | WS_VSCROLL | WS_TABSTOP,
                    120, 18, 340, 200,
                    hwnd, Some(ID_COMBO_DRIVE.into()), instance, None,
                );
                G_HWND_COMBO_DRIVE = Some(combo);

                for (_letter, label) in enumerate_removable_drives() {
                    let label_wide = to_wide(&label);
                    SendMessageW(combo, 0x0143, WPARAM(0), LPARAM(label_wide.as_ptr() as isize));
                }
                SendMessageW(combo, 0x014E, WPARAM(0), LPARAM(0)); // CB_SETCURSEL

                // --- 磁碟區標籤 ---
                CreateWindowExW(
                    Default::default(), s!("STATIC"), s!("磁碟區標籤:"),
                    WS_CHILD | WS_VISIBLE | SS_LEFT,
                    20, 62, 100, 20,
                    hwnd, None, instance, None,
                );

                let edit = CreateWindowExW(
                    WS_BORDER, s!("EDIT"), PCWSTR::null(),
                    WS_CHILD | WS_VISIBLE | ES_LEFT | ES_UPPERCASE | WS_TABSTOP,
                    120, 58, 200, 22,
                    hwnd, Some(ID_EDIT_LABEL.into()), instance, None,
                );
                SendMessageW(edit, 0x00C5, WPARAM(11), LPARAM(0)); // EM_SETLIMITTEXT = 11

                // --- 叢集大小 ---
                CreateWindowExW(
                    Default::default(), s!("STATIC"), s!("叢集大小:"),
                    WS_CHILD | WS_VISIBLE | SS_LEFT,
                    20, 102, 100, 20,
                    hwnd, None, instance, None,
                );

                let combo_cluster = CreateWindowExW(
                    Default::default(), s!("COMBOBOX"), PCWSTR::null(),
                    WS_CHILD | WS_VISIBLE | CBS_DROPDOWNLIST | WS_VSCROLL | WS_TABSTOP,
                    120, 98, 120, 200,
                    hwnd, Some(ID_COMBO_CLUSTER.into()), instance, None,
                );
                for (label, _) in CLUSTER_OPTIONS {
                    let label_wide = to_wide(label);
                    SendMessageW(combo_cluster, 0x0143, WPARAM(0), LPARAM(label_wide.as_ptr() as isize));
                }
                SendMessageW(combo_cluster, 0x014E, WPARAM(0), LPARAM(0));

                // --- 快速格式化 ---
                let check = CreateWindowExW(
                    Default::default(), s!("BUTTON"), s!("快速格式化（跳過壞軌掃描）"),
                    WS_CHILD | WS_VISIBLE | BS_CHECKBOX | WS_TABSTOP,
                    20, 140, 250, 22,
                    hwnd, Some(ID_CHECK_QUICK.into()), instance, None,
                );
                SendMessageW(check, 0x00F1, WPARAM(1), LPARAM(0)); // BST_CHECKED

                // --- 開始按鈕 ---
                let btn_start = CreateWindowExW(
                    Default::default(), s!("BUTTON"), s!("開始格式化"),
                    WS_CHILD | WS_VISIBLE | BS_PUSHBUTTON | WS_TABSTOP,
                    20, 180, 140, 30,
                    hwnd, Some(ID_BUTTON_START.into()), instance, None,
                );
                G_HWND_START = Some(btn_start);

                // --- 取消按鈕 ---
                let btn_cancel = CreateWindowExW(
                    Default::default(), s!("BUTTON"), s!("取消"),
                    WS_CHILD | WS_VISIBLE | BS_PUSHBUTTON | WS_TABSTOP,
                    180, 180, 80, 30,
                    hwnd, Some(ID_BUTTON_CANCEL.into()), instance, None,
                );
                EnableWindow(btn_cancel, false);
                G_HWND_CANCEL = Some(btn_cancel);

                // --- 進度條 ---
                let progress = CreateWindowExW(
                    Default::default(), s!("msctls_progress32"), PCWSTR::null(),
                    WS_CHILD | WS_VISIBLE | PBS_SMOOTH,
                    20, 230, 440, 24,
                    hwnd, Some(ID_PROGRESS_BAR.into()), instance, None,
                );
                SendMessageW(progress, PBM_SETRANGE32, WPARAM(0), LPARAM(1000));
                G_HWND_PROGRESS = Some(progress);

                // --- 狀態文字 ---
                let status = CreateWindowExW(
                    Default::default(), s!("STATIC"), s!("就緒"),
                    WS_CHILD | WS_VISIBLE | SS_LEFT,
                    20, 268, 440, 20,
                    hwnd, Some(ID_STATUS_TEXT.into()), instance, None,
                );
                G_HWND_STATUS = Some(status);

                G_SHARED_STATE = Some(Arc::new(Mutex::new(SharedState::new())));

                LRESULT(0)
            }

            WM_COMMAND => {
                let id = (wparam.0 & 0xFFFF) as u16;
                match id {
                    ID_BUTTON_START => start_format(hwnd),
                    ID_BUTTON_CANCEL => cancel_format(),
                    _ => {}
                }
                LRESULT(0)
            }

            WM_TIMER => {
                if wparam.0 == TIMER_ID {
                    update_ui();
                }
                LRESULT(0)
            }

            WM_FORMAT_DONE => {
                KillTimer(hwnd, TIMER_ID);
                update_ui();
                if let Some(btn) = G_HWND_START { EnableWindow(btn, true); }
                if let Some(btn) = G_HWND_CANCEL { EnableWindow(btn, false); }

                // 彈出格式化完成或失敗的對話框
                if let Some(ref state) = G_SHARED_STATE {
                    if let Ok(s) = state.lock() {
                        if let Some(ref msg) = s.result_msg {
                            let title = if s.is_error { "錯誤" } else { "提示" };
                            let icon = if s.is_error { MB_ICONERROR } else { MB_ICONINFORMATION };
                            let msg_wide = to_wide(msg);
                            let title_wide = to_wide(title);
                            MessageBoxW(
                                hwnd,
                                PCWSTR::from_raw(msg_wide.as_ptr()),
                                PCWSTR::from_raw(title_wide.as_ptr()),
                                MB_OK | icon,
                            );
                        }
                    }
                }
                LRESULT(0)
            }

            WM_CTLCOLORSTATIC => {
                LRESULT(unsafe { GetStockObject(GRAY_BRUSH).0 as isize })
            }

            WM_CLOSE => {
                cancel_format();
                if let Some(handle) = G_FORMAT_THREAD.take() {
                    let _ = handle.join();
                }
                unsafe { DestroyWindow(hwnd); }
                LRESULT(0)
            }

            WM_DESTROY => {
                KillTimer(hwnd, TIMER_ID);
                unsafe { PostQuitMessage(0); }
                LRESULT(0)
            }

            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }

    // ========================================================================
    // 格式化啟動 / 取消
    // ========================================================================

    unsafe fn start_format(hwnd: HWND) {
        let combo = match G_HWND_COMBO_DRIVE {
            Some(c) => c, None => return,
        };

        let sel = SendMessageW(combo, 0x0147, WPARAM(0), LPARAM(0));
        if sel.0 < 0 {
            show_error(hwnd, "請先選擇目標裝置");
            return;
        }

        let mut buf = vec![0u16; 64];
        SendMessageW(combo, 0x0148, WPARAM(sel.0 as usize), LPARAM(buf.as_mut_ptr() as isize));
        let label_text = String::from_utf16_lossy(&buf);
        let drive_letter: String = label_text.chars().take_while(|c| *c != ' ').collect();

        if drive_letter.len() < 2 {
            show_error(hwnd, "無法解析磁碟機代號");
            return;
        }

        let phys_num = match get_physical_drive_number(&drive_letter) {
            Some(n) => n,
            None => {
                show_error(hwnd, &format!("無法查詢 {drive_letter} 的實體磁碟編號"));
                return;
            }
        };

        let phys_path = format!("\\\\.\\PhysicalDrive{phys_num}");

        // 叢集大小
        let cluster_size = get_cluster_selection(hwnd);

        // 標籤
        let label = get_label_text(hwnd);

        // 快速格式化
        let quick_format = get_quick_format(hwnd);

        // 重設狀態
        let state = match G_SHARED_STATE.clone() {
            Some(s) => s, None => return,
        };
        {
            let mut s = state.lock().unwrap();
            *s = SharedState::new();
        }

        // 停用/啟用按鈕
        if let Some(btn) = G_HWND_START { EnableWindow(btn, false); }
        if let Some(btn) = G_HWND_CANCEL { EnableWindow(btn, true); }
        if let Some(bar) = G_HWND_PROGRESS {
            SendMessageW(bar, PBM_SETPOS, WPARAM(0), LPARAM(0));
        }

        SetTimer(hwnd, TIMER_ID, TIMER_INTERVAL_MS, None);

        let state_clone = state.clone();

        let handle = thread::spawn(move || {
            let device = match WindowsDeviceIO::new(&phys_path) {
                Ok(d) => d,
                Err(e) => {
                    let mut s = state_clone.lock().unwrap();
                    s.finished = true;
                    s.result_msg = Some(format!("{e}"));
                    s.is_error = true;
                    let _ = SendMessageW(hwnd, WM_FORMAT_DONE, WPARAM(0), LPARAM(0));
                    return;
                }
            };

            let config = FormatConfig {
                label,
                cluster_size,
                quick_format,
                ..FormatConfig::default()
            };

            let progress = GuiProgress::new(state_clone.clone());
            let mut formatter = Formatter::new(device, config, progress);

            match formatter.run() {
                Ok(()) => {
                    let mut s = state_clone.lock().unwrap();
                    s.finished = true;
                    s.result_msg = Some("格式化完成！".to_string());
                    s.is_error = false;
                }
                Err(e) => {
                    let mut s = state_clone.lock().unwrap();
                    s.finished = true;
                    s.result_msg = Some(format!("格式化失敗: {e}"));
                    s.is_error = true;
                }
            }

            let _ = SendMessageW(hwnd, WM_FORMAT_DONE, WPARAM(0), LPARAM(0));
        });

        G_FORMAT_THREAD = Some(handle);
    }

    unsafe fn cancel_format() {
        if let Some(ref state) = G_SHARED_STATE {
            if let Ok(mut s) = state.lock() {
                s.cancelled = true;
            }
        }
    }

    unsafe fn get_cluster_selection(hwnd: HWND) -> Option<u32> {
        let combo = match GetDlgItem(hwnd, ID_COMBO_CLUSTER as i32) {
            h if h.0 != 0 => h,
            _ => return None,
        };
        let sel = SendMessageW(combo, 0x0147, WPARAM(0), LPARAM(0));
        if sel.0 >= 0 && (sel.0 as usize) < CLUSTER_OPTIONS.len() {
            CLUSTER_OPTIONS[sel.0 as usize].1
        } else {
            None
        }
    }

    unsafe fn get_label_text(hwnd: HWND) -> String {
        let edit = match GetDlgItem(hwnd, ID_EDIT_LABEL as i32) {
            h if h.0 != 0 => h,
            _ => return String::new(),
        };
        let mut buf = vec![0u16; 32];
        let len = SendMessageW(edit, 0x000E, WPARAM(buf.len()), LPARAM(buf.as_mut_ptr() as isize));
        if len.0 > 0 {
            String::from_utf16_lossy(&buf[..len.0 as usize])
        } else {
            String::new()
        }
    }

    unsafe fn get_quick_format(hwnd: HWND) -> bool {
        let check = match GetDlgItem(hwnd, ID_CHECK_QUICK as i32) {
            h if h.0 != 0 => h,
            _ => return true,
        };
        SendMessageW(check, 0x00F0, WPARAM(0), LPARAM(0)).0 != 0
    }

    // ========================================================================
    // UI 更新
    // ========================================================================

    unsafe fn update_ui() {
        let state = match &G_SHARED_STATE {
            Some(s) => s, None => return,
        };

        let (phase, current, total, finished, result_msg) = {
            let s = state.lock().unwrap();
            (s.phase.clone(), s.current, s.total, s.finished, s.result_msg.clone())
        };

        if let Some(bar) = G_HWND_PROGRESS {
            let pos = if total > 0 {
                ((current as f64 / total as f64) * 1000.0) as usize
            } else {
                0
            };
            SendMessageW(bar, PBM_SETPOS, WPARAM(pos), LPARAM(0));
        }

        if let Some(status) = G_HWND_STATUS {
            let text = if finished {
                result_msg.unwrap_or_else(|| "完成".to_string())
            } else if phase.is_empty() {
                "準備中...".to_string()
            } else {
                format!("{phase}: {current}/{total}")
            };
            let text_wide = to_wide(&text);
            SetWindowTextW(status, PCWSTR::from_raw(text_wide.as_ptr()));
        }
    }

    // ========================================================================
    // 輔助：顯示錯誤
    // ========================================================================

    unsafe fn show_error(parent: HWND, msg: &str) {
        let msg_wide = to_wide(msg);
        let title_wide = to_wide("錯誤");
        MessageBoxW(
            parent,
            PCWSTR::from_raw(msg_wide.as_ptr()),
            PCWSTR::from_raw(title_wide.as_ptr()),
            MB_OK | MB_ICONERROR,
        );
    }

    // ========================================================================
    // 註冊視窗類別並啟動訊息迴圈
    // ========================================================================

    unsafe fn register_window_class(instance: windows::Win32::System::LibraryLoader::HINSTANCE) -> Result<(), ()> {
        let class_name = to_wide("FAT32FormatterWindow");
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(wnd_proc),
            hInstance: instance,
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap(),
            hbrBackground: GetStockObject(GRAY_BRUSH),
            lpszClassName: PCWSTR::from_raw(class_name.as_ptr()),
            ..Default::default()
        };

        if RegisterClassExW(&wc) == 0 {
            return Err(());
        }
        Ok(())
    }

    pub fn run() {
        unsafe {
            // 初始化 Common Controls（ProgressBar 等需要）
            let icc = INITCOMMONCONTROLSEX {
                dwSize: std::mem::size_of::<INITCOMMONCONTROLSEX>() as u32,
                dwICC: ICC_STANDARD_CLASSES | ICC_PROGRESS_CLASS,
            };
            InitCommonControlsEx(&icc)
                .expect("無法初始化 Common Controls");

            let instance = GetModuleHandleW(None).expect("無法取得 module handle");
            register_window_class(instance).expect("無法註冊視窗類別");

            let class_name = to_wide("FAT32FormatterWindow");
            let title = to_wide("FAT32 格式化工具");

            // 使用 WS_OVERLAPPEDWINDOW ^ WS_THICKFRAME ^ WS_MAXIMIZEBOX 禁止調整大小
            let hwnd = CreateWindowExW(
                Default::default(),
                PCWSTR::from_raw(class_name.as_ptr()),
                PCWSTR::from_raw(title.as_ptr()),
                0x00CA0000 | 0x00020000, // WS_OVERLAPPED | WS_SYSMENU | WS_MINIMIZEBOX
                CW_USEDEFAULT, CW_USEDEFAULT,
                WIN_WIDTH, WIN_HEIGHT,
                None, None, instance, None,
            );

            if hwnd.0 == 0 {
                let err = GetLastError();
                eprintln!("無法建立視窗: {err:?}");
                return;
            }

            ShowWindow(hwnd, SW_SHOW);
            UpdateWindow(hwnd);

            let mut msg = std::mem::zeroed();
            loop {
                let ret = GetMessageW(&mut msg, None, 0, 0);
                if ret.0 <= 0 {
                    break;
                }
                DispatchMessageW(&mut msg);
            }

            // 清理執行緒
            if let Some(handle) = G_FORMAT_THREAD.take() {
                let _ = handle.join();
            }
        }
    }
}

// ============================================================================
// main — 平台分派
// ============================================================================

#[cfg(windows)]
fn main() {
    env_logger::init();
    log::info!("FAT32 Formatter starting...");
    windows_gui::run();
}

#[cfg(not(windows))]
fn main() {
    eprintln!("錯誤：此應用程式僅能在 Windows 上執行。");
    eprintln!("FAT32 Formatter 是 Windows GUI 工具，需要 Win32 API。");
    std::process::exit(1);
}
