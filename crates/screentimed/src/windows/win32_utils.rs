#![cfg(target_os = "windows")]

use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, BOOL, HANDLE, HWND, LPARAM};
use windows_sys::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_FORMAT, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    EnumChildWindows, GetClassNameW, GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId,
};

use super::events::decode_wide_string;
use super::uwp::{extract_process_name, is_uwp_container, UWP_CORE_WINDOW_CLASS};

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct LASTINPUTINFO {
    pub cbSize: u32,
    pub dwTime: u32,
}

#[link(name = "user32")]
extern "system" {
    pub fn SetWinEventHook(
        eventmin: u32,
        eventmax: u32,
        hmodwineventproc: isize,
        pfnwineventproc: Option<
            unsafe extern "system" fn(
                hwineventhook: isize,
                event: u32,
                hwnd: isize,
                idobject: i32,
                idchild: i32,
                ideventthread: u32,
                dwmseventtime: u32,
            ),
        >,
        idprocess: u32,
        idthread: u32,
        dwflags: u32,
    ) -> isize;

    pub fn UnhookWinEvent(hwineventhook: isize) -> i32;

    pub fn GetLastInputInfo(plii: *mut LASTINPUTINFO) -> i32;
}

#[link(name = "kernel32")]
extern "system" {
    pub fn GetTickCount() -> u32;
}

pub struct UwpChildSearch {
    pub host_pid: u32,
    pub found_pid: Option<u32>,
    pub found_hwnd: Option<HWND>,
}

unsafe extern "system" fn enum_child_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let search = &mut *(lparam as *mut UwpChildSearch);
    let mut child_pid = 0;
    GetWindowThreadProcessId(hwnd, &mut child_pid);

    if child_pid != 0 && child_pid != search.host_pid {
        let mut class_buf = [0u16; 256];
        let len = GetClassNameW(hwnd, class_buf.as_mut_ptr(), class_buf.len() as i32);
        if len > 0 {
            let class_name = String::from_utf16_lossy(&class_buf[..len as usize]);
            if class_name == UWP_CORE_WINDOW_CLASS {
                search.found_pid = Some(child_pid);
                search.found_hwnd = Some(hwnd);
                return 0; // Stop enumeration
            }
        }
    }
    1 // Continue enumeration
}

pub fn get_process_path_and_name(hwnd: HWND) -> Result<(u32, String, String), String> {
    let mut pid: u32 = 0;
    unsafe { GetWindowThreadProcessId(hwnd, &mut pid) };
    if pid == 0 {
        return Err("Invalid process ID (0)".into());
    }

    let h_process: HANDLE = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if h_process == 0 {
        return Err(format!("OpenProcess failed with error {}", unsafe {
            GetLastError()
        }));
    }

    let mut buf = [0u16; 1024];
    let mut size = buf.len() as u32;
    let success = unsafe {
        QueryFullProcessImageNameW(
            h_process,
            0 as PROCESS_NAME_FORMAT,
            buf.as_mut_ptr(),
            &mut size,
        )
    };
    unsafe { CloseHandle(h_process) };

    if success == 0 {
        return Err("QueryFullProcessImageNameW failed".into());
    }

    let full_path = String::from_utf16_lossy(&buf[..size as usize]);
    let app_name = extract_process_name(&full_path);

    Ok((pid, full_path, app_name))
}

pub fn resolve_real_process_for_hwnd(root_hwnd: HWND) -> (HWND, u32) {
    let mut root_pid = 0;
    unsafe { GetWindowThreadProcessId(root_hwnd, &mut root_pid) };

    let (_, _, app_name) = match get_process_path_and_name(root_hwnd) {
        Ok(res) => res,
        Err(_) => return (root_hwnd, root_pid),
    };

    if is_uwp_container(&app_name) {
        let mut search = UwpChildSearch {
            host_pid: root_pid,
            found_pid: None,
            found_hwnd: None,
        };
        unsafe {
            EnumChildWindows(
                root_hwnd,
                Some(enum_child_proc),
                &mut search as *mut _ as LPARAM,
            );
        }
        if let (Some(real_hwnd), Some(real_pid)) = (search.found_hwnd, search.found_pid) {
            return (real_hwnd, real_pid);
        }
    }

    (root_hwnd, root_pid)
}

pub fn get_window_title(hwnd: HWND) -> String {
    let len = unsafe { GetWindowTextLengthW(hwnd) };
    if len <= 0 {
        return String::new();
    }
    let mut buf = vec![0u16; (len + 1) as usize];
    let copied = unsafe { GetWindowTextW(hwnd, buf.as_mut_ptr(), buf.len() as i32) };
    if copied > 0 {
        decode_wide_string(&buf[..copied as usize])
    } else {
        String::new()
    }
}

pub fn get_idle_time_millis() -> Result<u32, &'static str> {
    let mut lii: LASTINPUTINFO = unsafe { std::mem::zeroed() };
    lii.cbSize = std::mem::size_of::<LASTINPUTINFO>() as u32;

    let ok = unsafe { GetLastInputInfo(&mut lii) };
    if ok == 0 {
        return Err("GetLastInputInfo failed");
    }

    let current_ticks: u32 = unsafe { GetTickCount() };
    let idle_ms: u32 = current_ticks.wrapping_sub(lii.dwTime);
    Ok(idle_ms)
}
