//! UI 다국어(#30): 한국어/영어/일본어. 한국어 원문을 키로 영어·일본어를 찾는 단순 룩업과
//! OS 언어 감지. 번역이 없으면 한국어로 폴백하므로 부분 적용도 안전하다. 서식 문자열(플레이스홀더
//! 포함)은 단어 순서가 언어별로 달라 호출부에서 `match lang`으로 직접 처리한다.

use rawblow_core::config::{Config, Lang};

/// 정적 UI 문자열을 활성 언어로 변환. `ko` 원문이 키이며, 번역이 없으면 원문(ko)을 반환한다.
pub fn tr(lang: Lang, ko: &'static str) -> &'static str {
    match lang {
        Lang::Ko => ko,
        Lang::En => lookup(ko).map(|(en, _)| en).unwrap_or(ko),
        Lang::Ja => lookup(ko).map(|(_, ja)| ja).unwrap_or(ko),
    }
}

/// 설정에 저장된 언어(있으면) 또는 OS 감지값(#30: 기본은 OS 언어, 설정에서 변경·저장).
pub fn effective_lang(cfg: &Config) -> Lang {
    cfg.lang.unwrap_or_else(detect_os_lang)
}

/// 서식 문자열(#30): 번역된 템플릿의 `{...}` 슬롯들을 `args`로 순서대로 치환한다. 언어마다
/// 단어 순서가 달라 `format!`(리터럴 필요)을 못 쓰므로 런타임 치환. `{}`·`{:.1}` 모두 한 슬롯으로
/// 보고, 값은 호출부가 미리 문자열로 만들어 넘긴다(예: `{:.1}` → `format!("{:.1}", x)`).
pub fn trf(lang: Lang, ko: &'static str, args: &[&str]) -> String {
    let tpl = match lang {
        Lang::Ko => ko,
        Lang::En => fmt_lookup(ko).map(|(en, _)| en).unwrap_or(ko),
        Lang::Ja => fmt_lookup(ko).map(|(_, ja)| ja).unwrap_or(ko),
    };
    let mut out = String::with_capacity(tpl.len() + 16);
    let mut rest = tpl;
    let mut ai = 0;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        if let Some(close_rel) = rest[open..].find('}') {
            if let Some(a) = args.get(ai) {
                out.push_str(a);
            }
            ai += 1;
            rest = &rest[open + close_rel + 1..];
        } else {
            break; // 닫는 괄호 없음 — 남은 건 그대로
        }
    }
    out.push_str(rest);
    out
}

/// 서식 템플릿 (en, ja) 룩업. 플레이스홀더 개수·종류는 한국어 원문과 동일하게 유지된다.
fn fmt_lookup(ko: &str) -> Option<(&'static str, &'static str)> {
    Some(match ko {
        "{} 항목 로드" => ("Loaded {} items", "{} 項目を読み込み"),
        "✓ {} 파일 전송 · {} 리네임 · {} 실패" => (
            "✓ {} files transferred · {} renamed · {} failed",
            "✓ {} ファイル転送 · {} リネーム · {} 失敗",
        ),
        "RAW {} · 이미지 {} · {:.1} MB" => ("RAW {} · Images {} · {:.1} MB", "RAW {} · 画像 {} · {:.1} MB"),
        "{} 건 매칭" => ("{} matched", "{} 件一致"),
        "매칭 {}건" => ("{} matched", "一致 {} 件"),
        "{}건 → {}" => ("{} → {}", "{} 件 → {}"),
        "썸네일 캐시 사용량 · {}" => ("Thumbnail cache usage · {}", "サムネイルキャッシュ使用量 · {}"),
        "{} / {} 파일" => ("{} / {} files", "{} / {} ファイル"),
        _ => return None,
    })
}

/// OS UI 언어 감지. 한/일/영만 구분하고 그 외엔 영어로 폴백.
pub fn detect_os_lang() -> Lang {
    #[cfg(target_os = "windows")]
    {
        // kernel32.GetUserDefaultUILanguage(): LANGID(u16). 하위 10비트가 기본 언어 ID.
        #[link(name = "kernel32")]
        extern "system" {
            fn GetUserDefaultUILanguage() -> u16;
        }
        let primary = (unsafe { GetUserDefaultUILanguage() }) & 0x3ff;
        return match primary {
            0x12 => Lang::Ko, // LANG_KOREAN
            0x11 => Lang::Ja, // LANG_JAPANESE
            _ => Lang::En,
        };
    }
    #[cfg(target_os = "macos")]
    {
        return crate::macos_locale::preferred_language()
            .and_then(|c| Lang::from_locale(&c))
            .unwrap_or(Lang::En);
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        for var in ["LC_ALL", "LC_MESSAGES", "LANG", "LANGUAGE"] {
            if let Ok(v) = std::env::var(var) {
                if let Some(l) = Lang::from_locale(&v) {
                    return l;
                }
            }
        }
        return Lang::En;
    }
    #[allow(unreachable_code)]
    Lang::En
}

/// (en, ja) 번역 룩업. 표는 Opus 번역(#30) 기반. 서식 문자열은 호출부 match로 처리하므로 제외.
fn lookup(ko: &str) -> Option<(&'static str, &'static str)> {
    Some(match ko {
        "사진 셀렉 뷰어" => ("Photo Culling Viewer", "写真セレクトビューア"),
        "폴더 열기" => ("Open Folder", "フォルダを開く"),
        "해제" => ("Clear", "解除"),
        "전체" => ("All", "すべて"),
        "표시할 항목이 없습니다" => ("No items to show", "表示する項目がありません"),
        "디코딩 중…" => ("Decoding…", "デコード中…"),
        "파일 전송" => ("Transfer Files", "ファイル転送"),
        "선택한 라벨·별점의 파일을 복사/이동 · RAW 페어 처리" => (
            "Copy/Move files by selected label and rating · handles RAW pairs",
            "選択したラベル・評価のファイルをコピー/移動 · RAWペア処理",
        ),
        "라벨별 하위폴더로 분기 (/pick, /hold …)" => (
            "Split into subfolders by label (/pick, /hold …)",
            "ラベル別サブフォルダに振り分け (/pick, /hold …)",
        ),
        "라벨 또는 별점 중 하나라도 해당하면 전송됩니다(합집합)." => (
            "Files matching either the label or the rating are transferred (union).",
            "ラベルまたは評価のいずれかに該当すれば転送されます（和集合）。",
        ),
        "원본 유지" => ("Keep Originals", "元ファイルを保持"),
        "원본 이동" => ("Move Originals", "元ファイルを移動"),
        "RAW+이미지" => ("RAW + Image", "RAW+画像"),
        "페어 함께" => ("Pairs Together", "ペアごと"),
        "RAW만" => ("RAW Only", "RAWのみ"),
        "이미지만" => ("Image Only", "画像のみ"),
        "JPG만" => ("JPG Only", "JPGのみ"),
        "자동 일련번호" => ("Auto Numbering", "自動連番"),
        "_001 접미" => ("_001 Suffix", "_001 サフィックス"),
        "건너뛰기" => ("Skip", "スキップ"),
        "기존 유지" => ("Keep Existing", "既存を保持"),
        "파일" => ("Files", "ファイル"),
        "이미지" => ("Images", "画像"),
        "전송 시작" => ("Start Transfer", "転送開始"),
        "취소" => ("Cancel", "キャンセル"),
        "전송 완료" => ("Transfer Complete", "転送完了"),
        "닫기" => ("Close", "閉じる"),
        "대상 폴더 열기" => ("Open Destination Folder", "出力先フォルダを開く"),
        "파일번호 점프" => ("Jump to File Number", "ファイル番号へジャンプ"),
        "줄바꿈·쉼표·탭으로 구분" => ("Separate with newlines, commas, or tabs", "改行・カンマ・タブで区切る"),
        "정확히 일치" => ("Exact Match", "完全一致"),
        "점프" => ("Jump", "ジャンプ"),
        "닫기 (Esc)" => ("Close (Esc)", "閉じる (Esc)"),
        "매칭 없음" => ("No Match", "一致なし"),
        "일괄 분류 변경" => ("Batch Relabel", "一括ラベル変更"),
        "파일명·일부 → 매칭 → 라벨 적용" => (
            "Filename or substring → match → apply label",
            "ファイル名・一部 → 一致 → ラベル適用",
        ),
        "파일명 또는 일부 — 줄바꿈·쉼표·탭으로 구분" => (
            "Filename or substring — separate with newlines, commas, or tabs",
            "ファイル名または一部 — 改行・カンマ・タブで区切る",
        ),
        "검색" => ("Search", "検索"),
        "매칭 결과 없음" => ("No Matches", "一致結果なし"),
        "적용할 라벨" => ("Label to Apply", "適用するラベル"),
        "선택" => ("Pick", "選択"),
        "보류" => ("Hold", "保留"),
        "제외" => ("Reject", "除外"),
        "미선택" => ("Unrated", "未選択"),
        "적용" => ("Apply", "適用"),
        "돌아가기" => ("Back", "戻る"),
        "라벨링 후 자동 전진" => ("Auto-advance after labeling", "ラベル付け後に自動で次へ"),
        "하위 폴더 포함 스캔" => ("Scan subfolders", "サブフォルダを含めてスキャン"),
        "EXIF 오버레이 기본 표시" => ("Show EXIF overlay by default", "EXIFオーバーレイをデフォルト表示"),
        "히스토그램 기본 표시" => ("Show histogram by default", "ヒストグラムをデフォルト表示"),
        "프리로드 ±" => ("Preload ±", "プリロード ±"),
        "그리드 열 수" => ("Grid Columns", "グリッド列数"),
        "단축키 재바인딩 UI는 v1.1 예정 — 현재 기본값 QWER 고정 표시" => (
            "Shortcut rebinding UI is planned for v1.1 — currently fixed to default QWER",
            "ショートカット再割り当てUIはv1.1予定 — 現在はデフォルトのQWER固定",
        ),
        "별점 1~5 지정 · ` (백틱)으로 해제 — 라벨(QWER)과 독립으로 동시에 매겨집니다" => (
            "Set rating 1–5 · clear with ` (backtick) — applied independently alongside labels (QWER)",
            "評価1～5を指定 · ` (バッククォート)で解除 — ラベル(QWER)とは独立して同時に付けられます",
        ),
        "캐시 비우기" => ("Clear Cache", "キャッシュをクリア"),
        "썸네일 캐시를 비웠습니다" => ("Thumbnail cache cleared", "サムネイルキャッシュをクリアしました"),
        "새로고침" => ("Refresh", "更新"),
        "자동 상한" => ("Auto Limit", "自動上限"),
        "(0 = 무제한)" => ("(0 = unlimited)", "(0 = 無制限)"),
        "상한을 넘으면 오래된 썸네일부터 자동 삭제 — 폴더 열 때·설정 변경 시 정리됩니다." => (
            "When the limit is exceeded, the oldest thumbnails are removed automatically — cleaned up when opening a folder or changing settings.",
            "上限を超えると古いサムネイルから自動削除 — フォルダを開いたときや設定変更時に整理されます。",
        ),
        "폴더를 다시 열어도 재디코딩 없이 즉시 표시됩니다." => (
            "Reopening a folder shows instantly without re-decoding.",
            "フォルダを再度開いても再デコードなしで即座に表示されます。",
        ),
        "최신 버전 받기 · GitHub Releases" => ("Get the latest version · GitHub Releases", "最新バージョンを入手 · GitHub Releases"),
        "버그 제보 · GitHub Issues" => ("Report a bug · GitHub Issues", "バグ報告 · GitHub Issues"),
        "만든 사람 · 하레 (Hare)" => ("Created by · Hare", "制作者 · ハレ (Hare)"),
        "마음에 드시나요? 그럼 cosly도 이용해보세요." => (
            "Like it? Then give cosly a try too.",
            "気に入りましたか？ぜひcoslyもお試しください。",
        ),
        "언어" => ("Language", "言語"),
        "시스템 (자동)" => ("System (Auto)", "システム (自動)"),
        "전송" => ("Transfer", "転送"),
        "일괄" => ("Bulk", "一括"),
        // #27 컬러 태그 / #26 리네임 UI
        "색별 이름을 지정해 보정 방식 등 나만의 분류로 — ⇧1~5로 부여" => (
            "Name each color for your own classification (e.g. retouch style) — assign with ⇧1–5",
            "色ごとに名前を付けて自分の分類に（補正方式など）— ⇧1～5で付与",
        ),
        "태그별 하위폴더로 분기 (@teal …)" => (
            "Split into subfolders by tag (@teal …)",
            "タグ別サブフォルダに振り分け (@teal …)",
        ),
        "순번 (1,2,3)" => ("Sequence (1,2,3)", "連番 (1,2,3)"),
        "별점등급 (A1,B1…)" => ("Rating grade (A1,B1…)", "評価ランク (A1,B1…)"),
        "직접 입력" => ("Custom", "カスタム"),
        "선택순" => ("Order", "選択順"),
        "선택 순서" => ("Selection order", "選択順"),
        "등급순" => ("Grade", "ランク順"),
        "별점 등급" => ("By rating grade", "評価ランク順"),
        "대상 없음 — 위에서 라벨·별점·태그를 선택하세요." => (
            "No targets — select labels, ratings, or tags above.",
            "対象なし — 上でラベル・評価・タグを選択してください。",
        ),
        // #34 폴더 자동 분류 / #35 진행률
        "정리" => ("Organize", "整理"),
        "폴더 자동 분류" => ("Auto-Organize Folder", "フォルダ自動整理"),
        "폴더 안 사진을 기준별 하위폴더로 정리 · 셀렉 전송과 별개" => (
            "Sort photos into subfolders by a criterion · separate from Transfer",
            "フォルダ内の写真を基準別サブフォルダに整理 · 転送とは別機能",
        ),
        "촬영일" => ("Capture Date", "撮影日"),
        "카메라" => ("Camera", "カメラ"),
        "렌즈" => ("Lens", "レンズ"),
        "확장자" => ("Extension", "拡張子"),
        "대상 폴더 안에 기준별 하위폴더가 생성됩니다." => (
            "Subfolders are created inside the destination folder.",
            "出力先フォルダ内に基準別サブフォルダが作成されます。",
        ),
        "촬영일·카메라·렌즈 기준은 실행하며 EXIF를 읽어 분류합니다. RAW+JPG 페어는 같은 폴더로 유지됩니다." => (
            "Date/camera/lens read EXIF while running. RAW+JPG pairs stay in the same folder.",
            "撮影日・カメラ・レンズは実行中にEXIFを読んで分類します。RAW+JPGペアは同じフォルダに保たれます。",
        ),
        "정리 시작" => ("Start Organize", "整理開始"),
        "전송 중" => ("Transferring", "転送中"),
        "폴더 정리 중" => ("Organizing", "整理中"),
        "취소됨" => ("Canceled", "キャンセルされました"),
        // #36 사진 배경색
        "사진 표시 화면 배경색 — 프리셋 또는 HEX/RGB로 지정(Lightroom Develop 기본값은 50% 회색)" => (
            "Photo viewer background — pick a preset or set HEX/RGB (Lightroom Develop default is 50% gray)",
            "写真表示画面の背景色 — プリセットまたはHEX/RGBで指定（Lightroom Developの既定は50%グレー）",
        ),
        "기본" => ("Default", "デフォルト"),
        "검정" => ("Black", "黒"),
        "다크 그레이" => ("Dark Gray", "ダークグレー"),
        "중간 회색" => ("Medium Gray", "ミディアムグレー"),
        "라이트 그레이" => ("Light Gray", "ライトグレー"),
        "흰색" => ("White", "白"),
        // #33 새 릴리즈 안내
        "새로운 버전이 있습니다" => ("A new version is available", "新しいバージョンがあります"),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::{tr, trf};
    use rawblow_core::config::Lang;

    #[test]
    fn tr_picks_language_and_falls_back_to_ko() {
        assert_eq!(tr(Lang::Ko, "폴더 열기"), "폴더 열기");
        assert_eq!(tr(Lang::En, "폴더 열기"), "Open Folder");
        assert_eq!(tr(Lang::Ja, "폴더 열기"), "フォルダを開く");
        // 표에 없는 키는 한국어 원문으로 폴백(부분 적용 안전).
        assert_eq!(tr(Lang::En, "표에 없는 문자열"), "표에 없는 문자열");
    }

    #[test]
    fn trf_substitutes_in_order_with_reordering() {
        // 영어는 단어 순서가 달라도 슬롯이 순서대로 치환되어야 한다.
        assert_eq!(trf(Lang::Ko, "{} 항목 로드", &["12"]), "12 항목 로드");
        assert_eq!(trf(Lang::En, "{} 항목 로드", &["12"]), "Loaded 12 items");
        assert_eq!(trf(Lang::Ja, "{} 항목 로드", &["12"]), "12 項目を読み込み");
        // 다중 슬롯 + {:.1} 슬롯(미리 포맷한 문자열을 넘김).
        assert_eq!(
            trf(Lang::En, "RAW {} · 이미지 {} · {:.1} MB", &["3", "5", "12.3"]),
            "RAW 3 · Images 5 · 12.3 MB"
        );
    }

    #[test]
    fn from_locale_maps_prefixes() {
        assert_eq!(Lang::from_locale("ko-KR"), Some(Lang::Ko));
        assert_eq!(Lang::from_locale("ja_JP"), Some(Lang::Ja));
        assert_eq!(Lang::from_locale("en-US"), Some(Lang::En));
        assert_eq!(Lang::from_locale("fr-FR"), None);
    }
}
