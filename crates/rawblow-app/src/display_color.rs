//! 이 창의 표시 색(#122). Linux는 항상 sRGB. macOS·Windows만 창을 조사한다.

use rawblow_core::color::OsColor;
use rawblow_core::config::Lang;

/// `lang`은 내장 화면을 앱 언어의 「내장 디스플레이」로 부를 때 쓴다(macOS·Windows).
pub fn probe(frame: &eframe::Frame, wide: bool, lang: Lang) -> (OsColor, String) {
    #[cfg(target_os = "macos")]
    {
        crate::macos_color::probe(frame, wide, lang)
    }
    #[cfg(target_os = "windows")]
    {
        let _ = wide;
        crate::windows_color::probe(frame, lang)
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = (frame, wide, lang);
        (OsColor::Linux, String::new())
    }
}
