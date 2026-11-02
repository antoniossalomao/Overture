use overture_core::MonitorDescriptor;

/// Default Windows Application User Model ID for taskbar grouping and MSI shortcut matching.
pub const OVERTURE_APP_USER_MODEL_ID: &str = "Overture.WorshipProjection.App.1";

#[cfg(target_os = "windows")]
mod imp {
    use super::*;
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::{BOOL, HWND, LPARAM, RECT, TRUE};
    use windows_sys::Win32::Graphics::Gdi::{
        EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFOEXW,
    };
    use windows_sys::Win32::UI::HiDpi::{
        DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext,
    };
    use windows_sys::Win32::UI::Shell::SetCurrentProcessExplicitAppUserModelID;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GWL_EXSTYLE, GWL_STYLE, HWND_TOPMOST, MONITORINFOF_PRIMARY, SWP_FRAMECHANGED,
        SWP_NOACTIVATE, SWP_SHOWWINDOW, SetWindowLongPtrW, SetWindowPos, WS_EX_APPWINDOW,
        WS_EX_TOPMOST, WS_POPUP, WS_VISIBLE,
    };

    pub fn init_windows_process_identity(app_id: &str) {
        unsafe {
            let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
            let wide: Vec<u16> = OsStr::new(app_id)
                .encode_wide()
                .chain(std::iter::once(0))
                .collect();
            let _ = SetCurrentProcessExplicitAppUserModelID(wide.as_ptr());
        }
    }

    unsafe extern "system" fn monitor_enum_proc(
        hmonitor: HMONITOR,
        _hdc: HDC,
        _lprect: *mut RECT,
        lparam: LPARAM,
    ) -> BOOL {
        let list = unsafe { &mut *(lparam as *mut Vec<MonitorDescriptor>) };
        let mut info: MONITORINFOEXW = unsafe { std::mem::zeroed() };
        info.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;

        if unsafe { GetMonitorInfoW(hmonitor, &mut info as *mut _ as *mut _) } != 0 {
            let rc = info.monitorInfo.rcMonitor;
            let width = (rc.right - rc.left).max(1) as u32;
            let height = (rc.bottom - rc.top).max(1) as u32;
            let is_primary = (info.monitorInfo.dwFlags & MONITORINFOF_PRIMARY) != 0;

            let nul_pos = info
                .szDevice
                .iter()
                .position(|&c| c == 0)
                .unwrap_or(info.szDevice.len());
            let system_name = String::from_utf16_lossy(&info.szDevice[..nul_pos]);
            let index = list.len();
            let friendly_name = if is_primary {
                format!("Monitor {} (Principal — {}×{})", index + 1, width, height)
            } else {
                format!("Monitor {} ({}×{})", index + 1, width, height)
            };

            list.push(MonitorDescriptor {
                index,
                system_name,
                friendly_name,
                x: rc.left,
                y: rc.top,
                width,
                height,
                scale_factor: 1.0,
                is_primary,
            });
        }
        TRUE
    }

    pub fn enumerate_monitors() -> Vec<MonitorDescriptor> {
        let mut monitors: Vec<MonitorDescriptor> = Vec::new();
        unsafe {
            let _ = EnumDisplayMonitors(
                std::ptr::null_mut(),
                std::ptr::null(),
                Some(monitor_enum_proc),
                &mut monitors as *mut Vec<MonitorDescriptor> as LPARAM,
            );
        }
        monitors
    }

    pub fn apply_win32_fullscreen(hwnd_raw: isize, mon: &MonitorDescriptor, topmost: bool) {
        let hwnd = hwnd_raw as HWND;
        unsafe {
            let style = (WS_POPUP | WS_VISIBLE) as isize;
            let ex_style = if topmost {
                (WS_EX_APPWINDOW | WS_EX_TOPMOST) as isize
            } else {
                WS_EX_APPWINDOW as isize
            };
            SetWindowLongPtrW(hwnd, GWL_STYLE, style);
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex_style);
            let z_order = if topmost {
                HWND_TOPMOST
            } else {
                std::ptr::null_mut()
            };
            SetWindowPos(
                hwnd,
                z_order,
                mon.x,
                mon.y,
                mon.width as i32,
                mon.height as i32,
                SWP_FRAMECHANGED | SWP_SHOWWINDOW | SWP_NOACTIVATE,
            );
        }
    }
}

#[cfg(not(target_os = "windows"))]
mod imp {
    use super::*;
    pub fn init_windows_process_identity(_app_id: &str) {}
    pub fn enumerate_monitors() -> Vec<MonitorDescriptor> {
        vec![MonitorDescriptor {
            index: 0,
            system_name: "DISPLAY1".to_string(),
            friendly_name: "Monitor 1 (1920x1080)".to_string(),
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
            scale_factor: 1.0,
            is_primary: true,
        }]
    }
    pub fn apply_win32_fullscreen(_hwnd_raw: isize, _mon: &MonitorDescriptor, _topmost: bool) {}
}

pub use imp::{apply_win32_fullscreen, enumerate_monitors, init_windows_process_identity};
