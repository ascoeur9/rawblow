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
        // #63 원본 이동 확인 · 전송 결과 정직성
        "원본 {}개를 이동합니다 — 원래 폴더에서 제거됩니다." => (
            "Moving {} originals — they will be removed from the source folder.",
            "元ファイル {} 個を移動します — 元のフォルダからは削除されます。",
        ),
        "건너뜀 {} — 동명 파일 존재" => ("Skipped {} — same-name file exists", "スキップ {} — 同名ファイルあり"),
        "원본 삭제 실패 {} — 원본 파일이 남아 있습니다" => (
            "Failed to delete {} originals — the source files remain",
            "元ファイル {} 個の削除に失敗 — 元ファイルが残っています",
        ),
        "{} 건 매칭" => ("{} matched", "{} 件一致"),
        "매칭 {}건" => ("{} matched", "一致 {} 件"),
        "{}건 → {}" => ("{} → {}", "{} 件 → {}"),
        "썸네일 캐시 사용량 · {}" => ("Thumbnail cache usage · {}", "サムネイルキャッシュ使用量 · {}"),
        "{} / {} 파일" => ("{} / {} files", "{} / {} ファイル"),
        "{} 구성요소" => ("{} components", "{} コンポーネント"),
        // #50~#53 AI 컬링 / #52 점프
        "{} 항목" => ("{} items", "{} 項目"),
        "{} / {} 장" => ("{} / {} photos", "{} / {} 枚"),
        "AI 컬링 완료 · 좋음 {} · 탈락 {}" => ("AI culling done · Good {} · Culled {}", "AIカリング完了 · 良い {} · 落選 {}"),
        " · 캐시 {}장" => (" · {} cached", " · キャッシュ {}枚"),
        " · 제외 {}장" => (" · {} skipped", " · 除外 {}枚"),
        "1 ~ {} 사이 번호" => ("Number from 1 to {}", "1～{} の番号"),
        "1 ~ {} 사이 번호를 입력하세요" => ("Enter a number from 1 to {}", "1～{} の番号を入力してください"),
        // #65 끝 도달 피드백
        "마지막 사진 · 미분류 {}장" => ("Last photo · {} unrated", "最後の写真 · 未分類 {}枚"),
        "{} 건 매칭 — 첫 항목으로" => ("{} matched — jumped to first", "{} 件一致 — 最初の項目へ"),
        // #50 모델 다운로드 오류(토스트에 그대로 노출되는 문자열)
        "다운로드 실패: {}" => ("Download failed: {}", "ダウンロード失敗: {}"),
        // #71 영어 하드코딩이던 HTTP/mkdir 에러도 번역.
        "HTTP 오류: {}" => ("HTTP error: {}", "HTTPエラー: {}"),
        "폴더 생성 실패: {}" => ("Failed to create folder: {}", "フォルダ作成失敗: {}"),
        "파일 생성: {}" => ("create file: {}", "ファイル作成: {}"),
        "쓰기 오류: {}" => ("write error: {}", "書き込みエラー: {}"),
        "읽기 오류: {}" => ("read error: {}", "読み取りエラー: {}"),
        "검증 열기: {}" => ("open for verify: {}", "検証用オープン: {}"),
        "해시 읽기: {}" => ("hash read: {}", "ハッシュ読み取り: {}"),
        "파일 이동: {}" => ("move file: {}", "ファイル移動: {}"),
        "sha256 불일치 — 다시 시도해주세요\n예상: {}\n실제: {}" => (
            "sha256 mismatch — please retry\nexpected: {}\nactual: {}",
            "sha256不一致 — もう一度お試しください\n期待値: {}\n実際: {}",
        ),
        // #62 사이드카 저장 실패 표면화(토스트)
        "셀렉 저장 실패: {} — 폴더 쓰기 권한·용량을 확인하세요" => (
            "Failed to save selections: {} — check folder write permission/space",
            "セレクトの保存に失敗: {} — フォルダの書き込み権限・空き容量を確認してください",
        ),
        "이전 폴더({}) 셀렉 저장 실패 — 최근 변경이 유실될 수 있습니다" => (
            "Failed to save selections for previous folder ({}) — recent changes may be lost",
            "前のフォルダ（{}）のセレクト保存に失敗 — 直近の変更が失われた可能性があります",
        ),
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
        // #67 빈 화면 사유 구분 + 필터 초기화 CTA
        "이 폴더에 표시할 사진이 없습니다" => ("No photos to show in this folder", "このフォルダに表示できる写真がありません"),
        "필터와 일치하는 사진이 없습니다" => ("No photos match the filters", "フィルタに一致する写真がありません"),
        "필터 초기화" => ("Reset Filters", "フィルタをリセット"),
        "디코딩 중…" => ("Decoding…", "デコード中…"),
        "이 파일을 열 수 없습니다" => ("Can't open this file", "このファイルを開けません"), // #64
        "파일 전송" => ("Transfer Files", "ファイル転送"),
        "선택한 라벨·별점의 파일을 복사/이동 · RAW 페어 처리" => (
            "Copy/Move files by selected label and rating · handles RAW pairs",
            "選択したラベル・評価のファイルをコピー/移動 · RAWペア処理",
        ),
        "라벨별 하위폴더로 분기 (/pick, /hold …)" => (
            "Split into subfolders by label (/pick, /hold …)",
            "ラベル別サブフォルダに振り分け (/pick, /hold …)",
        ),
        "폴더 나누기" => ("Folder Split", "フォルダ分け"),
        "한 폴더로" => ("One folder", "1つのフォルダへ"),
        "라벨별 (QWER)" => ("By label (QWER)", "ラベル別 (QWER)"),
        "별점별 (12345)" => ("By rating (12345)", "評価別 (12345)"),
        "색 태그별" => ("By color tag", "色タグ別"),
        "선택한 사진을 대상 폴더 한곳에 둡니다. 지금 열린 폴더는 고를 수 없습니다." => (
            "Puts selected photos in one destination folder. The currently open folder cannot be chosen.",
            "選んだ写真を出力先フォルダ1つにまとめます。今開いているフォルダは選べません。",
        ),
        "pick / hold / reject 폴더를 만듭니다. 제외도 여기로 빠집니다." => (
            "Creates pick / hold / reject folders. Rejects go there too.",
            "pick / hold / reject フォルダを作ります。除外もここに入ります。",
        ),
        "1star … 5star 폴더를 만듭니다. 무별점은 unrated." => (
            "Creates 1star … 5star folders. Unrated photos go to unrated.",
            "1star … 5star フォルダを作ります。星なしは unrated。",
        ),
        "@teal 같은 색 태그 폴더를 만듭니다. 무태그는 @untagged." => (
            "Creates color-tag folders like @teal. Untagged photos go to @untagged.",
            "@teal などの色タグフォルダを作ります。タグなしは @untagged。",
        ),
        "지금 열린 폴더로는 보낼 수 없습니다. 다른 폴더를 고르세요." => (
            "Can't send to the currently open folder. Choose another folder.",
            "今開いているフォルダには送れません。別のフォルダを選んでください。",
        ),
        "전송 폴더 기본값" => ("Default transfer folder", "転送フォルダのデフォルト"),
        "지금 보고 있는 폴더" => ("Current folder", "今見ているフォルダ"),
        "지정된 폴더" => ("Chosen folder", "指定フォルダ"),
        "전송(Ctrl/⌘E) 화면에서 폴더를 바꿀 수 있습니다. 다음 전송은 다시 이 기본값입니다. 한 폴더로 보낼 때 지금 열린 폴더는 고를 수 없고, 그때 기본값은 selected 하위폴더입니다." => (
            "You can change the folder in Transfer (Ctrl/⌘E). The next transfer uses this default again. In one-folder mode the currently open folder can't be chosen; the default is the selected subfolder.",
            "転送(Ctrl/⌘E)画面でフォルダを変えられます。次の転送はまたこのデフォルトです。1つのフォルダへ送るとき、今開いているフォルダは選べず、そのときのデフォルトは selected サブフォルダです。",
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
        "이동 시작" => ("Start Move", "移動開始"), // #63 원본 이동 확인 오버레이
        "취소" => ("Cancel", "キャンセル"),
        "전송 완료" => ("Transfer Complete", "転送完了"),
        "정리 완료" => ("Organize Complete", "整理完了"), // #63 정리 결과창 제목 분리
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
        "스트립·그리드 표기 크기" => ("Strip/grid badge size", "ストリップ・グリッド表示サイズ"),
        "크게" => ("Large", "大きく"),
        "작게" => ("Small", "小さく"),
        // 정렬 기준(#56)
        "정렬 기준" => ("Sort by", "並び順"),
        "파일명순" => ("File name", "ファイル名順"),
        "촬영시간순" => ("Capture time", "撮影日時順"),
        // 사진 이동 시 원본 보기(ORIG) 유지 방식(#87)
        "사진 이동 시 원본 보기" => ("Original view when moving", "写真移動時の原寸表示"),
        "확대 중일 때만 유지" => ("Keep only while zoomed", "拡大中のみ維持"),
        "현재 보기 상태 유지" => ("Keep current view", "現在の表示を維持"),
        "「현재 보기 상태 유지」는 창맞춤에서도 ORIG를 계속 불러옵니다 — 넘김이 느려지고 메모리를 더 씁니다. 새 폴더는 두 방식 모두 프리뷰로 시작합니다." => (
            "\"Keep current view\" loads ORIG even at fit-to-window — slower moves and more memory. A newly opened folder starts in preview either way.",
            "「現在の表示を維持」はウィンドウ合わせでもORIGを読み込み続けます — 送りが遅くなりメモリも増えます。新しいフォルダはどちらの方式でもプレビューで始まります。",
        ),
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
        "투네이션으로 후원하기" => ("Donate via Toonation", "Toonationで支援する"),
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
        // "초점거리"(#92 분류 기준 칩)는 아래 컬링 필터 블록에 이미 있는 키를 그대로 쓴다
        // — 같은 match의 중복 arm은 unreachable 경고가 난다.
        "확장자" => ("Extension", "拡張子"),
        "대상 폴더 안에 기준별 하위폴더가 생성됩니다." => (
            "Subfolders are created inside the destination folder.",
            "出力先フォルダ内に基準別サブフォルダが作成されます。",
        ),
        "촬영일·카메라·렌즈·초점거리 기준은 실행하며 EXIF를 읽어 분류합니다. RAW+JPG 페어는 같은 폴더로 유지됩니다." => (
            "Date/camera/lens/focal length read EXIF while running. RAW+JPG pairs stay in the same folder.",
            "撮影日・カメラ・レンズ・焦点距離は実行中にEXIFを読んで分類します。RAW+JPGペアは同じフォルダに保たれます。",
        ),
        "초점거리는 1mm 단위(35mm)로 폴더를 만들고, EXIF가 없으면 unknown-focal로 모읍니다." => (
            "Focal length creates 1 mm folders (35mm); files without EXIF go to unknown-focal.",
            "焦点距離は1mm単位(35mm)でフォルダを作成し、EXIFがないファイルはunknown-focalにまとめます。",
        ),
        "정리 시작" => ("Start Organize", "整理開始"),
        "전송 중" => ("Transferring", "転送中"),
        "폴더 정리 중" => ("Organizing", "整理中"),
        "취소됨" => ("Canceled", "キャンセルされました"),
        // #63 워커가 Done 없이 종료(패닉 등)했을 때 결과창에 표시하는 실패 사유.
        "작업이 예기치 않게 중단되었습니다" => (
            "The operation stopped unexpectedly",
            "処理が予期せず中断されました",
        ),
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
        // #37 AF 포인트 / #38 GPS 지도
        "지도 로딩…" => ("Loading map…", "地図を読み込み中…"),
        "지도를 불러올 수 없음 (오프라인?)" => ("Couldn't load map (offline?)", "地図を読み込めません（オフライン？）"),
        "M = 촬영 위치 미니 지도(GPS 있는 사진) · A = AF 포인트 표시 — 토글 상태는 저장됩니다" => (
            "M = location mini-map (geotagged photos) · A = AF point overlay — toggle state is saved",
            "M = 撮影位置ミニマップ（GPS付き写真） · A = AFポイント表示 — トグル状態は保存されます",
        ),
        // #39 오픈소스 라이센스 고지
        "오픈소스 라이센스" => ("Open Source Licenses", "オープンソースライセンス"),
        "이 프로그램이 포함한 오픈소스 구성요소 목록과 라이센스 전문" => (
            "List of bundled open source components and their full license texts",
            "本アプリに含まれるオープンソースコンポーネントの一覧とライセンス全文",
        ),
        "이 프로그램은 아래의 오픈소스 소프트웨어를 포함합니다. LGPL 구성요소(rawloader · imagepipe · multicache)의 소스코드는 각 항목의 저장소 링크에서 구할 수 있습니다." => (
            "This application includes the open source software listed below. Source code for the LGPL components (rawloader · imagepipe · multicache) is available at each component's repository link.",
            "本アプリは以下のオープンソースソフトウェアを含みます。LGPLコンポーネント（rawloader · imagepipe · multicache）のソースコードは各項目のリポジトリリンクから入手できます。",
        ),
        // #50~#53 AI 컬링 — 좌측 레일·다이얼로그·확인 모달·토스트
        "AI 컬링" => ("AI Culling", "AIカリング"),
        "분석" => ("Analyzing", "分析"),
        "AI 컬링 중 — 선택(라벨)이 잠겨 있습니다" => ("AI culling in progress — labels are locked", "AIカリング中 — 選択（ラベル）はロック中です"),
        "AI 컬링 중 — 별점이 잠겨 있습니다" => ("AI culling in progress — ratings are locked", "AIカリング中 — 評価はロック中です"),
        "AI 컬링 중 — 색 태그가 잠겨 있습니다" => ("AI culling in progress — color tags are locked", "AIカリング中 — カラータグはロック中です"),
        "AI 컬링이 끝난 뒤 전송할 수 있습니다" => ("Transfer is available after AI culling finishes", "AIカリングが終わってから転送できます"),
        "AI 컬링이 끝난 뒤 정리할 수 있습니다" => ("Organize is available after AI culling finishes", "AIカリングが終わってから整理できます"),
        "사진을 분석해 흐림·노출·기울기로 자동 분류 · 전부 로컬 처리" => (
            "Analyzes photos and auto-classifies by blur, exposure, and tilt · all processed locally",
            "写真を分析してブレ・露出・傾きで自動分類 · すべてローカル処理",
        ),
        "초점(선명도)" => ("Focus (Sharpness)", "フォーカス（シャープネス）"),
        "노출" => ("Exposure", "露出"),
        "수평 기울기" => ("Horizon Tilt", "水平の傾き"),
        "미적(구도) AI" => ("Aesthetic (Composition) AI", "美的（構図）AI"),
        "흐림 임계" => ("Blur Threshold", "ブレしきい値"),
        "AF 측거점만" => ("AF Point Area Only", "AF測距点のみ"),
        "노출 하한" => ("Exposure Min", "露出下限"),
        "허용 기울기" => ("Max Tilt", "許容傾き"),
        "⚡ GPU 고속 모드" => ("⚡ GPU Fast Mode", "⚡ GPU高速モード"),
        "백본" => ("Backbone", "バックボーン"),
        "선택 기준" => ("Selection Criterion", "選択基準"),
        "상위 N장" => ("Top N", "上位N枚"),
        "최고 점수" => ("highest scores", "スコア上位"),
        "임계값" => ("Threshold", "しきい値"),
        "P(good) 하한" => ("min P(good)", "P(good)下限"),
        "최고 N장" => ("Top N", "上位N枚"),
        "장" => (" photos", "枚"),
        "✓ 미적 모델 준비됨" => ("✓ Aesthetic model ready", "✓ 美的モデル準備完了"),
        "미적 모델 없음 — 아래 버튼으로 받으세요" => ("Aesthetic model missing — download with the button below", "美的モデルなし — 下のボタンでダウンロード"),
        "미적 모델 다운로드" => ("Download Aesthetic Model", "美的モデルをダウンロード"),
        "(전체)" => ("(All)", "（すべて）"),
        "방향" => ("Orientation", "向き"),
        "세로" => ("Portrait", "縦"),
        "가로" => ("Landscape", "横"),
        "ISO 상한" => ("ISO Max", "ISO上限"),
        "초점거리" => ("Focal Length", "焦点距離"),
        "조리개 ≤" => ("Aperture ≤", "絞り ≤"),
        "셔터 하한" => ("Shutter Min", "シャッター下限"),
        "초 (손떨림)" => ("s (shake risk)", "秒（手ブレ）"),
        "메타 수집 중… (잠시 후 목록이 채워집니다)" => (
            "Collecting metadata… (lists fill in shortly)",
            "メタデータ収集中…（まもなくリストに反映されます）",
        ),
        "연사 베스트-N" => ("Burst Best-N", "連写ベストN"),
        "간격 ≤" => ("Gap ≤", "間隔 ≤"),
        "초" => ("s", "秒"),
        "그룹당" => ("Per group", "グループごと"),
        "시각적 중복 묶기" => ("Group Visual Duplicates", "見た目の重複をまとめる"),
        "해밍 ≤" => ("Hamming ≤", "ハミング ≤"),
        "클러스터당" => ("Per cluster", "クラスタごと"),
        "그룹 내 베스트는 점수(미적>선명도) 기준 선택" => (
            "Best in each group is picked by score (aesthetic > sharpness)",
            "グループ内のベストはスコア（美的＞シャープネス）で選択",
        ),
        "장르 픽" => ("Genre Pick", "ジャンルピック"),
        "인물" => ("Portrait", "人物"),
        "얼굴 있음" => ("has faces", "顔あり"),
        "풍경" => ("Landscape", "風景"),
        "얼굴 없음" => ("no faces", "顔なし"),
        "AI 선명도" => ("AI Sharpness", "AIシャープネス"),
        // "P(good) ≥"·"P(sharp) ≥"·"Pick / Reject"·"person"·"→ {}"는 전 언어 동일(ASCII)이라
        // 폴백으로 충분 — 표에 넣지 않는다.
        "장르 픽: 얼굴 검출(YuNet) 기반" => ("Genre pick: based on face detection (YuNet)", "ジャンルピック：顔検出（YuNet）ベース"),
        "CLIP 다축 모델(89MB) 없음 — 아래 버튼으로 받으세요" => (
            "CLIP multi-axis model (89MB) missing — download with the button below",
            "CLIPマルチ軸モデル（89MB）なし — 下のボタンでダウンロード",
        ),
        "AI 선명도 모델 다운로드" => ("Download AI Sharpness Model", "AIシャープネスモデルをダウンロード"),
        "✓ CLIP 다축 모델 준비됨" => ("✓ CLIP multi-axis model ready", "✓ CLIPマルチ軸モデル準備完了"),
        "얼굴 있는 컷만" => ("Only Shots with Faces", "顔がある写真のみ"),
        "객체 포함" => ("Contains Object", "オブジェクト含む"),
        "COCO 클래스명" => ("COCO class name", "COCOクラス名"),
        "객체 모델(YOLOv10n, 9MB) 없음 — 아래 버튼으로 받으세요" => (
            "Object model (YOLOv10n, 9MB) missing — download with the button below",
            "オブジェクトモデル（YOLOv10n、9MB）なし — 下のボタンでダウンロード",
        ),
        "객체 모델 다운로드" => ("Download Object Model", "オブジェクトモデルをダウンロード"),
        "✓ 객체 모델 준비됨 (YOLOv10n)" => ("✓ Object model ready (YOLOv10n)", "✓ オブジェクトモデル準備完了（YOLOv10n）"),
        "얼굴 모델(YuNet, 0.2MB) 없음 — 아래 버튼으로 받으세요" => (
            "Face model (YuNet, 0.2MB) missing — download with the button below",
            "顔モデル（YuNet、0.2MB）なし — 下のボタンでダウンロード",
        ),
        "얼굴 모델 다운로드" => ("Download Face Model", "顔モデルをダウンロード"),
        "✓ 얼굴 모델 준비됨 (YuNet)" => ("✓ Face model ready (YuNet)", "✓ 顔モデル準備完了（YuNet）"),
        "검사 항목을 하나 이상 켜세요." => ("Turn on at least one check.", "検査項目を1つ以上オンにしてください。"),
        "별점" => ("Rating", "評価"),
        "높음 / 낮음" => ("high / low", "高い / 低い"),
        "색 태그" => ("Color Tag", "カラータグ"),
        "두 색" => ("two colors", "2色"),
        "좋음 → 선택(Pick) · 탈락 → 제외(Reject)" => ("Good → Pick · Culled → Reject", "良い → Pick · 落選 → Reject"),
        "좋음" => ("Good", "良い"),
        "탈락" => ("Culled", "落選"),
        "현재 필터" => ("Current Filter", "現在のフィルタ"),
        "컬링 시작" => ("Start Culling", "カリング開始"),
        "~100장/초 내외 (GPU)" => ("~100 photos/s (GPU)", "~100枚/秒前後（GPU）"),
        "~90장/초 내외 (CPU)" => ("~90 photos/s (CPU)", "~90枚/秒前後（CPU）"),
        "~200장/초+ (모델 불필요·가장 빠름)" => ("~200+ photos/s (no model needed · fastest)", "~200枚/秒+（モデル不要・最速）"),
        "YuNet (얼굴)" => ("YuNet (face)", "YuNet（顔）"),
        "CLIP 다축 (AI 선명도)" => ("CLIP multi-axis (AI sharpness)", "CLIPマルチ軸（AIシャープネス）"),
        "YOLOv10n (객체)" => ("YOLOv10n (object)", "YOLOv10n（オブジェクト）"),
        "RN50 (GPU 고속)" => ("RN50 (GPU fast)", "RN50（GPU高速）"),
        "모델 다운로드" => ("Model Download", "モデルダウンロード"),
        "모델 다운로드 완료" => ("Model download complete", "モデルのダウンロード完了"),
        "모델 다운로드를 취소했습니다" => ("Model download canceled", "モデルのダウンロードをキャンセルしました"), // #71
        "연결 끊김" => ("connection lost", "接続が切断されました"),
        "폴더가 바뀌어 AI 컬링 결과를 버렸습니다" => ("Folder changed — AI culling results were discarded", "フォルダが変わったためAIカリング結果を破棄しました"),
        "AI 컬링 진행 중" => ("AI Culling in Progress", "AIカリング進行中"),
        "분석을 멈추시겠어요?" => ("Stop the analysis?", "分析を中止しますか？"),
        "컬링 취소" => ("Cancel Culling", "カリングをキャンセル"),
        "계속 진행" => ("Keep Running", "続行"),
        "AI 컬링을 취소했습니다" => ("AI culling canceled", "AIカリングをキャンセルしました"),
        "컬링이 진행 중입니다" => ("Culling is in progress", "カリングが進行中です"),
        "폴더를 바꾸면 진행 중인 AI 컬링이 취소됩니다." => (
            "Changing folders will cancel the AI culling in progress.",
            "フォルダを変更すると進行中のAIカリングはキャンセルされます。",
        ),
        "폴더 바꾸기" => ("Change Folder", "フォルダを変更"),
        "계속 컬링" => ("Keep Culling", "カリングを続行"),
        // #70 AI 컬링 다이얼로그 기본/고급 접기 + 사진가 용어 리라벨.
        // (옛 키 "사진을 분석해 흐림···"·"P(good) 하한"·"해밍 ≤"·"COCO 클래스명"은 무해하게 표에 남긴다.)
        "사진을 분석해 자동 분류 — 초점·노출·수평부터 미적 AI·연사 베스트·중복 정리까지 · 전부 로컬 처리" => (
            "Analyzes photos and auto-classifies — from focus/exposure/tilt to aesthetic AI, burst best, and dedup · all processed locally",
            "写真を分析して自動分類 — フォーカス・露出・水平から美的AI・連写ベスト・重複整理まで · すべてローカル処理",
        ),
        "고급 옵션" => ("Advanced Options", "詳細オプション"),
        "메타 필터·연사·중복·AI 축·검출" => ("meta filter · burst · dedup · AI axes · detect", "メタフィルタ・連写・重複・AI軸・検出"),
        "고급 옵션이 적용 중입니다" => ("Advanced options are active", "詳細オプションが適用中です"),
        "미적 점수 하한" => ("Aesthetic score min", "美的スコア下限"),
        "차이 허용 ≤" => ("Diff tolerance ≤", "差の許容 ≤"),
        "객체 이름(영문) — 예: person, dog" => ("Object name (English) — e.g. person, dog", "オブジェクト名（英語） — 例: person, dog"),
        "실제 속도는 디코딩·디스크 속도에 따라 다릅니다" => ("Actual speed depends on decode and disk speed", "実際の速度はデコード・ディスク速度に依存します"),
        "이 빌드에는 모델 다운로드가 포함되지 않았습니다" => ("This build does not include model downloads", "このビルドにはモデルダウンロードが含まれていません"),
        // #70 주요 옵션 툴팁(사진가 친화 한 줄 설명)
        "흐린 사진(핀이 나간 컷)을 걸러냅니다" => ("Filters out blurry (missed-focus) shots", "ブレた（ピントを外した）写真を除外します"),
        "심하게 어둡거나 하얗게 날아간 사진을 걸러냅니다" => (
            "Filters out badly underexposed or blown-out shots",
            "極端に暗い・白飛びした写真を除外します",
        ),
        "수평이 기울어진 사진을 걸러냅니다" => ("Filters out shots with a tilted horizon", "水平が傾いた写真を除外します"),
        "AI가 구도·분위기를 채점해 잘 나온 컷을 고릅니다 (모델 필요)" => (
            "AI scores composition and look to pick the best shots (model required)",
            "AIが構図・雰囲気を採点して良いカットを選びます（モデルが必要）",
        ),
        "높일수록 더 엄격하게 흐림으로 판정합니다" => ("Higher = stricter blur judgment", "高くするほど厳しくブレと判定します"),
        "초점을 사진 전체가 아니라 카메라가 맞춘 AF 지점에서만 봅니다" => (
            "Checks focus only at the camera's AF points, not the whole frame",
            "フォーカスを写真全体ではなくカメラが合わせたAF点だけで判定します",
        ),
        "그래픽카드로 미적 채점을 가속합니다" => ("Accelerates aesthetic scoring on the GPU", "GPUで美的採点を高速化します"),
        "미적 채점에 쓸 AI 모델 — 클수록 정확하지만 느립니다" => (
            "AI model for aesthetic scoring — larger is more accurate but slower",
            "美的採点に使うAIモデル — 大きいほど正確ですが遅くなります",
        ),
        "AI 미적 점수(0~1) 하한 — 이보다 낮으면 탈락합니다" => (
            "Min AI aesthetic score (0–1) — anything lower is culled",
            "AI美的スコア（0～1）の下限 — これ未満は落選します",
        ),
        "짧은 간격으로 찍힌 연사를 묶어 그룹당 베스트 N장만 남깁니다" => (
            "Groups rapid bursts and keeps only the best N per group",
            "短い間隔の連写をまとめてグループごとにベストN枚だけ残します",
        ),
        "거의 같은 장면을 묶어 베스트만 남깁니다" => (
            "Groups near-identical shots and keeps only the best",
            "ほぼ同じ場面をまとめてベストだけ残します",
        ),
        // #52 점프 다이얼로그
        "한 곳으로 이동 — 순번 또는 파일명 일부" => ("Go to one spot — by position or filename part", "1か所へ移動 — 番号またはファイル名の一部"),
        "순번" => ("Number", "番号"),
        "현재/전체의 번호" => ("position in current/total", "現在/全体の番号"),
        "파일명" => ("Filename", "ファイル名"),
        "일부 일치" => ("partial match", "部分一致"),
        "파일명 일부(한 개)" => ("part of a filename (single)", "ファイル名の一部（1件）"),
        "파일명 일부를 입력하세요" => ("Enter part of a filename", "ファイル名の一部を入力してください"),
        // #62 사이드카 저장 실패(상태바 3번째 상태 — saving…/saved는 영문 고정이라 표 제외)
        "저장 실패" => ("Save failed", "保存失敗"),
        // #66 단축키 치트시트 오버레이 — 제목·부제·섹션·행 설명·키캡 낱말.
        // ("파일"·"점프"·"보류"·"제외"·"해제"·"별점"·"폴더 열기"·"전송"·"닫기"는 위에 이미 있어 재사용.)
        "단축키" => ("Keyboard Shortcuts", "ショートカット"),
        "사진 위에서 바로 누르면 됩니다 — 입력창이 없어 즉시 반응" => (
            "Press keys right over the photo — no text field, instant response",
            "写真の上でそのまま押すだけ — 入力欄がなく即反応",
        ),
        "이동" => ("Move", "移動"),
        "분류" => ("Classify", "分類"),
        "별점·태그" => ("Rating & Tag", "評価・タグ"),
        "보기" => ("View", "表示"),
        "확대" => ("Zoom", "ズーム"),
        "이전·다음" => ("Prev / Next", "前 / 次"),
        "그리드 줄 이동" => ("Grid row move", "グリッド行移動"),
        "사진 넘김(창맞춤일 때)" => ("Advance (when Fit)", "写真送り（フィット時）"),
        "일괄 분류(그리드)" => ("Bulk label (grid)", "一括ラベル（グリッド）"),
        "채택" => ("Pick", "採用"),
        "같은 키 재입력 = 해제(토글)" => ("Same key again = clear (toggle)", "同じキーで解除（トグル）"),
        "별점 해제" => ("Clear rating", "評価解除"),
        "컬러 태그" => ("Color tag", "カラータグ"),
        "태그 해제" => ("Clear tag", "タグ解除"),
        "단일↔그리드" => ("Single / Grid", "シングル / グリッド"),
        "원본(ORIG)" => ("Original (ORIG)", "原寸（ORIG）"),
        "히스토그램" => ("Histogram", "ヒストグラム"),
        "촬영 위치 지도" => ("Location map", "撮影地マップ"),
        "AF 포인트" => ("AF points", "AFポイント"),
        "라벨 필터 순환" => ("Cycle label filter", "ラベルフィルタ切替"),
        "전체화면" => ("Fullscreen", "全画面"),
        "창맞춤↔1:1" => ("Fit / 1:1", "フィット / 1:1"),
        "연속 확대" => ("Zoom in/out", "連続ズーム"),
        "이동(확대 중)" => ("Pan (while zoomed)", "移動（ズーム中）"),
        "드래그앤드롭으로 폴더 열기" => ("Drag & drop to open", "ドラッグ＆ドロップで開く"),
        "휠" => ("Wheel", "ホイール"),
        "클릭" => ("Click", "クリック"),
        "핀치" => ("Pinch", "ピンチ"),
        "드래그" => ("Drag", "ドラッグ"),
        "열기" => ("Open", "開く"),
        // #72 툴바 툴팁 보완(Single/Grid 토글·⚙ 설정 버튼).
        "T 키로 전환" => ("Press T to switch", "Tキーで切替"),
        "설정" => ("Settings", "設定"),
        // #74 설정 화면 헤더 전용 캡션 — 재사용 키("설정")의 En이 "Settings"라 영어 UI에서
        // 기존 "Settings — Keyboard & General"이 축약됐던 문제. 헤더 전용 키로 전체 제목을 복원한다.
        "설정 — 키보드 · 일반" => ("Settings — Keyboard & General", "設定 — キーボードと一般"),
        // #75 디코드 실패 고착 회복 — ⚠ 상태에서 수동 재시도 안내.
        "클릭하여 재시도" => ("Click to retry", "クリックで再試行"),
        // #78 컬링 되돌리기(Undo/Redo) 토스트 피드백.
        "되돌렸습니다" => ("Undone", "元に戻しました"),
        "다시 실행했습니다" => ("Redone", "やり直しました"),
        "되돌릴 작업이 없습니다" => ("Nothing to undo", "元に戻す操作がありません"),
        "다시 실행할 작업이 없습니다" => ("Nothing to redo", "やり直す操作がありません"),
        "되돌리기" => ("Undo", "元に戻す"),
        "다시 실행" => ("Redo", "やり直し"),
        // #69 설정 다듬기 — 기본값 복원·자동 업데이트 확인
        "기본값 복원" => ("Reset to Defaults", "デフォルトに戻す"),
        "모든 설정을 기본값으로 되돌립니다" => ("This resets all settings to their defaults", "すべての設定をデフォルトに戻します"),
        "복원" => ("Reset", "リセット"),
        "설정을 기본값으로 되돌렸습니다" => ("Settings were reset to defaults", "設定をデフォルトに戻しました"),
        "새 버전 자동 확인" => ("Check for new versions automatically", "新バージョンを自動確認"),
        // #73 i18n 마감 — 다이얼로그 캡션·레일 라벨·설정 섹션 등 하드코딩 영문을 tr() 경유로.
        // En 값은 모두 기존 영문과 동일(영어 UI 무변화). section_head/section_label이 표시 시
        // 대문자화하는 캡션은 소스가 이미 대문자거나(설정) En 원형(레일 "Color" 등)을 그대로 둔다.
        // ── 전송·정리 다이얼로그 섹션 캡션(section_label) ──
        "원본 라벨" => ("SOURCE LABELS", "元ラベル"),
        "별점 기준" => ("STAR RATING", "評価"), // 기존 "별점"(Rating)과 별개 키(캡션용).
        "컬러 태그 기준" => ("COLOR TAGS", "カラータグ"),
        "동작" => ("ACTION", "動作"),
        "동반 파일" => ("COMPANIONS", "付随ファイル"),
        "대상 폴더" => ("DESTINATION", "出力先"), // 기존 "대상 폴더 열기"와 별개 키.
        "이름 충돌 시" => ("ON FILENAME CONFLICT", "同名ファイル時"),
        "리네임" => ("RENAME", "リネーム"),
        "범위" => ("SCOPE", "範囲"),
        "기준" => ("BY", "基準"),
        "미리보기" => ("PREVIEW", "プレビュー"),
        // ── 푸터/결과 목록 라벨 ──
        "전송 대상" => ("WILL TRANSFER", "転送対象"),
        "정리 대상" => ("WILL ORGANIZE", "整理対象"),
        "분석 대상" => ("WILL ANALYZE", "分析対象"),
        "이름 변경" => ("RENAMED", "リネーム済み"), // 결과창 "RENAMED · N" 헤더(· N 접미는 유지).
        "실패 목록" => ("FAILED", "失敗"),           // 결과창 "FAILED · N" 헤더.
        // ── Browse 버튼 · Copy 세그 제목("이동"→Move는 #66 키 재사용) ──
        "찾아보기…" => ("Browse…", "参照…"),
        "복사" => ("Copy", "コピー"),
        // ── AI 컬링 다이얼로그 섹션 캡션(section_label). "범위"(SCOPE)는 위 전송 키 재사용 ──
        "검사" => ("CHECKS", "検査"),
        "옵션" => ("OPTIONS", "オプション"),
        "메타 필터" => ("METADATA FILTER", "メタフィルタ"),
        "중복 정리" => ("DEDUP / BEST-OF", "重複整理"),
        "AI 축" => ("AI AXES", "AI軸"),
        "검출" => ("DETECT", "検出"),
        "결과 지정" => ("ASSIGN RESULT TO", "結果の割り当て"),
        // ── 좌측 레일 섹션 헤더(section_head — 표시 시 대문자화). "분류"(Classify)·"별점"(Rating)은 재사용 ──
        "색" => ("Color", "色"),
        "진행" => ("Progress", "進行"),
        "보기 필터" => ("Filter View", "表示フィルタ"),
        "별점 필터" => ("Filter Stars", "評価フィルタ"),
        "색 필터" => ("Filter Color", "色フィルタ"),
        // ── 설정 섹션 캡션. 제목 "설정"(Settings)은 #72 키 재사용 ──
        "일반" => ("GENERAL", "一般"),
        "사진 배경" => ("PHOTO BACKGROUND", "写真の背景"),
        "라벨" => ("LABELS", "ラベル"),
        "색 태그 이름" => ("COLOR TAGS", "カラータグ"), // 설정 캡션용(전송의 "컬러 태그 기준"과 별개).
        "캐시" => ("CACHE", "キャッシュ"),
        "초기화" => ("RESET", "リセット"), // 기존 "복원"(Reset, 버튼)과 별개 키(섹션 캡션용).
        "정보" => ("ABOUT", "情報"),
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
        // #65 끝 도달 피드백.
        assert_eq!(trf(Lang::En, "마지막 사진 · 미분류 {}장", &["3"]), "Last photo · 3 unrated");
        assert_eq!(trf(Lang::Ja, "마지막 사진 · 미분류 {}장", &["3"]), "最後の写真 · 未分類 3枚");
    }

    #[test]
    fn from_locale_maps_prefixes() {
        assert_eq!(Lang::from_locale("ko-KR"), Some(Lang::Ko));
        assert_eq!(Lang::from_locale("ja_JP"), Some(Lang::Ja));
        assert_eq!(Lang::from_locale("en-US"), Some(Lang::En));
        assert_eq!(Lang::from_locale("fr-FR"), None);
    }

    #[test]
    fn ai_cull_strings_are_translated() {
        // #50~#53 AI 컬링 UI가 En/Ja에서 한국어로 폴백되던 회귀 방지(대표 키 스팟체크).
        // #64 에러 상태 문구, #67 빈 화면 문구·필터 초기화 CTA, #72 툴바 툴팁 키도 함께 스팟체크.
        // #70 기본/고급 접기·사진가 용어 리라벨·다운로드 미포함 빌드 문구 키도 스팟체크.
        for ko in [
            "AI 컬링",
            "컬링 시작",
            "초점(선명도)",
            "연사 베스트-N",
            "장르 픽",
            "파일명 일부를 입력하세요",
            "이 파일을 열 수 없습니다",
            "이 폴더에 표시할 사진이 없습니다",
            "필터와 일치하는 사진이 없습니다",
            "필터 초기화",
            "T 키로 전환",
            // #69 설정 다듬기 키(대표 2개 스팟체크).
            "기본값 복원",
            "새 버전 자동 확인",
            // #70 기본/고급 접기·용어 리라벨 키.
            "고급 옵션",
            "고급 옵션이 적용 중입니다",
            "미적 점수 하한",
            "차이 허용 ≤",
            "이 빌드에는 모델 다운로드가 포함되지 않았습니다",
        ] {
            assert_ne!(tr(Lang::En, ko), ko, "En 번역 누락: {ko}");
            assert_ne!(tr(Lang::Ja, ko), ko, "Ja 번역 누락: {ko}");
        }
        assert_eq!(tr(Lang::En, "AI 컬링"), "AI Culling");
        assert_eq!(tr(Lang::Ja, "AI 컬링"), "AIカリング");
        assert_eq!(tr(Lang::En, "고급 옵션"), "Advanced Options");
        // #71 모델 다운로드 모달 — 취소 토스트·HTTP/mkdir 에러 번역.
        assert_eq!(tr(Lang::En, "모델 다운로드를 취소했습니다"), "Model download canceled");
        assert_eq!(tr(Lang::Ja, "모델 다운로드를 취소했습니다"), "モデルのダウンロードをキャンセルしました");
        assert_eq!(trf(Lang::En, "HTTP 오류: {}", &["timeout"]), "HTTP error: timeout");
        assert_eq!(trf(Lang::Ja, "폴더 생성 실패: {}", &["denied"]), "フォルダ作成失敗: denied");
        // 서식 문자열도 표에 있어야 한다(슬롯 수 유지).
        assert_eq!(
            trf(Lang::En, "AI 컬링 완료 · 좋음 {} · 탈락 {}", &["3", "1"]),
            "AI culling done · Good 3 · Culled 1"
        );
        assert_eq!(trf(Lang::Ja, "1 ~ {} 사이 번호", &["42"]), "1～42 の番号");
    }

    #[test]
    fn save_failure_strings_are_translated() {
        // #62 저장 실패 표면화 — 실패 안내가 En/Ja에서 한국어로 폴백되지 않는지 스팟체크.
        assert_eq!(tr(Lang::En, "저장 실패"), "Save failed");
        assert_eq!(tr(Lang::Ja, "저장 실패"), "保存失敗");
        // OS 오류 문자열 슬롯({})이 언어별 템플릿에서도 유지돼야 한다.
        assert_eq!(
            trf(Lang::En, "셀렉 저장 실패: {} — 폴더 쓰기 권한·용량을 확인하세요", &["permission denied"]),
            "Failed to save selections: permission denied — check folder write permission/space"
        );
        assert_eq!(
            trf(Lang::Ja, "이전 폴더({}) 셀렉 저장 실패 — 최근 변경이 유실될 수 있습니다", &["DCIM"]),
            "前のフォルダ（DCIM）のセレクト保存に失敗 — 直近の変更が失われた可能性があります"
        );
    }

    #[test]
    fn issue63_move_confirm_and_result_strings_translated() {
        // #63 원본 이동 확인 오버레이·전송 결과 정직성 문자열이 En/Ja로 번역되는지 스팟체크.
        assert_eq!(tr(Lang::En, "이동 시작"), "Start Move");
        assert_eq!(tr(Lang::Ja, "정리 완료"), "整理完了");
        assert_eq!(tr(Lang::En, "작업이 예기치 않게 중단되었습니다"), "The operation stopped unexpectedly");
        // 서식 문자열(슬롯 유지) — 확인 안내·건너뜀·원본 삭제 실패.
        assert_eq!(
            trf(Lang::En, "원본 {}개를 이동합니다 — 원래 폴더에서 제거됩니다.", &["12"]),
            "Moving 12 originals — they will be removed from the source folder."
        );
        assert_eq!(trf(Lang::Ja, "건너뜀 {} — 동명 파일 존재", &["3"]), "スキップ 3 — 同名ファイルあり");
        assert_eq!(
            trf(Lang::En, "원본 삭제 실패 {} — 원본 파일이 남아 있습니다", &["2"]),
            "Failed to delete 2 originals — the source files remain"
        );
    }

    #[test]
    fn help_overlay_strings_are_translated() {
        // #66 단축키 치트시트: 새 키들이 En/Ja에서 한국어로 폴백되지 않는지 스팟체크.
        for ko in ["단축키", "이동", "채택", "히스토그램", "컬러 태그", "휠"] {
            assert_ne!(tr(Lang::En, ko), ko, "En 번역 누락: {ko}");
            assert_ne!(tr(Lang::Ja, ko), ko, "Ja 번역 누락: {ko}");
        }
        assert_eq!(tr(Lang::En, "단축키"), "Keyboard Shortcuts");
        assert_eq!(tr(Lang::Ja, "단축키"), "ショートカット");
        // 섹션 헤더 "파일"은 기존 키 재사용(중복 추가 없이 폴백 아님).
        assert_eq!(tr(Lang::En, "파일"), "Files");
    }

    #[test]
    fn issue73_caption_and_rail_labels_translated() {
        // #73 i18n 마감: 다이얼로그 섹션 캡션·푸터·레일 헤더의 대표 키가 En/Ja에서 한국어로
        // 폴백되지 않는지 스팟체크(대화상자 캡션 + 레일 헤더 + 버튼/푸터 혼합).
        for ko in [
            "원본 라벨", // 전송 SOURCE LABELS
            "검사",      // 컬링 CHECKS
            "일반",      // 설정 GENERAL
            "보기 필터", // 레일 Filter View
            "찾아보기…", // Browse 버튼
            "전송 대상", // 푸터 WILL TRANSFER
            "전송 폴더 기본값",
            "한 폴더로",
            "폴더 나누기",
        ] {
            assert_ne!(tr(Lang::En, ko), ko, "En 번역 누락: {ko}");
            assert_ne!(tr(Lang::Ja, ko), ko, "Ja 번역 누락: {ko}");
        }
        // En 값은 기존 하드코딩 영문과 바이트 동일해야 한다(영어 UI 무변화).
        assert_eq!(tr(Lang::En, "원본 라벨"), "SOURCE LABELS");
        assert_eq!(tr(Lang::En, "검사"), "CHECKS");
        assert_eq!(tr(Lang::En, "일반"), "GENERAL");
        assert_eq!(tr(Lang::En, "찾아보기…"), "Browse…");
        assert_eq!(tr(Lang::En, "전송 대상"), "WILL TRANSFER");
        assert_eq!(tr(Lang::En, "이름 변경"), "RENAMED"); // 결과창 "RENAMED · N" 헤더의 접두.
        // 재사용 키(레일 헤더·Copy/Move·설정 제목)는 기존 En 값을 그대로 쓴다 — section_head가
        // 표시 시 대문자화하므로 "Classify"→"CLASSIFY"로 기존 영문과 동일하게 보인다.
        assert_eq!(tr(Lang::En, "분류"), "Classify");
        assert_eq!(tr(Lang::En, "별점"), "Rating");
        assert_eq!(tr(Lang::En, "복사"), "Copy"); // Copy 세그(이동=Move는 #66 키 재사용).
        assert_eq!(tr(Lang::En, "설정"), "Settings"); // 설정 제목(#72 키 재사용).
        // Ja 대표 확인(레일 헤더 + 컬링 캡션).
        assert_eq!(tr(Lang::Ja, "결과 지정"), "結果の割り当て");
        assert_eq!(tr(Lang::Ja, "색 필터"), "色フィルタ");
    }
}
