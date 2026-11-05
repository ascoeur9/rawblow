//! Windows 광색역(#122). 창 중심이 있는 모니터의 ICC를 읽고, ACM/HDR이면 sRGB로 둔다.
//! 스왑체인은 그대로 8비트 sRGB다. 조회가 실패하면 ICC 없이 넘어가 sRGB로 폴백한다.

use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use rawblow_core::color::OsColor;
use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;
use std::path::Path;
use windows_sys::Win32::Devices::Display::{
    DisplayConfigGetDeviceInfo, GetDisplayConfigBufferSizes, QueryDisplayConfig,
    DISPLAYCONFIG_DEVICE_INFO_GET_ADVANCED_COLOR_INFO, DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME,
    DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO, DISPLAYCONFIG_MODE_INFO, DISPLAYCONFIG_PATH_INFO,
    DISPLAYCONFIG_SOURCE_DEVICE_NAME, QDC_ONLY_ACTIVE_PATHS,
};
use windows_sys::Win32::Foundation::{ERROR_SUCCESS, POINT, RECT};
use windows_sys::Win32::Graphics::Gdi::{
    CreateDCW, DeleteDC, GetICMProfileW, GetMonitorInfoW, MONITORINFOEXW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetWindowRect, MonitorFromPoint, MONITOR_DEFAULTTONEAREST,
};

pub fn probe(frame: &eframe::Frame) -> (OsColor, String) {
    let Some(hwnd) = window_hwnd(frame) else {
        return (OsColor::Windows { icc: None, acm_or_hdr: false }, String::new());
    };
    if monitor_is_acm_or_hdr(hwnd) {
        return (OsColor::Windows { icc: None, acm_or_hdr: true }, String::new());
    }
    match monitor_icc(hwnd) {
        Some((bytes, name)) => (
            OsColor::Windows { icc: Some(bytes), acm_or_hdr: false },
            name,
        ),
        None => (OsColor::Windows { icc: None, acm_or_hdr: false }, String::new()),
    }
}

fn window_hwnd(frame: &eframe::Frame) -> Option<windows_sys::Win32::Foundation::HWND> {
    let handle = frame.window_handle().ok()?;
    match handle.as_raw() {
        RawWindowHandle::Win32(h) => Some(h.hwnd.get() as _),
        _ => None,
    }
}

fn monitor_device(hwnd: windows_sys::Win32::Foundation::HWND) -> Option<[u16; 32]> {
    unsafe {
        let mut rect = RECT { left: 0, top: 0, right: 0, bottom: 0 };
        if GetWindowRect(hwnd, &mut rect) == 0 {
            return None;
        }
        let pt = POINT {
            x: rect.left + (rect.right - rect.left) / 2,
            y: rect.top + (rect.bottom - rect.top) / 2,
        };
        let mon = MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST);
        if mon.is_null() {
            return None;
        }
        let mut info: MONITORINFOEXW = std::mem::zeroed();
        info.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
        if GetMonitorInfoW(mon, &mut info as *mut _ as *mut _) == 0 {
            return None;
        }
        Some(info.szDevice)
    }
}

/// ACM/HDR(`advancedColorEnabled`)이면 true. 조회 실패는 false(ICC를 시도한다).
fn monitor_is_acm_or_hdr(hwnd: windows_sys::Win32::Foundation::HWND) -> bool {
    let Some(device) = monitor_device(hwnd) else {
        return false;
    };
    unsafe {
        let mut n_path = 0u32;
        let mut n_mode = 0u32;
        if GetDisplayConfigBufferSizes(QDC_ONLY_ACTIVE_PATHS, &mut n_path, &mut n_mode) != ERROR_SUCCESS
        {
            return false;
        }
        if n_path == 0 || n_path > 64 {
            return false;
        }
        let mut paths = vec![std::mem::zeroed::<DISPLAYCONFIG_PATH_INFO>(); n_path as usize];
        let mut modes = vec![std::mem::zeroed::<DISPLAYCONFIG_MODE_INFO>(); n_mode as usize];
        if QueryDisplayConfig(
            QDC_ONLY_ACTIVE_PATHS,
            &mut n_path,
            paths.as_mut_ptr(),
            &mut n_mode,
            modes.as_mut_ptr(),
            std::ptr::null_mut(),
        ) != ERROR_SUCCESS
        {
            return false;
        }
        for path in paths.iter().take(n_path as usize) {
            let mut src: DISPLAYCONFIG_SOURCE_DEVICE_NAME = std::mem::zeroed();
            src.header.r#type = DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME;
            src.header.size = std::mem::size_of::<DISPLAYCONFIG_SOURCE_DEVICE_NAME>() as u32;
            src.header.adapterId = path.sourceInfo.adapterId;
            src.header.id = path.sourceInfo.id;
            if DisplayConfigGetDeviceInfo(&mut src.header) != ERROR_SUCCESS {
                continue;
            }
            if !wide_eq(&src.viewGdiDeviceName, &device) {
                continue;
            }
            let mut adv: DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO = std::mem::zeroed();
            adv.header.r#type = DISPLAYCONFIG_DEVICE_INFO_GET_ADVANCED_COLOR_INFO;
            adv.header.size = std::mem::size_of::<DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO>() as u32;
            adv.header.adapterId = path.targetInfo.adapterId;
            adv.header.id = path.targetInfo.id;
            if DisplayConfigGetDeviceInfo(&mut adv.header) != ERROR_SUCCESS {
                return false;
            }
            // bit 0 supported, bit 1 enabled.
            return adv.Anonymous.value & 2 != 0;
        }
    }
    false
}

fn wide_eq(a: &[u16], b: &[u16]) -> bool {
    let end = |s: &[u16]| s.iter().position(|&c| c == 0).unwrap_or(s.len());
    a[..end(a)] == b[..end(b)]
}

fn monitor_icc(hwnd: windows_sys::Win32::Foundation::HWND) -> Option<(Vec<u8>, String)> {
    let device = monitor_device(hwnd)?;
    unsafe {
        let dc = CreateDCW(std::ptr::null(), device.as_ptr(), std::ptr::null(), std::ptr::null());
        if dc.is_null() {
            return None;
        }
        let mut chars = 0u32;
        let _ = GetICMProfileW(dc, &mut chars, std::ptr::null_mut());
        if chars < 2 || chars > 4096 {
            let _ = DeleteDC(dc);
            return None;
        }
        let mut buf = vec![0u16; chars as usize];
        let ok = GetICMProfileW(dc, &mut chars, buf.as_mut_ptr());
        let _ = DeleteDC(dc);
        if ok == 0 {
            return None;
        }
        let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        let path = Path::new(&OsString::from_wide(&buf[..end]));
        let bytes = std::fs::read(path).ok()?;
        if bytes.is_empty() {
            return None;
        }
        let name = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        Some((bytes, name))
    }
}
