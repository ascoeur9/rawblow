//! Windows 광색역(#122). 창 중심이 있는 모니터의 ICC를 읽고, ACM/HDR이면 sRGB로 둔다.
//! 스왑체인은 그대로 8비트 sRGB다. 조회가 실패하면 ICC 없이 넘어가 sRGB로 폴백한다.
//! 표시 이름은 `모니터 이름 · ICC 파일 이름`이다. EDID에 이름이 없는 노트북 내장 패널은
//! 「내장 디스플레이」로 부르고, 그 밖에 이름을 못 얻으면 ICC 파일 이름만 쓴다.

use crate::i18n::tr;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use rawblow_core::config::Lang;
use rawblow_core::color::OsColor;
use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;
use std::path::Path;
use windows_sys::Win32::Devices::Display::{
    DisplayConfigGetDeviceInfo, GetDisplayConfigBufferSizes, QueryDisplayConfig,
    DISPLAYCONFIG_DEVICE_INFO_GET_ADVANCED_COLOR_INFO, DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME,
    DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME, DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO,
    DISPLAYCONFIG_MODE_INFO, DISPLAYCONFIG_PATH_INFO, DISPLAYCONFIG_SOURCE_DEVICE_NAME,
    DISPLAYCONFIG_OUTPUT_TECHNOLOGY_DISPLAYPORT_EMBEDDED, DISPLAYCONFIG_OUTPUT_TECHNOLOGY_INTERNAL,
    DISPLAYCONFIG_OUTPUT_TECHNOLOGY_LVDS, DISPLAYCONFIG_OUTPUT_TECHNOLOGY_UDI_EMBEDDED,
    DISPLAYCONFIG_TARGET_DEVICE_NAME, DISPLAYCONFIG_VIDEO_OUTPUT_TECHNOLOGY, QDC_ONLY_ACTIVE_PATHS,
};
use windows_sys::Win32::Foundation::{ERROR_SUCCESS, POINT, RECT};
use windows_sys::Win32::Graphics::Gdi::{
    CreateDCW, DeleteDC, GetMonitorInfoW, MonitorFromPoint, MONITORINFOEXW,
    MONITOR_DEFAULTTONEAREST,
};
use windows_sys::Win32::UI::ColorSystem::GetICMProfileW;
use windows_sys::Win32::UI::WindowsAndMessaging::GetWindowRect;

pub fn probe(frame: &eframe::Frame, lang: Lang) -> (OsColor, String) {
    let Some(hwnd) = window_hwnd(frame) else {
        return (OsColor::Windows { icc: None, acm_or_hdr: false }, String::new());
    };
    let path = monitor_path(hwnd);
    if path.as_ref().is_some_and(path_is_acm_or_hdr) {
        return (OsColor::Windows { icc: None, acm_or_hdr: true }, String::new());
    }
    match monitor_icc(hwnd) {
        Some((bytes, profile)) => {
            let (friendly, internal) = path.as_ref().map(path_target_name).unwrap_or_default();
            let monitor = monitor_name(&friendly, internal, tr(lang, "내장 디스플레이"));
            (
                OsColor::Windows { icc: Some(bytes), acm_or_hdr: false },
                output_label(&monitor, &profile),
            )
        }
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
        if mon == 0 {
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

/// EDID 이름. 비었는데 내장 연결이면 `builtin`(「내장 디스플레이」), 아니면 빈 문자열.
fn monitor_name(friendly: &str, internal: bool, builtin: &str) -> String {
    match friendly.trim() {
        "" if internal => builtin.to_string(),
        f => f.to_string(),
    }
}

/// 노트북 내장 패널 연결(eDP·LVDS·내장 UDI·OS가 내장으로 표시한 경로).
fn is_internal_output(tech: DISPLAYCONFIG_VIDEO_OUTPUT_TECHNOLOGY) -> bool {
    matches!(
        tech,
        DISPLAYCONFIG_OUTPUT_TECHNOLOGY_INTERNAL
            | DISPLAYCONFIG_OUTPUT_TECHNOLOGY_DISPLAYPORT_EMBEDDED
            | DISPLAYCONFIG_OUTPUT_TECHNOLOGY_UDI_EMBEDDED
            | DISPLAYCONFIG_OUTPUT_TECHNOLOGY_LVDS
    )
}

/// `모니터 이름 · ICC 파일 이름`. 한쪽이 비면 남은 쪽만.
fn output_label(friendly: &str, profile: &str) -> String {
    match (friendly.trim(), profile.trim()) {
        ("", p) => p.to_string(),
        (f, "") => f.to_string(),
        (f, p) => format!("{f} · {p}"),
    }
}

/// 창의 모니터(GDI 이름)에 물린 활성 디스플레이 경로. 조회 실패는 None.
fn monitor_path(hwnd: windows_sys::Win32::Foundation::HWND) -> Option<DISPLAYCONFIG_PATH_INFO> {
    let device = monitor_device(hwnd)?;
    unsafe {
        let mut n_path = 0u32;
        let mut n_mode = 0u32;
        if GetDisplayConfigBufferSizes(QDC_ONLY_ACTIVE_PATHS, &mut n_path, &mut n_mode) != ERROR_SUCCESS
        {
            return None;
        }
        if n_path == 0 || n_path > 64 {
            return None;
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
            return None;
        }
        for path in paths.iter().take(n_path as usize) {
            let mut src: DISPLAYCONFIG_SOURCE_DEVICE_NAME = std::mem::zeroed();
            src.header.r#type = DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME;
            src.header.size = std::mem::size_of::<DISPLAYCONFIG_SOURCE_DEVICE_NAME>() as u32;
            src.header.adapterId = path.sourceInfo.adapterId;
            src.header.id = path.sourceInfo.id;
            if DisplayConfigGetDeviceInfo(&mut src.header) != ERROR_SUCCESS as i32 {
                continue;
            }
            if wide_eq(&src.viewGdiDeviceName, &device) {
                return Some(*path);
            }
        }
    }
    None
}

/// ACM/HDR(`advancedColorEnabled`)이면 true. 조회 실패는 false(ICC를 시도한다).
fn path_is_acm_or_hdr(path: &DISPLAYCONFIG_PATH_INFO) -> bool {
    unsafe {
        let mut adv: DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO = std::mem::zeroed();
        adv.header.r#type = DISPLAYCONFIG_DEVICE_INFO_GET_ADVANCED_COLOR_INFO;
        adv.header.size = std::mem::size_of::<DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO>() as u32;
        adv.header.adapterId = path.targetInfo.adapterId;
        adv.header.id = path.targetInfo.id;
        if DisplayConfigGetDeviceInfo(&mut adv.header) != ERROR_SUCCESS as i32 {
            return false;
        }
        // bit 0 supported, bit 1 enabled.
        adv.Anonymous.value & 2 != 0
    }
}

/// 모니터 이름(EDID, 예: "DELL U2723QE")과 내장 연결 여부. 조회 실패는 `("", false)`.
fn path_target_name(path: &DISPLAYCONFIG_PATH_INFO) -> (String, bool) {
    unsafe {
        let mut tgt: DISPLAYCONFIG_TARGET_DEVICE_NAME = std::mem::zeroed();
        tgt.header.r#type = DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME;
        tgt.header.size = std::mem::size_of::<DISPLAYCONFIG_TARGET_DEVICE_NAME>() as u32;
        tgt.header.adapterId = path.targetInfo.adapterId;
        tgt.header.id = path.targetInfo.id;
        if DisplayConfigGetDeviceInfo(&mut tgt.header) != ERROR_SUCCESS as i32 {
            return (String::new(), false);
        }
        let name = &tgt.monitorFriendlyDeviceName;
        let end = name.iter().position(|&c| c == 0).unwrap_or(name.len());
        (String::from_utf16_lossy(&name[..end]), is_internal_output(tgt.outputTechnology))
    }
}

fn wide_eq(a: &[u16], b: &[u16]) -> bool {
    let end = |s: &[u16]| s.iter().position(|&c| c == 0).unwrap_or(s.len());
    a[..end(a)] == b[..end(b)]
}

fn monitor_icc(hwnd: windows_sys::Win32::Foundation::HWND) -> Option<(Vec<u8>, String)> {
    let device = monitor_device(hwnd)?;
    unsafe {
        let dc = CreateDCW(std::ptr::null(), device.as_ptr(), std::ptr::null(), std::ptr::null());
        if dc == 0 {
            return None;
        }
        let mut chars = 0u32;
        let _ = GetICMProfileW(dc, &mut chars, std::ptr::null_mut());
        if !(2..=4096).contains(&chars) {
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
        let wide = OsString::from_wide(&buf[..end]);
        let path = Path::new(&wide);
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

#[cfg(test)]
mod tests {
    use super::{is_internal_output, monitor_name, output_label};
    use windows_sys::Win32::Devices::Display::{
        DISPLAYCONFIG_OUTPUT_TECHNOLOGY_DISPLAYPORT_EMBEDDED,
        DISPLAYCONFIG_OUTPUT_TECHNOLOGY_DISPLAYPORT_EXTERNAL, DISPLAYCONFIG_OUTPUT_TECHNOLOGY_HDMI,
        DISPLAYCONFIG_OUTPUT_TECHNOLOGY_INTERNAL, DISPLAYCONFIG_OUTPUT_TECHNOLOGY_LVDS,
        DISPLAYCONFIG_OUTPUT_TECHNOLOGY_UDI_EMBEDDED,
    };

    #[test]
    fn output_label_joins_monitor_and_profile() {
        assert_eq!(output_label("DELL U2723QE", "U2723QE"), "DELL U2723QE · U2723QE");
        assert_eq!(output_label("", "sRGB Color Space Profile"), "sRGB Color Space Profile");
        assert_eq!(output_label("  ", "U2723QE"), "U2723QE");
        assert_eq!(output_label("DELL U2723QE", ""), "DELL U2723QE");
    }

    #[test]
    fn nameless_internal_panel_is_builtin_display() {
        for t in [
            DISPLAYCONFIG_OUTPUT_TECHNOLOGY_INTERNAL,
            DISPLAYCONFIG_OUTPUT_TECHNOLOGY_DISPLAYPORT_EMBEDDED,
            DISPLAYCONFIG_OUTPUT_TECHNOLOGY_UDI_EMBEDDED,
            DISPLAYCONFIG_OUTPUT_TECHNOLOGY_LVDS,
        ] {
            assert!(is_internal_output(t));
        }
        assert!(!is_internal_output(DISPLAYCONFIG_OUTPUT_TECHNOLOGY_HDMI));
        assert!(!is_internal_output(DISPLAYCONFIG_OUTPUT_TECHNOLOGY_DISPLAYPORT_EXTERNAL));

        let builtin = "내장 디스플레이";
        let label = |f: &str, internal: bool| {
            output_label(&monitor_name(f, internal, builtin), "250cc_25-06-14")
        };
        assert_eq!(label("", true), "내장 디스플레이 · 250cc_25-06-14");
        assert_eq!(label("  ", true), "내장 디스플레이 · 250cc_25-06-14");
        // EDID 이름이 있으면 내장이어도 그 이름을 쓴다.
        assert_eq!(label("LEN P27", true), "LEN P27 · 250cc_25-06-14");
        // 외장인데 이름이 없으면 ICC 이름만.
        assert_eq!(label("", false), "250cc_25-06-14");
    }
}
