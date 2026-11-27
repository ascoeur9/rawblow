//! macOS 광색역(#122). 창의 CAMetalLayer.colorspace를 Display P3 또는 sRGB로 둔다.
//! 설정이 실패하면 레이어를 못 쓴 것으로 보고 sRGB로 디코드한다.
//! P3를 건 뒤에는 창이 놓인 화면 이름을 돌려준다. 내장 화면(CGDisplayIsBuiltin)은 앱 언어의
//! 「내장 디스플레이」, 외장은 NSScreen.localizedName(제품명)이다. localizedName은 앱 언어가 아니라
//! 번들 지역화를 따라서, 내장 화면에 쓰면 한국어 맥에서도 "Built-in Retina Display"가 된다.
//! 윈도우는 EDID 이름이 있으면 내장 패널도 그 이름을 쓴다(windows_color::monitor_name). 맥 내장
//! 화면 이름은 EDID가 아니라 OS 문구라서 규칙이 다르다.

#![allow(unexpected_cfgs)]

use crate::i18n::tr;
use objc::runtime::{Class, Object, BOOL, NO};
use objc::{class, msg_send, sel, sel_impl};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use rawblow_core::color::OsColor;
use rawblow_core::config::Lang;
use std::ffi::c_void;

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    static kCGColorSpaceDisplayP3: *const c_void;
    static kCGColorSpaceSRGB: *const c_void;
    fn CGColorSpaceCreateWithName(name: *const c_void) -> *mut c_void;
    fn CGColorSpaceRelease(space: *mut c_void);
    // CGDirectDisplayID는 uint32_t. boolean_t는 32비트(arm64 int, x86_64 unsigned int)라 0과만 비교한다.
    fn CGDisplayIsBuiltin(display: u32) -> u32;
}

/// `wide`이면 Display P3를 걸고, 성공했을 때만 `layer_ok`. 꺼져 있으면 sRGB로 되돌린다.
pub fn probe(frame: &eframe::Frame, wide: bool, lang: Lang) -> (OsColor, String) {
    let ok = set_layer_space(frame, wide);
    let on = wide && ok;
    let name = if on {
        // 매 프레임 만드는 NSString 키를 이벤트 루프 풀에 기대지 않고 바로 비운다.
        objc::rc::autoreleasepool(|| screen_name(frame, tr(lang, "내장 디스플레이")))
    } else {
        String::new()
    };
    (OsColor::MacOs { layer_ok: on }, name)
}

/// 창이 놓인 화면의 표시 이름. 화면을 못 찾으면 빈 문자열(→ 「현재 출력: Display P3」).
fn screen_name(frame: &eframe::Frame, builtin_label: &str) -> String {
    let Some(screen) = window_screen(frame) else {
        return String::new();
    };
    unsafe {
        let builtin = display_id(screen).is_some_and(|id| CGDisplayIsBuiltin(id) != 0);
        let localized = if builtin { None } else { localized_name(screen) };
        screen_label(localized.as_deref(), builtin, builtin_label)
    }
}

fn window_screen(frame: &eframe::Frame) -> Option<*mut Object> {
    let view = ns_view(frame)?;
    unsafe {
        let window: *mut Object = msg_send![view, window];
        if window.is_null() {
            return None;
        }
        let screen: *mut Object = msg_send![window, screen];
        (!screen.is_null()).then_some(screen)
    }
}

/// `localizedName`(macOS 10.15+). 번들 지역화를 따르므로 외장 제품명에만 쓴다.
unsafe fn localized_name(screen: *mut Object) -> Option<String> {
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

/// deviceDescription["NSScreenNumber"] → CGDirectDisplayID. 없거나 NSNumber가 아니거나 0이면 None.
/// objc 0.2는 반환 타입을 검사하지 않으므로 unsignedIntValue를 u32로 정확히 받는다.
unsafe fn display_id(screen: *mut Object) -> Option<u32> {
    let desc: *mut Object = msg_send![screen, deviceDescription];
    if desc.is_null() {
        return None;
    }
    let key: *mut Object = msg_send![class!(NSString), stringWithUTF8String: c"NSScreenNumber".as_ptr()];
    if key.is_null() {
        return None;
    }
    let num: *mut Object = msg_send![desc, objectForKey: key];
    if num.is_null() {
        return None;
    }
    let is_num: BOOL = msg_send![num, isKindOfClass: class!(NSNumber)];
    if is_num == NO {
        return None;
    }
    let id: u32 = msg_send![num, unsignedIntValue];
    (id != 0).then_some(id)
}

/// 내장이면 앱 언어 문구. 외장이거나 판별에 실패하면 OS 이름(앞뒤 공백 제거), 그것도 없으면 "".
/// 판별 실패를 OS 이름으로 두는 것은 예전 동작과 같아 나빠지는 경우가 없게 하려는 것이다.
fn screen_label(localized: Option<&str>, builtin: bool, builtin_label: &str) -> String {
    if builtin {
        builtin_label.to_string()
    } else {
        localized.map(str::trim).unwrap_or_default().to_string()
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

#[cfg(test)]
mod tests {
    use super::screen_label;
    use crate::i18n::tr;
    use rawblow_core::config::Lang;

    #[test]
    fn builtin_screen_uses_app_label_over_os_name() {
        // 내장으로 판별되면 OS 이름(번들 지역화를 따름)이 무엇이든 앱 언어 문구를 쓴다.
        // 번역값 자체는 i18n 테스트가 고정한다.
        let en = tr(Lang::En, "내장 디스플레이");
        assert_eq!(screen_label(None, true, en), en);
        assert_eq!(screen_label(Some("내장 Retina 디스플레이"), true, en), en);
    }

    #[test]
    fn external_or_unknown_screen_keeps_os_name() {
        assert_eq!(screen_label(Some("DELL U2723QE"), false, "내장 디스플레이"), "DELL U2723QE");
        assert_eq!(screen_label(Some("  Studio Display "), false, "내장 디스플레이"), "Studio Display");
        assert_eq!(screen_label(Some("  "), false, "내장 디스플레이"), "");
        assert_eq!(screen_label(None, false, "내장 디스플레이"), "");
    }
}
