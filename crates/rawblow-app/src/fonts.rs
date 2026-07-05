//! CJK 폰트 런타임 로드. egui 기본 폰트는 라틴만 포함하므로, OS 폰트
//! 디렉토리에서 한국어/일본어 폰트를 찾아 등록한다(임베드 대신 → 빌드 단순).

use egui::{FontData, FontDefinitions, FontFamily};
use rawblow_core::config::Lang;

/// 플랫폼별 CJK 폰트 후보 경로(우선순위 순). **활성 언어의 폰트를 맨 앞(primary)으로** 둔다.
/// 한 폰트가 그 언어 글리프를 모두 가지면 폴백이 일어나지 않아, 폰트마다 다른 세로 메트릭이
/// 섞여 글자가 위/아래로 어긋나는 문제(#32 후속)가 사라진다. 폴백은 타 언어 stray 글자용.
fn cjk_candidates(lang: Lang) -> Vec<&'static str> {
    let _ = lang;
    #[cfg(target_os = "windows")]
    {
        let malgun = r"C:\Windows\Fonts\malgun.ttf"; // 한국어
        let ja = [
            r"C:\Windows\Fonts\YuGothM.ttc",
            r"C:\Windows\Fonts\meiryo.ttc",
            r"C:\Windows\Fonts\msgothic.ttc",
        ];
        match lang {
            // 일본어 UI: 일본어 폰트를 primary로 → 가나·한자·라틴이 한 폰트에서 일관 렌더.
            Lang::Ja => vec![ja[0], ja[1], ja[2], malgun],
            // 한국어/영어 UI: 한국어 폰트를 primary로(한글·한자·라틴 일관), 일본어는 폴백.
            _ => vec![malgun, ja[0], ja[1], ja[2]],
        }
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

/// 존재하는 CJK 폰트를 **여러 개**(폴백 체인용) (이름, 바이트)로 반환한다(최대 4개). 활성 언어의
/// 폰트가 primary(첫 번째)가 되어 그 언어 글리프를 모두 커버하므로 폴백이 거의 안 일어나
/// 글자 어긋남이 없다. 단일 폰트만 쓰면 한국어 폰트(Windows malgun)에 일본어 신자체(黄/緑/青)가
/// 없어 두부(□)로 깨지므로, 폴백 체인은 그대로 둔다(타 언어 stray 글자 대비).
///
/// 상한이 4인 이유: 일본어 UI 후보가 [YuGoth, meiryo, msgothic, malgun]로 한국어 폰트(malgun)가
/// 맨 뒤다. 상한이 3이면 일본어 폰트 3개에서 잘려 malgun이 안 실리고, 설정 언어 메뉴의
/// "한국어"(native_name)가 두부(□)로 깨졌다. 4로 올려 일본어 UI에서도 한국어 글리프를 커버한다.
fn load_cjk_fonts(lang: Lang) -> Vec<(String, Vec<u8>)> {
    let mut out: Vec<(String, Vec<u8>)> = Vec::new();
    for path in cjk_candidates(lang) {
        if let Ok(bytes) = std::fs::read(path) {
            if !bytes.is_empty() && !out.iter().any(|(_, b)| b.len() == bytes.len()) {
                out.push((format!("cjk{}", out.len()), bytes));
                if out.len() >= 4 {
                    break;
                }
            }
        }
    }
    out
}

/// CJK 폰트를 egui에 설치한다(활성 언어의 폰트를 primary로). 없으면 기본 폰트만(라틴) 사용.
/// 언어 변경 시 다시 호출하면 폰트도 그 언어에 맞게 교체된다(#32 후속: 일본어 세로 어긋남 해결).
pub fn install(ctx: &egui::Context, lang: Lang) {
    let mut fonts = FontDefinitions::default();
    let loaded = load_cjk_fonts(lang);
    let names: Vec<String> = loaded.iter().map(|(n, _)| n.clone()).collect();
    for (name, bytes) in loaded {
        // `Arc<T>: From<T>` 이므로 `.into()`가 egui 버전과 무관하게 동작(0.29는 FontData,
        // 0.30+는 Arc<FontData>를 받음 — 업그레이드 대비 의도적 유지).
        #[allow(clippy::useless_conversion)]
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
