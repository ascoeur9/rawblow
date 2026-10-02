//! 비파괴 사이드카 (F5): 분류 결과를 폴더 내 `.rawblow/session.json`에 저장/복원.
//! PRD §8 스키마(version=1, items=stem 키). 사람용 txt도 함께 출력.

use crate::model::{ColorTag, Entry, Label};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
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
/// `.bak`으로 읽으면 `session.json`에 다시 써서, 라벨을 안 바꾼 채 다시 열어도 분류가 남는다(#96).
pub fn load(folder: &Path) -> Option<Session> {
    let data = std::fs::read(sidecar_path(folder)).ok()?;
    // UTF-8이 아닌 내용도 파싱 실패와 똑같이 손상 처리(예전엔 '없음'으로 취급돼 .corrupt·복구 없이
    // 다음 저장이 손상본을 .bak 위에 복사했다).
    if let Some(s) = parse(&data) {
        return Some(s);
    }
    let dir = sidecar_dir(folder);
    let _ = std::fs::rename(sidecar_path(folder), dir.join(SIDECAR_CORRUPT));
    let bak = std::fs::read_to_string(dir.join(SIDECAR_BAK)).ok()?;
    let session: Session = serde_json::from_str(&bak).ok()?;
    // 복구한 내용을 메인 경로에 되살린다. 실패해도 이번 세션 메모리는 유효하다.
    let _ = crate::fsio::write_atomic_nosync(&sidecar_path(folder), bak.as_bytes());
    Some(session)
}

fn parse(data: &[u8]) -> Option<Session> {
    serde_json::from_str(std::str::from_utf8(data).ok()?).ok()
}

/// 멤버 경로를 폴더 기준 상대 문자열로(불가하면 파일명).
fn rel(folder: &Path, p: &Path) -> String {
    p.strip_prefix(folder)
        .ok()
        .and_then(|r| r.to_str())
        .map(|s| s.to_string())
        .or_else(|| p.file_name().and_then(|n| n.to_str()).map(|s| s.to_string()))
        .unwrap_or_default()
        .replace('\\', "/")
}

/// 사이드카 항목 키(#98). 재귀 스캔에서 같은 파일번호가 여러 장이면 stem만으로는 덮어쓴다.
/// 폴더 기준 상대 경로에서 확장자를 뺀 값(`day1/DSC_0001`). 루트 파일은 기존처럼 stem.
fn item_key(folder: &Path, e: &Entry) -> String {
    let primary = e.members.first().map(|p| p.as_path()).unwrap_or(e.display.as_path());
    path_key(folder, primary).unwrap_or_else(|| e.stem.clone())
}

/// 파일 하나의 키 형태(폴더 기준 상대 경로, 확장자 제외).
fn path_key(folder: &Path, p: &Path) -> Option<String> {
    let rel = rel(folder, p);
    let no_ext = Path::new(&rel).with_extension("");
    no_ext.to_str().map(|s| s.replace('\\', "/")).filter(|s| !s.is_empty())
}

/// 키·멤버 경로 비교용 정규화('/' 구분, 대소문자 무시).
fn norm(s: &str) -> String {
    s.replace('\\', "/").to_ascii_lowercase()
}

/// 저장된 기록 색인. 항목 키는 첫 멤버를 따르므로 페어링·멤버가 바뀌면 달라진다 —
/// 그래서 기록을 항목 키 하나로만 찾지 않고 멤버 키·저장된 멤버 경로로도 찾는다.
struct Index<'a> {
    by_key: BTreeMap<String, &'a ItemRec>,
    /// 정규화 멤버 경로 → 그 멤버를 담은 기록 키들(키 순).
    by_member: BTreeMap<String, Vec<String>>,
}

impl<'a> Index<'a> {
    fn new(session: &'a Session) -> Self {
        let mut by_key = BTreeMap::new();
        let mut by_member: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for (k, v) in &session.items {
            let k = norm(k);
            for m in &v.members {
                by_member.entry(norm(m)).or_default().push(k.clone());
            }
            by_key.insert(k, v);
        }
        Index { by_key, by_member }
    }

    /// 항목에 속한 기록 키(우선순위 순, 중복 없음): 항목 자신의 키 → 멤버 순서대로
    /// 그 멤버의 키, 그 멤버 경로를 담은 기록.
    fn keys_for(&self, folder: &Path, e: &Entry) -> Vec<String> {
        let mut cands = vec![norm(&item_key(folder, e))];
        for m in &e.members {
            cands.extend(path_key(folder, m).map(|k| norm(&k)));
            cands.extend(self.by_member.get(&norm(&rel(folder, m))).into_iter().flatten().cloned());
        }
        let mut out: Vec<String> = Vec::new();
        for k in cands {
            if self.by_key.contains_key(&k) && !out.contains(&k) {
                out.push(k);
            }
        }
        out
    }
}

/// 분류된(미선택 제외) 항목을 사이드카로 저장한다. txt도 동시 출력.
/// 범위는 재귀 목록과 같다(전체 재작성) — 비재귀 목록은 [`save_scoped`].
pub fn save(folder: &Path, entries: &[Entry]) -> std::io::Result<()> {
    save_scoped(folder, entries, true)
}

/// `recursive`는 entries를 만든 **스캔**이 하위 폴더까지 봤는지(설정값이 아니라 실제 스캔 기준).
/// 재귀 목록이면 전체를 다시 써서 묵은 키를 정리한다. 비재귀 목록이면 하위 폴더 기록(키에 '/')은
/// 범위 밖이라 그대로 둔다 — 하위 폴더를 끄고 한 장만 분류해도 재귀 세션 기록이 지워지지 않게.
/// 단 현재 항목이 읽어 가는 기록(병합 항목의 루트 멤버 등)은 이번 저장이 대신하므로 남기지 않는다
/// (미선택으로 되돌린 것이 되살아나지 않게). 기존 파일을 못 읽으면 예전처럼 전체 재작성.
pub fn save_scoped(folder: &Path, entries: &[Entry], recursive: bool) -> std::io::Result<()> {
    let mut items = BTreeMap::new();
    if !recursive {
        if let Some(old) = std::fs::read(sidecar_path(folder)).ok().and_then(|d| parse(&d)) {
            let idx = Index::new(&old);
            let claimed: BTreeSet<String> =
                entries.iter().flat_map(|e| idx.keys_for(folder, e)).collect();
            for (k, v) in old.items {
                let nk = norm(&k);
                if nk.contains('/') && !claimed.contains(&nk) {
                    items.insert(k, v);
                }
            }
        }
    }
    for e in entries {
        if e.label == Label::Unrated && e.stars == 0 && e.tag == ColorTag::None {
            continue; // 미선택 + 무별점 + 무태그는 키 생략(스펙). 별점·태그만 있어도 보존.
        }
        let members = e.members.iter().map(|p| rel(folder, p)).collect();
        items.insert(
            item_key(folder, e),
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

/// 로드한 세션의 라벨을 현재 항목에 복원한다.
/// 키는 상대 경로(확장자 제외). 항목 키뿐 아니라 멤버 키·저장된 멤버 경로로 찾은 기록을 모두
/// 모아 필드별로 합친다(자기 키 기록 우선, 이후 멤버 순: 미선택 아닌 첫 라벨, 0 아닌 첫 별점,
/// 무태그 아닌 첫 태그) — 0.6.1의 jpg/·원본/ 두 키나, 멤버가 바뀌어 달라진 키도 잃지 않게.
/// 옛 stem-only 세션은 아무 기록도 못 찾았고 그 stem이 **한 장뿐일 때**만 폴백(#98).
pub fn apply(session: &Session, entries: &mut [Entry], folder: &Path) {
    let idx = Index::new(session);
    let mut stem_counts: BTreeMap<String, usize> = BTreeMap::new();
    for e in entries.iter() {
        *stem_counts.entry(e.stem.to_ascii_lowercase()).or_insert(0) += 1;
    }
    for e in entries.iter_mut() {
        let mut recs: Vec<&ItemRec> =
            idx.keys_for(folder, e).iter().filter_map(|k| idx.by_key.get(k).copied()).collect();
        let stem_l = e.stem.to_ascii_lowercase();
        if recs.is_empty() && stem_counts.get(&stem_l).copied().unwrap_or(0) == 1 {
            recs.extend(idx.by_key.get(&stem_l).copied());
        }
        if recs.is_empty() {
            continue;
        }
        e.label = recs.iter().map(|r| r.label).find(|l| *l != Label::Unrated).unwrap_or_default();
        // 손편집/구포맷의 비정상 값 방어(표시 깨짐 방지).
        e.stars = recs.iter().map(|r| r.stars).find(|s| *s != 0).unwrap_or(0).min(5);
        e.tag = recs.iter().map(|r| r.tag).find(|t| *t != ColorTag::None).unwrap_or_default();
    }
}
