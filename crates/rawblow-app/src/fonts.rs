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
            // 변수폰트(VF) 패키지: 최신 Fedora/RHEL 기본 구성에 흔함(고정 Regular .ttc가 없을 수 있음).
            "/usr/share/fonts/google-noto-sans-cjk-vf-fonts/NotoSansCJK-VF.ttc",
            "/usr/share/fonts/google-noto-sans-cjk-vf-fonts/NotoSansCJKkr-VF.ttf",
            "/usr/share/fonts/google-noto-sans-cjk-fonts/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/google-noto-cjk/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/NotoSansKR-Regular.otf",
            "/usr/share/fonts/google-noto/NotoSansKR-Regular.ttf",
        ]
    }
}

/// 존재하는 CJK 폰트를 **여러 개**(폴백 체인용) (이름, 바이트)로 반환한다(최대 3개).
/// 단일 폰트만 쓰면 한국어 폰트(Windows malgun)에 일본어 신자체(黄/緑/青 등) 글리프가 없어
/// 두부(□)로 깨진다. 한국어 폰트를 앞에, 일본어 폰트를 뒤 폴백으로 두면 한 폰트에 없는 글리프가
/// 다음 폰트로 채워진다(언어 무관 자동). 후보 목록은 한국어 우선·일본어 폴백 순으로 정렬돼 있다.
fn load_cjk_fonts() -> Vec<(String, Vec<u8>)> {
    let mut out: Vec<(String, Vec<u8>)> = Vec::new();
    for path in cjk_candidates() {
        if let Ok(bytes) = std::fs::read(path) {
            if !bytes.is_empty() && !out.iter().any(|(_, b)| b.len() == bytes.len()) {
                out.push((format!("cjk{}", out.len()), bytes));
                if out.len() >= 3 {
                    break;
                }
            }
        }
    }
    out
}

/// CJK 폰트를 egui에 설치한다. 없으면 기본 폰트만(라틴) 사용.
pub fn install(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    let loaded = load_cjk_fonts();
    let names: Vec<String> = loaded.iter().map(|(n, _)| n.clone()).collect();
    for (name, bytes) in loaded {
        // `Arc<T>: From<T>` 이므로 `.into()`가 egui 버전과 무관하게 동작.
        fonts.font_data.insert(name, FontData::from_owned(bytes).into());
    }
    if !names.is_empty() {
        // Proportional: CJK 폰트들을 선두에 순서대로(앞이 우선, 뒤가 폴백) → 그다음 egui 기본(라틴).
        let prop = fonts.families.entry(FontFamily::Proportional).or_default();
        for (i, name) in names.iter().enumerate() {
            prop.insert(i, name.clone());
        }
        // Monospace: 라틴은 기본 고정폭, CJK 문자는 뒤의 cjk 폰트들로 폴백.
        let mono = fonts.families.entry(FontFamily::Monospace).or_default();
        for name in &names {
            mono.push(name.clone());
        }
    }
    ctx.set_fonts(fonts);
}
