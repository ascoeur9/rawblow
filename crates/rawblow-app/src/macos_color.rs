//! macOS 광색역(#122). 창의 CAMetalLayer.colorspace를 Display P3 또는 sRGB로 둔다.
//! 설정이 실패하면 레이어를 못 쓴 것으로 보고 sRGB로 디코드한다.
//! P3를 건 뒤에는 창이 놓인 화면 이름(NSScreen.localizedName)을 표시 이름으로 돌려준다.

#![allow(unexpected_cfgs)]

use objc::runtime::{Class, Object, BOOL, NO};
use objc::{msg_send, sel, sel_impl};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use rawblow_core::color::OsColor;
use std::ffi::c_void;

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    static kCGColorSpaceDisplayP3: *const c_void;
    static kCGColorSpaceSRGB: *const c_void;
    fn CGColorSpaceCreateWithName(name: *const c_void) -> *mut c_void;
    fn CGColorSpaceRelease(space: *mut c_void);
}

/// `wide`이면 Display P3를 걸고, 성공했을 때만 `layer_ok`. 꺼져 있으면 sRGB로 되돌린다.
pub fn probe(frame: &eframe::Frame, wide: bool) -> (OsColor, String) {
    let ok = set_layer_space(frame, wide);
    let name = if wide && ok { screen_name(frame).unwrap_or_default() } else { String::new() };
    (OsColor::MacOs { layer_ok: wide && ok }, name)
}

/// 창이 놓인 화면 이름. `localizedName`은 macOS 10.15부터라 없으면 None.
fn screen_name(frame: &eframe::Frame) -> Option<String> {
    let view = ns_view(frame)?;
    unsafe {
        let window: *mut Object = msg_send![view, window];
        if window.is_null() {
            return None;
        }
        let screen: *mut Object = msg_send![window, screen];
        if screen.is_null() {
            return None;
        }
        let has: BOOL = msg_send![screen, respondsToSelector: sel!(localizedName)];
        if has == NO {
            return None;
        }
        let name: *mut Object = msg_send![screen, localizedName];
        if name.is_null() {
            return None;
        }
        let utf8: *const std::os::raw::c_char = msg_send![name, UTF8String];
        if utf8.is_null() {
            return None;
        }
        let s = std::ffi::CStr::from_ptr(utf8).to_string_lossy().into_owned();
        (!s.is_empty()).then_some(s)
    }
}

fn ns_view(frame: &eframe::Frame) -> Option<*mut Object> {
    let handle = frame.window_handle().ok()?;
    match handle.as_raw() {
        RawWindowHandle::AppKit(h) => Some(h.ns_view.as_ptr() as *mut Object),
        _ => None,
    }
}

fn set_layer_space(frame: &eframe::Frame, p3: bool) -> bool {
    let Some(view) = ns_view(frame) else {
        return false;
    };
    unsafe {
        let layer: *mut Object = msg_send![view, layer];
        if layer.is_null() {
            return false;
        }
        let Some(cls) = Class::get("CAMetalLayer") else {
            return false;
        };
        let is_metal: BOOL = msg_send![layer, isKindOfClass: cls];
        if is_metal == NO {
            return false;
        }
        let name = if p3 { kCGColorSpaceDisplayP3 } else { kCGColorSpaceSRGB };
        let space = CGColorSpaceCreateWithName(name);
        if space.is_null() {
            return false;
        }
        let _: () = msg_send![layer, setColorspace: space];
        CGColorSpaceRelease(space);
        true
    }
}
