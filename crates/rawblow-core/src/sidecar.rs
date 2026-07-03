//! 비파괴 사이드카 (F5): 분류 결과를 폴더 내 `.rawblow/session.json`에 저장/복원.
//! PRD §8 스키마(version=1, items=stem 키). 사람용 txt도 함께 출력.

use crate::model::{ColorTag, Entry, Label};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub const SIDECAR_DIR: &str = ".rawblow";
pub const SIDECAR_FILE: &str = "session.json";
pub const SIDECAR_TXT: &str = "session.txt";
/// 직전 정상 저장본(한 세대). 외부 요인으로 session.json이 손상돼도 여기서 복구한다.
pub const SIDECAR_BAK: &str = "session.json.bak";
/// 파싱에 실패한 손상본 보존 이름(수동 복구·진단용). 다음 저장이 덮어쓰지 않게 치워 둔다.
pub const SIDECAR_CORRUPT: &str = "session.json.corrupt";
pub const VERSION: u32 = 1;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Session {
    pub version: u32,
    pub folder: String,
    pub updated_at: String,
    pub items: BTreeMap<String, ItemRec>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ItemRec {
    pub label: Label,
    /// 별점(0~5). 구버전 세션(필드 없음)은 0으로 로드된다(#23).
    #[serde(default)]
    pub stars: u8,
    /// 컬러 태그(#27). 구버전 세션(필드 없음)은 `None`으로 로드된다.
    #[serde(default)]
    pub tag: ColorTag,
    pub members: Vec<String>,
}

pub fn sidecar_dir(folder: &Path) -> PathBuf {
    folder.join(SIDECAR_DIR)
}
pub fn sidecar_path(folder: &Path) -> PathBuf {
    sidecar_dir(folder).join(SIDECAR_FILE)
}
pub fn sidecar_txt_path(folder: &Path) -> PathBuf {
    sidecar_dir(folder).join(SIDECAR_TXT)
}

/// 사이드카를 읽는다(없으면 None). 파손 시엔 손상본을 `.corrupt`로 치워 두고
/// (다음 저장이 덮어써 증거가 사라지는 것 방지) 직전 백업(`.bak`)으로 복구를 시도한다 —
/// 복구까지 실패하면 None. 파일이 아예 없을 때는 백업을 뒤지지 않는다(의도적 초기화 존중).
pub fn load(folder: &Path) -> Option<Session> {
    let data = std::fs::read_to_string(sidecar_path(folder)).ok()?;
    if let Ok(s) = serde_json::from_str(&data) {
        return Some(s);
    }
    let dir = sidecar_dir(folder);
    let _ = std::fs::rename(sidecar_path(folder), dir.join(SIDECAR_CORRUPT));
    let bak = std::fs::read_to_string(dir.join(SIDECAR_BAK)).ok()?;
    serde_json::from_str(&bak).ok()
}

/// 멤버 경로를 폴더 기준 상대 문자열로(불가하면 파일명).
fn rel(folder: &Path, p: &Path) -> String {
    p.strip_prefix(folder)
        .ok()
        .and_then(|r| r.to_str())
        .map(|s| s.to_string())
        .or_else(|| p.file_name().and_then(|n| n.to_str()).map(|s| s.to_string()))
        .unwrap_or_default()
}

/// 분류된(미선택 제외) 항목을 사이드카로 저장한다. txt도 동시 출력.
pub fn save(folder: &Path, entries: &[Entry]) -> std::io::Result<()> {
    let mut items = BTreeMap::new();
    for e in entries {
        if e.label == Label::Unrated && e.stars == 0 && e.tag == ColorTag::None {
            continue; // 미선택 + 무별점 + 무태그는 키 생략(스펙). 별점·태그만 있어도 보존.
        }
        let members = e.members.iter().map(|p| rel(folder, p)).collect();
        items.insert(
            e.stem.clone(),
            ItemRec {
                label: e.label,
                stars: e.stars,
                tag: e.tag,
                members,
            },
        );
    }
    let session = Session {
        version: VERSION,
        folder: folder.to_string_lossy().to_string(),
        updated_at: chrono::Local::now().to_rfc3339(),
        items,
    };

    std::fs::create_dir_all(sidecar_dir(folder))?;
    let json = serde_json::to_string_pretty(&session)
        .map_err(std::io::Error::other)?;
    let main = sidecar_path(folder);
    // 직전 정상본을 .bak으로 보존(best-effort). 저장 자체는 원자적이라 우리 쓰기로는
    // 손상되지 않지만, 외부 요인(동기화 충돌·디스크 오류) 손상 시 한 세대 복구용.
    if main.exists() {
        let _ = std::fs::copy(&main, sidecar_dir(folder).join(SIDECAR_BAK));
    }
    // 원자적 교체(rename)로 잘린 파일을 방지. fsync는 생략 — 이 함수는 UI 스레드에서
    // 300ms 디바운스로 불리므로 느린 NAS에서 프레임 히치를 만들지 않기 위함. 전원차단
    // 최악의 경우에도 rename 원자성 + 위 .bak 폴백(load 참조)으로 직전 세대까지 복구된다
    // (손실 상한 = 디바운스 한 번 분량의 라벨링).
    crate::fsio::write_atomic_nosync(&main, json.as_bytes())?;
    crate::fsio::write_atomic_nosync(&sidecar_txt_path(folder), render_txt(&session).as_bytes())?;
    Ok(())
}

/// 사람이 읽는 txt: 라벨별 stem 목록(다른 도구 붙여넣기용).
pub fn render_txt(session: &Session) -> String {
    let mut out = String::new();
    out.push_str(&format!("# RawBlow session — {}\n", session.folder));
    out.push_str(&format!("# updated {}\n\n", session.updated_at));
    for label in [Label::Pick, Label::Hold, Label::Reject] {
        let stems: Vec<&String> = session
            .items
            .iter()
            .filter(|(_, v)| v.label == label)
            .map(|(k, _)| k)
            .collect();
        out.push_str(&format!("# {} ({})\n", label.ko(), stems.len()));
        for s in stems {
            out.push_str(s);
            out.push('\n');
        }
        out.push('\n');
    }
    // 별점 섹션(있는 것만, 높은 별점부터). 라벨과 독립이므로 별도로 나열.
    for stars in (1..=5u8).rev() {
        let stems: Vec<&String> = session
            .items
            .iter()
            .filter(|(_, v)| v.stars == stars)
            .map(|(k, _)| k)
            .collect();
        if stems.is_empty() {
            continue;
        }
        out.push_str(&format!("# {}★ ({})\n", stars, stems.len()));
        for s in stems {
            out.push_str(s);
            out.push('\n');
        }
        out.push('\n');
    }
    // 컬러 태그 섹션(#27, 있는 것만). 라벨·별점과 독립이므로 별도 나열(영문 슬러그로 안정 표기).
    for tag in ColorTag::ALL {
        let stems: Vec<&String> = session
            .items
            .iter()
            .filter(|(_, v)| v.tag == tag)
            .map(|(k, _)| k)
            .collect();
        if stems.is_empty() {
            continue;
        }
        out.push_str(&format!("# @{} ({})\n", tag.slug(), stems.len()));
        for s in stems {
            out.push_str(s);
            out.push('\n');
        }
        out.push('\n');
    }
    out
}

/// 로드한 세션의 라벨을 현재 항목에 복원한다(stem 대소문자 무시 매칭).
pub fn apply(session: &Session, entries: &mut [Entry]) {
    let map: BTreeMap<String, (Label, u8, ColorTag)> = session
        .items
        .iter()
        .map(|(k, v)| (k.to_ascii_lowercase(), (v.label, v.stars, v.tag)))
        .collect();
    for e in entries.iter_mut() {
        if let Some((l, s, t)) = map.get(&e.stem.to_ascii_lowercase()) {
            e.label = *l;
            e.stars = (*s).min(5); // 손편집/구포맷의 비정상 값 방어(표시 깨짐 방지).
            e.tag = *t;
        }
    }
}
