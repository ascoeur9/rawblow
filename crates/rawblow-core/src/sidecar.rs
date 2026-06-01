//! 비파괴 사이드카 (F5): 분류 결과를 폴더 내 `.rawblow/session.json`에 저장/복원.
//! PRD §8 스키마(version=1, items=stem 키). 사람용 txt도 함께 출력.

use crate::model::{Entry, Label};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub const SIDECAR_DIR: &str = ".rawblow";
pub const SIDECAR_FILE: &str = "session.json";
pub const SIDECAR_TXT: &str = "session.txt";
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

/// 사이드카를 읽는다(없거나 파손 시 None).
pub fn load(folder: &Path) -> Option<Session> {
    let data = std::fs::read_to_string(sidecar_path(folder)).ok()?;
    serde_json::from_str(&data).ok()
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
        if e.label == Label::Unrated && e.stars == 0 {
            continue; // 미선택 + 무별점은 키 생략(스펙). 별점만 있어도 보존.
        }
        let members = e.members.iter().map(|p| rel(folder, p)).collect();
        items.insert(
            e.stem.clone(),
            ItemRec {
                label: e.label,
                stars: e.stars,
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
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    std::fs::write(sidecar_path(folder), json)?;
    std::fs::write(sidecar_txt_path(folder), render_txt(&session))?;
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
    out
}

/// 로드한 세션의 라벨을 현재 항목에 복원한다(stem 대소문자 무시 매칭).
pub fn apply(session: &Session, entries: &mut [Entry]) {
    let map: BTreeMap<String, (Label, u8)> = session
        .items
        .iter()
        .map(|(k, v)| (k.to_ascii_lowercase(), (v.label, v.stars)))
        .collect();
    for e in entries.iter_mut() {
        if let Some((l, s)) = map.get(&e.stem.to_ascii_lowercase()) {
            e.label = *l;
            e.stars = (*s).min(5); // 손편집/구포맷의 비정상 값 방어(표시 깨짐 방지).
        }
    }
}
