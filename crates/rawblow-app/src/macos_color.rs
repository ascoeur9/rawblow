//! macOS 광색역(#122). 창의 CAMetalLayer.colorspace를 Display P3 또는 sRGB로 둔다.
//! 설정이 실패하면 레이어를 못 쓴 것으로 보고 sRGB로 디코드한다.

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
    (OsColor::MacOs { layer_ok: wide && ok }, String::new())
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
