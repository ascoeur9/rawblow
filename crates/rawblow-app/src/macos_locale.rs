//! macOS 단독 바이너리(번들 X)에서 AppKit 다이얼로그가 시스템 언어를 따르도록 강제.
//!
//! 배경: `.app` 번들과 Info.plist의 `CFBundleLocalizations`가 없으면 AppKit이 앱의
//! 선호 언어를 결정하지 못해 `NSOpenPanel` 등 시스템 다이얼로그가 한국어 macOS에서도
//! 영어로 표시된다(이슈 #12). 시작 시 `NSLocale.preferredLanguages`를 `NSUserDefaults`의
//! `AppleLanguages` 키에 써 두면 AppKit이 자기 프레임워크의 `.lproj`에서 올바른 로캘을
//! 고른다. rfd가 NSOpenPanel을 호출하기 **전에** 한 번만 부르면 된다.

use objc::runtime::Object;
use objc::{class, msg_send, sel, sel_impl};

pub fn align_with_system() {
    unsafe {
        let langs: *mut Object = msg_send![class!(NSLocale), preferredLanguages];
        if langs.is_null() {
            return;
        }
        let key_cstr = b"AppleLanguages\0".as_ptr() as *const std::os::raw::c_char;
        let key: *mut Object = msg_send![class!(NSString), stringWithUTF8String: key_cstr];
        let defaults: *mut Object = msg_send![class!(NSUserDefaults), standardUserDefaults];
        let _: () = msg_send![defaults, setObject: langs, forKey: key];
    }
}
