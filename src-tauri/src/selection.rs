
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU64, Ordering};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager, PhysicalPosition};
use windows::Win32::Foundation::{LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED,
};
use windows::Win32::System::DataExchange::GetClipboardSequenceNumber;
use windows::Win32::System::SystemInformation::GetTickCount64;
use windows::core::implement;
use windows::Win32::UI::Accessibility::{
    CUIAutomation, IUIAutomation, IUIAutomationElement, IUIAutomationEventHandler,
    IUIAutomationEventHandler_Impl, IUIAutomationTextPattern, IUIAutomationTreeWalker,
    TextPatternRangeEndpoint_End, TextPatternRangeEndpoint_Start, TreeScope_Descendants,
    TreeScope_Subtree, UIA_ControlTypePropertyId, UIA_DocumentControlTypeId,
    UIA_EditControlTypeId, UIA_Text_TextSelectionChangedEventId, UIA_TextControlTypeId,
    UIA_EVENT_ID, UIA_ImageControlTypeId, UIA_TextPatternId, UIA_TitleBarControlTypeId,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, GetDoubleClickTime, MapVirtualKeyW, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD,
    KEYBDINPUT, KEYBD_EVENT_FLAGS, MAPVK_VK_TO_VSC, KEYEVENTF_KEYUP, VIRTUAL_KEY, VK_CONTROL,
    VK_C, VK_LCONTROL, VK_LMENU, VK_LSHIFT, VK_LWIN, VK_MENU, VK_RCONTROL, VK_RMENU, VK_RSHIFT,
    VK_RWIN, VK_SHIFT,
};
use windows::Win32::System::Threading::GetCurrentProcessId;
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetClassNameW, GetCursorPos,
    GetForegroundWindow, GetMessageW, GetWindowThreadProcessId, SendMessageTimeoutW,
    SetWindowsHookExW, TranslateMessage, UnhookWindowsHookEx, WindowFromPoint, KBDLLHOOKSTRUCT,
    LLKHF_INJECTED, LLMHF_INJECTED, MSG, MSLLHOOKSTRUCT, SMTO_ABORTIFHUNG, WH_KEYBOARD_LL,
    WH_MOUSE_LL, WM_KEYDOWN, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_MOUSEWHEEL,
    WM_NCHITTEST, WM_SYSKEYDOWN,
};

static TRACE_SELECTION: AtomicBool = AtomicBool::new(false);

fn debug_log(_message: impl AsRef<str>) {
        if !TRACE_SELECTION.load(Ordering::Relaxed) {
        return;
    }
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(r"D:\RustProject\SimpleTranslation\_sel.log")
    {
        let _ = writeln!(f, "{}", _message.as_ref());
    }
}

enum HookEvent {
        Candidate(i32, i32),
        Dismiss,
}

static HOOK_TX: OnceLock<Sender<HookEvent>> = OnceLock::new();
static LAST_TEXT: Mutex<String> = Mutex::new(String::new());
static BUTTON_RECT: Mutex<Option<(i32, i32, i32, i32)>> = Mutex::new(None);
static PRESSED: AtomicBool = AtomicBool::new(false);
static DRAGGED: AtomicBool = AtomicBool::new(false);
static DOWN_X: AtomicI32 = AtomicI32::new(0);
static DOWN_Y: AtomicI32 = AtomicI32::new(0);
static LAST_UP_TICK: AtomicU64 = AtomicU64::new(0);

static SELECTION_SEQ: AtomicU64 = AtomicU64::new(0);

static DOWN_IN_CLIENT: AtomicBool = AtomicBool::new(false);

static DOWN_IN_SELF: AtomicBool = AtomicBool::new(false);

static DOWN_WINDOW_STATE: Mutex<Option<(isize, i32, i32, i32, i32, bool)>> = Mutex::new(None);

const INJECT_SENTINEL: usize = 0x5349_4D50;
static BUTTON_SHOWN_AT: AtomicU64 = AtomicU64::new(0);

static SELECTION_POINT: Mutex<Option<(i32, i32)>> = Mutex::new(None);

const SELECTION_WINDOW: &str = "selection";

pub const SELECTION_BUTTON_SIZE: u32 = 38;

const SELECTION_PADDING: u32 = 4;

pub const SELECTION_WINDOW_SIZE: u32 = SELECTION_BUTTON_SIZE + SELECTION_PADDING * 2;


fn point_in_button(x: i32, y: i32) -> bool {
    if let Ok(rect) = BUTTON_RECT.lock() {
        if let Some((bx, by, bw, bh)) = *rect {
            return x >= bx - 4 && x <= bx + bw + 4 && y >= by - 4 && y <= by + bh + 4;
        }
    }
    false
}

fn foreground_window_state() -> Option<(isize, i32, i32, i32, i32, bool)> {
    use windows::Win32::Foundation::RECT;
    use windows::Win32::UI::WindowsAndMessaging::{GetWindowRect, IsZoomed};

    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.0.is_null() {
            return None;
        }
        let mut rect = RECT::default();
        if GetWindowRect(hwnd, &mut rect).is_err() {
            return None;
        }
        Some((
            hwnd.0 as isize,
            rect.left,
            rect.top,
            rect.right,
            rect.bottom,
            IsZoomed(hwnd).as_bool(),
        ))
    }
}

fn point_owned_by_self(x: i32, y: i32) -> bool {
    unsafe {
        let hwnd = WindowFromPoint(POINT { x, y });
        if hwnd.0.is_null() {
            return false;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        pid != 0 && pid == GetCurrentProcessId()
    }
}

fn hit_test_is_client(x: i32, y: i32) -> bool {
    unsafe {
        let hwnd = WindowFromPoint(POINT { x, y });
        if hwnd.0.is_null() {
            return true;
        }

        let mut result: usize = 0;
        let lparam = (((y as isize) & 0xFFFF) << 16) | ((x as isize) & 0xFFFF);
        let ok = SendMessageTimeoutW(
            hwnd,
            WM_NCHITTEST,
            WPARAM(0),
            LPARAM(lparam),
            SMTO_ABORTIFHUNG,
            20,
            Some(&mut result),
        );

                if ok.0 == 0 {
            return true;
        }

        let hit = result as isize;
                                                                        !matches!(hit, 2 | 6..=9 | 10..=17 | 20)
    }
}

fn foreground_is_self() -> bool {
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.0.is_null() {
            return false;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        pid != 0 && pid == GetCurrentProcessId()
    }
}

fn foreground_class_name() -> String {
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.0.is_null() {
            return String::new();
        }
        let mut buf = [0u16; 256];
        let len = GetClassNameW(hwnd, &mut buf);
        if len <= 0 {
            return String::new();
        }
        String::from_utf16_lossy(&buf[..len as usize])
    }
}

#[cfg(target_os = "windows")]
pub fn foreground_process_name() -> String {
    use windows::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_FORMAT, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.0.is_null() {
            return String::new();
        }

        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == 0 {
            return String::new();
        }

        let Ok(process) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
            return String::new();
        };

        let mut buf = [0u16; 260];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_FORMAT(0),
            windows::core::PWSTR(buf.as_mut_ptr()),
            &mut len,
        );
        let _ = windows::Win32::Foundation::CloseHandle(process);

        if ok.is_err() {
            return String::new();
        }

        let full = String::from_utf16_lossy(&buf[..len as usize]);

                full.rsplit(['\\', '/'])
            .next()
            .map(|name| {
                let lower = name.to_lowercase();
                lower
                    .strip_suffix(".exe")
                    .map(|s| s.to_string())
                    .unwrap_or(lower)
            })
            .unwrap_or_default()
    }
}

#[cfg(target_os = "windows")]
pub fn is_foreground_fullscreen() -> bool {
    use windows::Win32::Foundation::RECT;
    use windows::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST,
    };
    use windows::Win32::UI::WindowsAndMessaging::{GetWindowRect, IsZoomed};

    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.0.is_null() {
            return false;
        }

                if IsZoomed(hwnd).as_bool() {
            return false;
        }

        let mut win = RECT::default();
        if GetWindowRect(hwnd, &mut win).is_err() {
            return false;
        }

        let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if !GetMonitorInfoW(monitor, &mut info).as_bool() {
            return false;
        }

        let m = info.rcMonitor;
                win.left <= m.left + 2
            && win.top <= m.top + 2
            && win.right >= m.right - 2
            && win.bottom >= m.bottom - 2
    }
}

fn is_desktop_or_shell() -> bool {
    matches!(
        foreground_class_name().as_str(),
        "Progman" | "WorkerW" | "Shell_TrayWnd" | "Shell_SecondaryTrayWnd"
    )
}

fn shift_pressed() -> bool {
    unsafe { GetAsyncKeyState(VK_SHIFT.0 as i32) < 0 }
}

fn notify(ev: HookEvent) {
    if let Some(tx) = HOOK_TX.get() {
        let _ = tx.send(ev);
    }
}

unsafe extern "system" fn mouse_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 {
                        if (*(lparam.0 as *const MSLLHOOKSTRUCT)).flags & LLMHF_INJECTED != 0 {
            return CallNextHookEx(None, code, wparam, lparam);
        }

        let msg = wparam.0 as u32;

        match msg {
            WM_LBUTTONDOWN => {
                let info = &*(lparam.0 as *const MSLLHOOKSTRUCT);
                let (x, y) = (info.pt.x, info.pt.y);

                PRESSED.store(true, Ordering::Relaxed);
                DRAGGED.store(false, Ordering::Relaxed);
                DOWN_X.store(x, Ordering::Relaxed);
                DOWN_Y.store(y, Ordering::Relaxed);

                                                let in_self = point_owned_by_self(x, y) || point_in_button(x, y);
                DOWN_IN_SELF.store(in_self, Ordering::Relaxed);
                DOWN_IN_CLIENT.store(!in_self && hit_test_is_client(x, y), Ordering::Relaxed);
                                if let Ok(mut g) = DOWN_WINDOW_STATE.lock() {
                    *g = foreground_window_state();
                }

                                if !point_in_button(x, y) {
                    notify(HookEvent::Dismiss);
                }
            }
            WM_MOUSEMOVE => {
                if PRESSED.load(Ordering::Relaxed) {
                    let info = &*(lparam.0 as *const MSLLHOOKSTRUCT);
                    let dx = (info.pt.x - DOWN_X.load(Ordering::Relaxed)) as i64;
                    let dy = (info.pt.y - DOWN_Y.load(Ordering::Relaxed)) as i64;
                                        if dx * dx + dy * dy >= 36 {
                                                DRAGGED.store(true, Ordering::Relaxed);
                    }
                }
            }
            WM_MOUSEWHEEL => {
                                notify(HookEvent::Dismiss);
            }
            WM_LBUTTONUP => {
                let info = &*(lparam.0 as *const MSLLHOOKSTRUCT);
                let (x, y) = (info.pt.x, info.pt.y);

                let was_pressed = PRESSED.swap(false, Ordering::Relaxed);
                let dragged = DRAGGED.load(Ordering::Relaxed);
                let now = GetTickCount64();
                let last = LAST_UP_TICK.load(Ordering::Relaxed);
                let near = (x - DOWN_X.load(Ordering::Relaxed)).abs() < 6
                    && (y - DOWN_Y.load(Ordering::Relaxed)).abs() < 6;
                                let dbl_ms = unsafe { GetDoubleClickTime() } as u64;
                let is_multi_click =
                    was_pressed && last != 0 && now.saturating_sub(last) < dbl_ms && near;
                                let shift_click = shift_pressed();
                LAST_UP_TICK.store(now, Ordering::Relaxed);

                                                                let before = DOWN_WINDOW_STATE.lock().ok().and_then(|g| *g);
                if let (Some(before), Some(after)) = (before, foreground_window_state()) {
                    let changed = after.0 != before.0
                        || after.1 != before.1
                        || after.2 != before.2
                        || after.3 != before.3
                        || after.4 != before.4
                        || after.5 != before.5;
                    if changed && (dragged || is_multi_click || shift_click) {
                        debug_log(format!(
                            "[钩子] 窗口在按压期间发生变化（最大化/还原/移动/缩放），忽略 ({x},{y})"
                        ));
                        return CallNextHookEx(None, code, wparam, lparam);
                    }
                }

                                if DOWN_IN_SELF.load(Ordering::Relaxed) || point_owned_by_self(x, y) {
                    if dragged || is_multi_click || shift_click {
                        debug_log(format!("[钩子] 落点在自家窗口，忽略 ({x},{y})"));
                    }
                    return CallNextHookEx(None, code, wparam, lparam);
                }

                                if !DOWN_IN_CLIENT.load(Ordering::Relaxed) {
                    if dragged || is_multi_click || shift_click {
                        debug_log(format!(
                            "[钩子] 按下点非客户区（拖标题栏/边框/滚动条），忽略 ({x},{y})"
                        ));
                    }
                    return CallNextHookEx(None, code, wparam, lparam);
                }

                                                                                if (dragged || is_multi_click || shift_click) && !point_in_button(x, y) {
                    notify(HookEvent::Candidate(x, y));
                }
            }
            _ => {}
        }
    }

    CallNextHookEx(None, code, wparam, lparam)
}

unsafe extern "system" fn keyboard_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 {
        let msg = wparam.0 as u32;
        if msg == WM_KEYDOWN || msg == WM_SYSKEYDOWN {
            let kbd = &*(lparam.0 as *const KBDLLHOOKSTRUCT);

                                                if kbd.flags.0 & LLKHF_INJECTED.0 != 0 {
                return CallNextHookEx(None, code, wparam, lparam);
            }
                        if kbd.dwExtraInfo == INJECT_SENTINEL {
                return CallNextHookEx(None, code, wparam, lparam);
            }

            let vk = kbd.vkCode as u16;

                                    if vk == VK_C.0 && is_key_down(VK_CONTROL.0) {
                SELECTION_SEQ.fetch_add(1, Ordering::SeqCst);
                debug_log("用户手动 Ctrl+C，作废待执行的取词任务");
                return CallNextHookEx(None, code, wparam, lparam);
            }

                                                let is_modifier = [
                VK_SHIFT,
                VK_LSHIFT,
                VK_RSHIFT,
                VK_CONTROL,
                VK_LCONTROL,
                VK_RCONTROL,
                VK_MENU,
                VK_LMENU,
                VK_RMENU,
                VK_LWIN,
                VK_RWIN,
            ]
            .contains(&VIRTUAL_KEY(vk));

            if !is_modifier {
                notify(HookEvent::Dismiss);
            }
        }
    }

    CallNextHookEx(None, code, wparam, lparam)
}

pub fn start(app: AppHandle) {
        start_selection_watcher();

    let (tx, rx) = channel::<HookEvent>();
    if HOOK_TX.set(tx).is_err() {
        return;
    }

        std::thread::spawn(move || unsafe {
        let Ok(mouse_hook) = SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_proc), None, 0) else {
            return;
        };
        let keyboard_hook = SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_proc), None, 0).ok();

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        let _ = UnhookWindowsHookEx(mouse_hook);
        if let Some(hook) = keyboard_hook {
            let _ = UnhookWindowsHookEx(hook);
        }
    });

    std::thread::spawn(move || {
                unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        }
        while let Ok(event) = rx.recv() {
            match event {
                HookEvent::Candidate(x, y) => handle_mouse_up(&app, x, y),
                HookEvent::Dismiss => handle_dismiss(&app),
            }
        }
    });
}

fn handle_mouse_up(app: &AppHandle, x: i32, y: i32) {
        if !crate::SELECTION_ENABLED.load(Ordering::SeqCst) {
        hide_button(app);
        return;
    }

        if point_in_button(x, y) {
        return;
    }

            if foreground_is_self() {
        hide_button(app);
        return;
    }

            if is_desktop_or_shell() {
        hide_button(app);
        return;
    }

            let cfg = app.state::<crate::AppState>().config(app);
    if let Some(reason) = crate::selection_blocked_by_rules(&cfg) {
        hide_button(app);
        debug_log(format!("被规则拦截：{reason}"));
        return;
    }

    let mode = cfg.trigger.mode;
    let want_modifier = cfg.trigger.modifier.clone();

    hide_button(app);

        if mode == "hotkey" {
        return;
    }

        if !modifier_satisfied(&want_modifier) {
        debug_log(format!("需要按住 {want_modifier} 才触发，本次跳过"));
        return;
    }

    debug_log(format!("── 鼠标在 ({x}, {y}) 抬起，开始判断有没有选中文本"));
    debug_log(format!(
        "   前台进程={} 窗口类={} 全屏={} hit-test={}",
        foreground_process_name(),
        foreground_class_name(),
        is_foreground_fullscreen(),
        hit_test_is_client(x, y)
    ));

            let task_seq = SELECTION_SEQ.fetch_add(1, Ordering::SeqCst) + 1;

    let max_len = app
        .state::<crate::AppState>()
        .config(app)
        .selection_threshold as usize;

        std::thread::sleep(Duration::from_millis(60));

    if SELECTION_SEQ.load(Ordering::SeqCst) != task_seq {
        debug_log("任务已过期（等选区稳定期间又有新操作），放弃");
        return;
    }

                if let Some(text) = take_cached_selection(3000) {
        let len = text.chars().count();
        debug_log(format!("① 选区广播：拿到 {len} 个字符（松手前的最后一次变化）"));

        if max_len > 0 && len > max_len {
            debug_log(format!("   但超过设定上限 {max_len} 字，判定为误选，不弹按钮"));
            return;
        }

        if let Ok(mut guard) = LAST_TEXT.lock() {
            *guard = text;
        }
        debug_log("=> 结论：显示悬浮按钮");
        show_button(app, x, y);
        return;
    }

    debug_log("① 选区广播：没有缓存 → 改用主动探测");

                    let has_selection = match probe_selection((x, y)) {
        SelectionProbe::Selected => true,
        SelectionProbe::Unsupported => {
            debug_log("② 落点没有文本能力，跳过剪贴板探测");
            false
        }
        SelectionProbe::Empty => {
            debug_log("② 剪贴板探测：备份剪贴板 → 临时 Ctrl+C → 读取 → 立刻还原");
            let got = capture_selection_text();
            match &got {
                Some(text) => {
                    let len = text.chars().count();
                    debug_log(format!("② 剪贴板探测：复制到了新内容（{len} 个字符）"));
                    if max_len > 0 && len > max_len {
                        debug_log(format!("   超过设定上限 {max_len} 字，判定为误选，不弹按钮"));
                        return;
                    }
                }
                None => debug_log("② 剪贴板探测：没有复制到新内容"),
            }
            if let Some(text) = &got {
                if let Ok(mut guard) = LAST_TEXT.lock() {
                    *guard = text.clone();
                }
            }
            got.is_some()
        }
    };

    if SELECTION_SEQ.load(Ordering::SeqCst) != task_seq {
        debug_log("取词期间用户又操作了，放弃本次结果");
        return;
    }

    if has_selection {
        debug_log("=> 结论：显示悬浮按钮");
        show_button(app, x, y);
    } else {
        debug_log("=> 结论：不显示按钮");
    }
}


fn keyboard_input(vk: VIRTUAL_KEY, up: bool) -> INPUT {
            let scan = unsafe { MapVirtualKeyW(vk.0 as u32, MAPVK_VK_TO_VSC) } as u16;

    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: scan,
                dwFlags: if up {
                    KEYEVENTF_KEYUP
                } else {
                    KEYBD_EVENT_FLAGS(0)
                },
                time: 0,
                                                                dwExtraInfo: 0,
            },
        },
    }
}

fn is_key_down(vk: u16) -> bool {
    unsafe { (GetAsyncKeyState(vk as i32) as u16 & 0x8000) != 0 }
}

fn modifier_satisfied(want: &str) -> bool {
    match want.trim().to_lowercase().as_str() {
        "ctrl" => is_key_down(VK_CONTROL.0) || is_key_down(VK_LCONTROL.0) || is_key_down(VK_RCONTROL.0),
        "alt" => is_key_down(VK_MENU.0) || is_key_down(VK_LMENU.0) || is_key_down(VK_RMENU.0),
        "shift" => is_key_down(VK_SHIFT.0) || is_key_down(VK_LSHIFT.0) || is_key_down(VK_RSHIFT.0),
        _ => true,
    }
}

pub fn wait_modifiers_released(timeout_ms: u64) {
    let start = std::time::Instant::now();
    loop {
        let busy = is_key_down(VK_LSHIFT.0)
            || is_key_down(VK_RSHIFT.0)
            || is_key_down(VK_LMENU.0)
            || is_key_down(VK_RMENU.0)
            || is_key_down(VK_LWIN.0)
            || is_key_down(VK_RWIN.0);

        if !busy || start.elapsed().as_millis() as u64 >= timeout_ms {
            return;
        }
        std::thread::sleep(Duration::from_millis(8));
    }
}

fn send_ctrl_c() {
    let send = |vk: VIRTUAL_KEY, up: bool| {
        let inputs = [keyboard_input(vk, up)];
        unsafe {
            SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
        }
    };

    send(VK_CONTROL, false);
    std::thread::sleep(Duration::from_millis(15));
    send(VK_C, false);
    std::thread::sleep(Duration::from_millis(15));
    send(VK_C, true);
    std::thread::sleep(Duration::from_millis(15));
    send(VK_CONTROL, true);
}

fn read_clipboard_text() -> Option<String> {
    let mut clipboard = arboard::Clipboard::new().ok()?;
    clipboard.get_text().ok()
}

fn read_clipboard_text_retry() -> Option<String> {
    for attempt in 0..6 {
        if let Some(text) = read_clipboard_text() {
            return Some(text);
        }
        if attempt < 5 {
            std::thread::sleep(Duration::from_millis(25));
        }
    }
    None
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SelectionProbe {
        Selected,
        Empty,
        Unsupported,
}

#[allow(dead_code)]
fn control_type_name(id: i32) -> &'static str {
    match id {
        50000 => "Button",
        50001 => "Calendar",
        50002 => "CheckBox",
        50003 => "ComboBox",
        50004 => "Edit",
        50005 => "Hyperlink",
        50006 => "Image",
        50007 => "ListItem",
        50008 => "List",
        50009 => "Menu",
        50010 => "MenuBar",
        50011 => "MenuItem",
        50012 => "ProgressBar",
        50013 => "RadioButton",
        50014 => "ScrollBar",
        50015 => "Slider",
        50016 => "Spinner",
        50017 => "StatusBar",
        50018 => "Tab",
        50019 => "TabItem",
        50020 => "Text",
        50021 => "ToolBar",
        50022 => "ToolTip",
        50023 => "Tree",
        50024 => "TreeItem",
        50025 => "Custom",
        50026 => "Group",
        50027 => "Thumb",
        50028 => "DataGrid",
        50029 => "DataItem",
        50030 => "Document",
        50031 => "SplitButton",
        50032 => "Window",
        50033 => "Pane",
        50034 => "Header",
        50035 => "HeaderItem",
        50036 => "Table",
        50037 => "TitleBar",
        50038 => "Separator",
        50039 => "SemanticZoom",
        50040 => "AppBar",
        _ => "?",
    }
}

unsafe fn element_is_text_capable(element: &IUIAutomationElement) -> bool {
        if element
        .GetCurrentPatternAs::<IUIAutomationTextPattern>(UIA_TextPatternId)
        .is_ok()
    {
        return true;
    }

                            if let Ok(ct) = element.CurrentControlType() {
        if ct.0 == 50030 {
            if let Ok(cls) = element.CurrentClassName() {
                return !cls.to_string().trim().is_empty();
            }
        }
    }

    false
}

unsafe fn ancestry_has_text_capability(
    automation: &IUIAutomation,
    element: &IUIAutomationElement,
    max_depth: usize,
) -> bool {
    let Ok(walker): Result<IUIAutomationTreeWalker, _> = automation.ControlViewWalker() else {
                return true;
    };

    let mut current = element.clone();
    for _ in 0..max_depth {
        let Ok(parent) = walker.GetParentElement(&current) else {
            return false;
        };
        if element_is_text_capable(&parent) {
            return true;
        }
        current = parent;
    }
    false
}

unsafe fn subtree_has_text_capability(
    automation: &IUIAutomation,
    element: &IUIAutomationElement,
) -> bool {
    let mut hit = false;
    for type_id in [
        UIA_DocumentControlTypeId,
        UIA_EditControlTypeId,
        UIA_TextControlTypeId,
    ] {
        let count = count_descendants_of_type(automation, element, type_id.0);
        if TRACE_SELECTION.load(Ordering::Relaxed) {
            debug_log(format!(
                "  子树探测: {}={} 个",
                control_type_name(type_id.0),
                count
            ));
        }
        if count > 0 {
            hit = true;
        }
    }
    hit
}

unsafe fn count_descendants_of_type(
    automation: &IUIAutomation,
    element: &IUIAutomationElement,
    type_id: i32,
) -> usize {
    let Ok(cond) = automation.CreatePropertyCondition(
        UIA_ControlTypePropertyId,
        &windows::core::VARIANT::from(type_id),
    ) else {
        return 0;
    };
    match element.FindAll(TreeScope_Descendants, &cond) {
        Ok(found) => found.Length().unwrap_or(0) as usize,
        Err(_) => 0,
    }
}

pub fn probe_selection(point: (i32, i32)) -> SelectionProbe {
    let result = unsafe {
        (|| -> Option<SelectionProbe> {
            let automation: IUIAutomation =
                CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER).ok()?;

            let pt = POINT {
                x: point.0,
                y: point.1,
            };
            let element = automation.ElementFromPoint(pt).ok()?;

                        if TRACE_SELECTION.load(Ordering::Relaxed) {
                let ct = element.CurrentControlType().ok();
                let ct_txt = match ct {
                    Some(c) => format!("{}({})", control_type_name(c.0), c.0),
                    None => "?".into(),
                };
                let cls = element
                    .CurrentClassName()
                    .map(|s| s.to_string())
                    .unwrap_or_default();
                let nm: String = element
                    .CurrentName()
                    .map(|s| s.to_string())
                    .unwrap_or_default()
                    .chars()
                    .take(24)
                    .collect();
                debug_log(format!(
                    "  落点: ControlType={ct_txt} class={cls:?} name={nm:?} 自身有TextPattern={}",
                    element
                        .GetCurrentPatternAs::<IUIAutomationTextPattern>(UIA_TextPatternId)
                        .is_ok()
                ));

                if let Ok(walker) = automation.ControlViewWalker() {
                    let mut cur = element.clone();
                    for lvl in 0..6 {
                        let Ok(par) = walker.GetParentElement(&cur) else {
                            debug_log(format!("  祖先 L{lvl}: （到顶）"));
                            break;
                        };
                        let pct = par.CurrentControlType().ok();
                        let pct_txt = match pct {
                            Some(c) => format!("{}({})", control_type_name(c.0), c.0),
                            None => "?".into(),
                        };
                        let pcls = par.CurrentClassName().map(|s| s.to_string()).unwrap_or_default();
                        let par_tp = par
                            .GetCurrentPatternAs::<IUIAutomationTextPattern>(UIA_TextPatternId)
                            .is_ok();
                        debug_log(format!(
                            "  祖先 L{lvl}: {pct_txt} class={pcls:?} TextPattern={par_tp} 判为文本容器={}",
                            element_is_text_capable(&par)
                        ));
                        cur = par;
                    }
                }
            }

                                                                                                                        if let Ok(ct) = element.CurrentControlType() {
                if ct == UIA_ImageControlTypeId || ct == UIA_TitleBarControlTypeId {
                    return Some(SelectionProbe::Unsupported);
                }
            }

                        if let Ok(pattern) = element
                .GetCurrentPatternAs::<IUIAutomationTextPattern>(UIA_TextPatternId)
            {
                if let Ok(ranges) = pattern.GetSelection() {
                    let count = ranges.Length().unwrap_or(0);
                    for i in 0..count {
                        let Ok(range) = ranges.GetElement(i) else {
                            continue;
                        };
                        match range.GetText(64) {
                            Ok(text) => {
                                if !text.to_string().trim().is_empty() {
                                    return Some(SelectionProbe::Selected);
                                }
                            }
                                                        Err(_) => return Some(SelectionProbe::Selected),
                        }
                    }
                }
                return Some(SelectionProbe::Empty);
            }

                        if ancestry_has_text_capability(&automation, &element, 6) {
                return Some(SelectionProbe::Empty);
            }

                        if subtree_has_text_capability(&automation, &element) {
                return Some(SelectionProbe::Empty);
            }

                        Some(SelectionProbe::Unsupported)
        })()
    };

    let result = result.unwrap_or(SelectionProbe::Empty);

    match result {
        SelectionProbe::Selected => debug_log("① 无障碍：确认有选中文本"),
        SelectionProbe::Empty => debug_log("① 无障碍：允许剪贴板兜底"),
        SelectionProbe::Unsupported => debug_log("① 无障碍：无文本能力，不发送 Ctrl+C"),
    }

    result
}

unsafe fn element_selection_text(element: &IUIAutomationElement) -> Option<String> {
    let pattern = element
        .GetCurrentPatternAs::<IUIAutomationTextPattern>(UIA_TextPatternId)
        .ok()?;
    let ranges = pattern.GetSelection().ok()?;
    let count = ranges.Length().unwrap_or(0);

    let mut collected = String::new();
    for i in 0..count {
        let Ok(range) = ranges.GetElement(i) else {
            continue;
        };
        let Ok(part) = range.GetText(-1) else {
            continue;
        };
        collected.push_str(&part.to_string());
    }

    let trimmed = collected.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

pub fn read_selection_via_uia(point: (i32, i32)) -> Option<String> {
    let text = unsafe {
        (|| -> Option<String> {
            let automation: IUIAutomation =
                CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER).ok()?;
            let pt = POINT {
                x: point.0,
                y: point.1,
            };
            let element = automation.ElementFromPoint(pt).ok()?;
            element_selection_text(&element)
        })()
    };

    match text {
        Some(t) if t.chars().count() <= 8000 => Some(t),
        _ => None,
    }
}


struct CachedSelection {
    text: String,
    at: u64,
}

static SELECTION_CACHE: Mutex<Option<CachedSelection>> = Mutex::new(None);

fn cache_selection(text: String) {
    if let Ok(mut cache) = SELECTION_CACHE.lock() {
        *cache = Some(CachedSelection {
            text,
            at: unsafe { GetTickCount64() },
        });
    }
}

fn take_cached_selection(max_age_ms: u64) -> Option<String> {
    let mut cache = SELECTION_CACHE.lock().ok()?;
    let cached = cache.take()?;
    let now = unsafe { GetTickCount64() };
    if now.saturating_sub(cached.at) <= max_age_ms {
        Some(cached.text)
    } else {
        None
    }
}

unsafe fn element_selection_text_strict(element: &IUIAutomationElement) -> Option<String> {
    let pattern = element
        .GetCurrentPatternAs::<IUIAutomationTextPattern>(UIA_TextPatternId)
        .ok()?;
    let ranges = pattern.GetSelection().ok()?;
    let count = ranges.Length().unwrap_or(0);

    let mut collected = String::new();
    for i in 0..count {
        let Ok(range) = ranges.GetElement(i) else {
            continue;
        };

                if let Ok(cmp) = range.CompareEndpoints(
            TextPatternRangeEndpoint_Start,
            &range,
            TextPatternRangeEndpoint_End,
        ) {
            if cmp == 0 {
                continue;
            }
        }

        let Ok(part) = range.GetText(-1) else {
            continue;
        };
        collected.push_str(&part.to_string());
    }

    let trimmed = collected.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

#[implement(IUIAutomationEventHandler)]
struct SelectionWatcher;

impl IUIAutomationEventHandler_Impl for SelectionWatcher_Impl {
    fn HandleAutomationEvent(
        &self,
        sender: Option<&IUIAutomationElement>,
        _eventid: UIA_EVENT_ID,
    ) -> windows::core::Result<()> {
        if let Some(element) = sender {
            unsafe {
                if let Some(text) = element_selection_text_strict(element) {
                    cache_selection(text);
                }
            }
        }
        Ok(())
    }
}

pub fn start_selection_watcher() {
    std::thread::spawn(|| unsafe {
        let _ = CoInitializeEx(None, windows::Win32::System::Com::COINIT_MULTITHREADED);

        let Ok(automation) =
            CoCreateInstance::<_, IUIAutomation>(&CUIAutomation, None, CLSCTX_INPROC_SERVER)
        else {
            debug_log("选区广播：创建 UIA 实例失败，退回主动探测");
            return;
        };

        let Ok(root) = automation.GetRootElement() else {
            return;
        };

        let handler: IUIAutomationEventHandler = SelectionWatcher.into();
        let registered = automation.AddAutomationEventHandler(
            UIA_Text_TextSelectionChangedEventId,
            &root,
            TreeScope_Subtree,
            None,
            &handler,
        );

        match registered {
            Ok(()) => debug_log("选区广播：已开始监听全局选区变化"),
            Err(_) => {
                debug_log("选区广播：注册失败，退回主动探测");
                return;
            }
        }

                loop {
            std::thread::sleep(Duration::from_secs(30));
        }
    });
}

fn restore_clipboard(backup: Option<&str>) {
    let Ok(mut clipboard) = arboard::Clipboard::new() else {
        return;
    };

    match backup {
        Some(text) => {
            let _ = clipboard.set_text(text.to_string());
        }
                None => {
            let _ = clipboard.clear();
        }
    }
}

pub fn capture_selection_text() -> Option<String> {
        wait_modifiers_released(700);

        let backup = read_clipboard_text();
    let before = unsafe { GetClipboardSequenceNumber() };

    send_ctrl_c();

                    let mut changed = false;
    for _ in 0..80 {
        std::thread::sleep(Duration::from_millis(10));
        if unsafe { GetClipboardSequenceNumber() } != before {
            changed = true;
            break;
        }
    }

    if !changed {
                debug_log("      · Ctrl+C 之后剪贴板内容没有变化（没有产生新选区）");
        return None;
    }

    let copied = read_clipboard_text_retry();
        restore_clipboard(backup.as_deref());

    let Some(text) = copied else {
        debug_log("      · 剪贴板读取失败（可能正被其他程序占用）");
        return None;
    };

    let trimmed = text.trim();
    if trimmed.is_empty() || trimmed.chars().count() > 8000 {
        return None;
    }
    Some(trimmed.to_string())
}

pub fn cursor_position() -> (i32, i32) {
    let mut point = POINT::default();
    unsafe {
        let _ = GetCursorPos(&mut point);
    }
    (point.x, point.y)
}


fn show_button(app: &AppHandle, x: i32, y: i32) {
    let Some(win) = app.get_webview_window(SELECTION_WINDOW) else {
        return;
    };

            let _ = win.set_size(tauri::LogicalSize::new(
        SELECTION_WINDOW_SIZE as f64,
        SELECTION_WINDOW_SIZE as f64,
    ));

    let size = win
        .outer_size()
        .unwrap_or(tauri::PhysicalSize::new(69, 69));
    let w = size.width as i32;
    let h = size.height as i32;

        let mut px = x + 10;
    let mut py = y + 10;

    if let Ok(Some(monitor)) = win.current_monitor() {
        let ms = monitor.size();
        let mp = monitor.position();
        let right = mp.x + ms.width as i32;
        let bottom = mp.y + ms.height as i32;
        if px + w > right - 4 {
            px = x - w - 14;
        }
        if py + h > bottom - 4 {
            py = y - h - 14;
        }
    }

    let _ = win.set_position(PhysicalPosition::new(px, py));
    let _ = win.show();

            let _ = app.emit_to(SELECTION_WINDOW, "selection-shown", ());

    BUTTON_SHOWN_AT.store(unsafe { GetTickCount64() }, Ordering::Relaxed);
    if let Ok(mut rect) = BUTTON_RECT.lock() {
        *rect = Some((px, py, w, h));
    }
        if let Ok(mut point) = SELECTION_POINT.lock() {
        *point = Some((x, y));
    }
}

pub fn take_selection_point() -> Option<(i32, i32)> {
    SELECTION_POINT.lock().ok().and_then(|mut guard| guard.take())
}

fn handle_dismiss(app: &AppHandle) {
    let shown_at = BUTTON_SHOWN_AT.load(Ordering::Relaxed);
    if shown_at != 0 && unsafe { GetTickCount64() }.saturating_sub(shown_at) < 400 {
        return;
    }
    hide_button(app);
}

pub fn hide_button(app: &AppHandle) {
            let visible = BUTTON_RECT.lock().map(|r| r.is_some()).unwrap_or(false);
    if !visible {
        return;
    }

    if let Some(win) = app.get_webview_window(SELECTION_WINDOW) {
        let _ = win.hide();
    }
    if let Ok(mut rect) = BUTTON_RECT.lock() {
        *rect = None;
    }
    if let Ok(mut point) = SELECTION_POINT.lock() {
        *point = None;
    }
    BUTTON_SHOWN_AT.store(0, Ordering::Relaxed);
}

#[allow(dead_code)]
pub fn take_last_text() -> String {
    LAST_TEXT
        .lock()
        .map(|mut guard| std::mem::take(&mut *guard))
        .unwrap_or_default()
}

pub fn peek_last_text() -> String {
    LAST_TEXT
        .lock()
        .map(|guard| guard.clone())
        .unwrap_or_default()
}

pub fn make_noactivate(window: &tauri::WebviewWindow) {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{
        GetWindowLongPtrW, SetWindowLongPtrW, GWL_EXSTYLE, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
    };

    if let Ok(hwnd) = window.hwnd() {
        let handle = HWND(hwnd.0 as *mut std::ffi::c_void);
        unsafe {
            let style = GetWindowLongPtrW(handle, GWL_EXSTYLE);
            SetWindowLongPtrW(
                handle,
                GWL_EXSTYLE,
                style | (WS_EX_NOACTIVATE.0 as isize) | (WS_EX_TOOLWINDOW.0 as isize),
            );
        }
    }
}
