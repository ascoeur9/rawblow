//! CJK 폰트 런타임 로드. egui 기본 폰트는 라틴만 포함하므로, OS 폰트
//! 디렉토리에서 한국어/일본어 폰트를 찾아 등록한다(임베드 대신 → 빌드 단순).

use egui::{FontData, FontDefinitions, FontFamily};

/// 플랫폼별 CJK 폰트 후보 경로(우선순위 순).
fn cjk_candidates() -> Vec<&'static str> {
    #[cfg(target_os = "windows")]
    {
        vec![
            r"C:\Windows\Fonts\malgun.ttf",
            r"C:\Windows\Fonts\YuGothM.ttc",
            r"C:\Windows\Fonts\meiryo.ttc",
            r"C:\Windows\Fonts\msgothic.ttc",
        ]
    }
    #[cfg(target_os = "macos")]
    {
        vec![
            "/System/Library/Fonts/AppleSDGothicNeo.ttc",
            "/System/Library/Fonts/Hiragino Sans GB.ttc",
            "/Library/Fonts/Arial Unicode.ttf",
        ]
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        vec![
            "/usr/share/fonts/google-noto-cjk/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/NotoSansKR-Regular.otf",
            "/usr/share/fonts/google-noto/NotoSansKR-Regular.ttf",
        ]
    }
}

/// 첫 번째로 읽히는 CJK 폰트를 (이름, 바이트)로 반환.
fn load_cjk() -> Option<(String, Vec<u8>)> {
    for path in cjk_candidates() {
        if let Ok(bytes) = std::fs::read(path) {
            if !bytes.is_empty() {
                return Some(("cjk".to_string(), bytes));
            }
        }
    }
    None
}

/// CJK 폰트를 egui에 설치한다. 없으면 기본 폰트만(라틴) 사용.
pub fn install(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    if let Some((name, bytes)) = load_cjk() {
        // `Arc<T>: From<T>` 이므로 `.into()`가 egui 버전과 무관하게 동작.
        fonts
            .font_data
            .insert(name.clone(), FontData::from_owned(bytes).into());
        fonts
            .families
            .entry(FontFamily::Proportional)
            .or_default()
            .insert(0, name.clone());
        fonts
            .families
            .entry(FontFamily::Monospace)
            .or_default()
            .push(name);
    }
    ctx.set_fonts(fonts);
}
