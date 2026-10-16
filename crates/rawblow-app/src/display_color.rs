//! 이 창의 표시 색(#122). Linux는 항상 sRGB. macOS·Windows만 창을 조사한다.

use rawblow_core::color::OsColor;

pub fn probe(frame: &eframe::Frame, wide: bool) -> (OsColor, String) {
    #[cfg(target_os = "macos")]
    {
        return crate::macos_color::probe(frame, wide);
    }
    #[cfg(target_os = "windows")]
    {
        let _ = wide;
        return crate::windows_color::probe(frame);
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = (frame, wide);
        (OsColor::Linux, String::new())
    }
}
