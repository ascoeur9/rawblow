//! macOS 단독 바이너리(번들 X)에서 AppKit 다이얼로그가 시스템 언어를 따르도록 강제.
//!
//! 배경: `.app` 번들과 Info.plist의 `CFBundleLocalizations`가 없으면 AppKit이 앱의
//! 선호 언어를 결정하지 못해 `NSOpenPanel` 등 시스템 다이얼로그가 한국어 macOS에서도
//! 영어로 표시된다(이슈 #12). 시작 시 `NSLocale.preferredLanguages`를 `NSUserDefaults`의
//! `AppleLanguages` 키에 써 두면 AppKit이 자기 프레임워크의 `.lproj`에서 올바른 로캘을
//! 고른다. rfd가 NSOpenPanel을 호출하기 **전에** 한 번만 부르면 된다.

// objc 0.2 의 msg_send!/sel_impl! 매크로가 확장 중 `cfg(feature = "cargo-clippy")` 를 쓴다.
// 우리 크레이트에 없는 feature 라 rustc 가 unexpected_cfgs 로 경고하지만, 외부 매크로라
// 우리 쪽에서 고칠 수 없다. 이 모듈에서만 끈다.
#![allow(unexpected_cfgs)]

use objc::runtime::Object;
use objc::{class, msg_send, sel, sel_impl};

pub fn align_with_system() {
    unsafe {
        let langs: *mut Object = msg_send![class!(NSLocale), preferredLanguages];
        if langs.is_null() {
            return;
        }
        let key_cstr: *const std::os::raw::c_char = c"AppleLanguages".as_ptr();
        let key: *mut Object = msg_send![class!(NSString), stringWithUTF8String: key_cstr];
        let defaults: *mut Object = msg_send![class!(NSUserDefaults), standardUserDefaults];
        let _: () = msg_send![defaults, setObject:langs forKey:key];
    }
}

/// 시스템 선호 언어 코드(예: "ko-KR", "ja-JP", "en-US")의 첫 항목. UI 언어 자동 선택(#30)에 쓴다.
pub fn preferred_language() -> Option<String> {
    unsafe {
        let langs: *mut Object = msg_send![class!(NSLocale), preferredLanguages];
        if langs.is_null() {
            return None;
        }
        let count: usize = msg_send![langs, count];
        if count == 0 {
            return None;
        }
        let first: *mut Object = msg_send![langs, objectAtIndex: 0usize];
        if first.is_null() {
            return None;
        }
        let utf8: *const std::os::raw::c_char = msg_send![first, UTF8String];
        if utf8.is_null() {
            return None;
        }
        Some(std::ffi::CStr::from_ptr(utf8).to_string_lossy().into_owned())
    }
}
