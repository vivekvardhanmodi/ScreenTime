#![cfg(target_os = "windows")]

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc::UnboundedSender;
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Power::{
    RegisterPowerSettingNotification, UnregisterPowerSettingNotification, HPOWERNOTIFY,
    POWERBROADCAST_SETTING,
};
use windows_sys::Win32::System::RemoteDesktop::{
    WTSRegisterSessionNotification, WTSUnRegisterSessionNotification, NOTIFY_FOR_THIS_SESSION,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW,
    PostThreadMessageW, RegisterClassExW, TranslateMessage, DEVICE_NOTIFY_WINDOW_HANDLE,
    EVENT_SYSTEM_FOREGROUND, HWND_MESSAGE, MSG, WINEVENT_OUTOFCONTEXT, WINEVENT_SKIPOWNPROCESS,
    WNDCLASSEXW,
};

use super::events::{
    parse_display_power_state, parse_power_broadcast, parse_session_change_event, PlatformEvent,
    PBT_POWERSETTINGCHANGE, WM_POWERBROADCAST, WM_WTSSESSION_CHANGE,
};
use super::win32_utils::{
    get_process_path_and_name, get_window_title, resolve_real_process_for_hwnd, SetWinEventHook,
    UnhookWinEvent,
};
use screentime_core::WindowInfo;

// GUID_CONSOLE_DISPLAY_STATE: 6FE69556-704A-47A0-8F24-C28D936FDA47
const GUID_CONSOLE_DISPLAY_STATE: windows_sys::core::GUID = windows_sys::core::GUID {
    data1: 0x6fe69556,
    data2: 0x704a,
    data3: 0x47a0,
    data4: [0x8f, 0x24, 0xc2, 0x8d, 0x93, 0x6f, 0xda, 0x47],
};

const WM_QUIT: u32 = 0x0012;

static GLOBAL_TX: Mutex<Option<UnboundedSender<PlatformEvent>>> = Mutex::new(None);

#[inline]
fn guid_eq(a: &windows_sys::core::GUID, b: &windows_sys::core::GUID) -> bool {
    a.data1 == b.data1 && a.data2 == b.data2 && a.data3 == b.data3 && a.data4 == b.data4
}

unsafe extern "system" fn win_event_proc(
    _hook: *mut std::ffi::c_void,
    event: u32,
    hwnd: HWND,
    _id_object: i32,
    _id_child: i32,
    _event_thread: u32,
    _event_time: u32,
) {
    if event != EVENT_SYSTEM_FOREGROUND {
        return;
    }

    let guard = GLOBAL_TX.lock().ok();
    let Some(ref tx) = guard.as_deref().and_then(|g| g.as_ref()) else {
        return;
    };

    if hwnd.is_null() {
        let _ = tx.send(PlatformEvent::WindowChanged(None));
        return;
    }

    let (real_hwnd, _real_pid) = resolve_real_process_for_hwnd(hwnd);
    let title = get_window_title(real_hwnd);
    let app_class = match get_process_path_and_name(real_hwnd) {
        Ok((_, _, name)) => name,
        Err(_) => "unknown".to_string(),
    };

    let info = WindowInfo { app_class, title };

    let _ = tx.send(PlatformEvent::WindowChanged(Some(info)));
}

unsafe extern "system" fn wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_POWERBROADCAST => {
            if wparam == PBT_POWERSETTINGCHANGE && lparam != 0 {
                let setting = &*(lparam as *const POWERBROADCAST_SETTING);
                if guid_eq(&setting.PowerSetting, &GUID_CONSOLE_DISPLAY_STATE) {
                    let state = *(setting.Data.as_ptr() as *const u32);
                    if let Some(event) = parse_display_power_state(state) {
                        if let Ok(guard) = GLOBAL_TX.lock() {
                            if let Some(ref tx) = *guard {
                                let _ = tx.send(event);
                            }
                        }
                    }
                }
            } else if let Some(event) = parse_power_broadcast(wparam) {
                if let Ok(guard) = GLOBAL_TX.lock() {
                    if let Some(ref tx) = *guard {
                        let _ = tx.send(event);
                    }
                }
            }
            1
        }
        WM_WTSSESSION_CHANGE => {
            if let Some(event) = parse_session_change_event(wparam) {
                if let Ok(guard) = GLOBAL_TX.lock() {
                    if let Some(ref tx) = *guard {
                        let _ = tx.send(event);
                    }
                }
            }
            0
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

pub struct Win32ListenerHandle {
    thread_id: u32,
    running: Arc<AtomicBool>,
}

impl Win32ListenerHandle {
    pub fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
        unsafe {
            PostThreadMessageW(self.thread_id, WM_QUIT, 0, 0);
        }
    }
}

pub fn spawn_win32_listener(event_tx: UnboundedSender<PlatformEvent>) -> Win32ListenerHandle {
    let running = Arc::new(AtomicBool::new(true));
    let running_clone = running.clone();

    let (tid_tx, tid_rx) = std::sync::mpsc::channel();

    std::thread::spawn(move || {
        let thread_id = unsafe { windows_sys::Win32::System::Threading::GetCurrentThreadId() };
        let _ = tid_tx.send(thread_id);

        unsafe {
            if let Ok(mut guard) = GLOBAL_TX.lock() {
                *guard = Some(event_tx);
            }

            let h_instance = GetModuleHandleW(std::ptr::null());
            let class_name = windows_sys::w!("ScreentimeMsgClass");

            let wnd_class = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                style: 0,
                lpfnWndProc: Some(wnd_proc),
                cbClsExtra: 0,
                cbWndExtra: 0,
                hInstance: h_instance,
                hIcon: std::ptr::null_mut(),
                hCursor: std::ptr::null_mut(),
                hbrBackground: std::ptr::null_mut(),
                lpszMenuName: std::ptr::null(),
                lpszClassName: class_name,
                hIconSm: std::ptr::null_mut(),
            };
            RegisterClassExW(&wnd_class);

            let hwnd = CreateWindowExW(
                0,
                class_name,
                windows_sys::w!("ScreentimeMsgWindow"),
                0,
                0,
                0,
                0,
                0,
                HWND_MESSAGE,
                std::ptr::null_mut(),
                h_instance,
                std::ptr::null(),
            );

            let power_notify: HPOWERNOTIFY = RegisterPowerSettingNotification(
                hwnd as _,
                &GUID_CONSOLE_DISPLAY_STATE as *const _ as _,
                DEVICE_NOTIFY_WINDOW_HANDLE,
            );
            WTSRegisterSessionNotification(hwnd, NOTIFY_FOR_THIS_SESSION);

            let hook = SetWinEventHook(
                EVENT_SYSTEM_FOREGROUND,
                EVENT_SYSTEM_FOREGROUND,
                std::ptr::null_mut(),
                Some(win_event_proc),
                0,
                0,
                WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
            );

            let mut msg: MSG = std::mem::zeroed();
            while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }

            if !hook.is_null() {
                UnhookWinEvent(hook);
            }
            WTSUnRegisterSessionNotification(hwnd);
            if power_notify != 0 {
                UnregisterPowerSettingNotification(power_notify);
            }
            if !hwnd.is_null() {
                DestroyWindow(hwnd);
            }

            if let Ok(mut guard) = GLOBAL_TX.lock() {
                *guard = None;
            }
        }
    });

    let thread_id = tid_rx.recv().expect("Failed to receive thread ID");
    Win32ListenerHandle {
        thread_id,
        running: running_clone,
    }
}
