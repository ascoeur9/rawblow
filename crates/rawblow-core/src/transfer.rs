//! 셀렉 파일 전송 (F6, RawPull copier 이식·확장).
//!
//! 분류 라벨 기준으로 대상 파일을 복사/이동한다. 동반 범위(RAW/이미지/모두),
//! 라벨별 하위폴더, 충돌 시 자동 일련번호(덮어쓰기 금지)를 지원한다.
//! 파일번호(stem) 매칭(점프/필터)도 RawPull 방식으로 제공한다.

use crate::model::{kind_of, Entry, Kind, Label, MatchMode};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Action {
    Copy,
    Move,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Companions {
    /// RAW + 이미지 모두(기본).
    Both,
    RawOnly,
    ImageOnly,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ConflictPolicy {
    /// `_001`, `_002` … 자동 증가(기본). 덮어쓰기는 PRD §F6에 의해 금지.
    AutoIncrement,
    Skip,
}

pub struct TransferRequest<'a> {
    pub entries: &'a [Entry],
    pub labels: Vec<Label>,
    /// 전송 대상 별점 집합(1~5). 라벨과 **합집합(OR)**으로 묶인다(#23):
    /// 항목의 라벨이 선택됐거나 별점이 이 집합에 들면 전송 대상.
    pub stars: Vec<u8>,
    pub action: Action,
    pub companions: Companions,
    pub dest: PathBuf,
    pub split_by_label: bool,
    pub conflict: ConflictPolicy,
}

#[derive(Default, Debug, Clone)]
pub struct TransferReport {
    pub raw_count: usize,
    pub image_count: usize,
    pub transferred: usize,
    pub skipped: usize,
    pub failed: Vec<(PathBuf, String)>,
    /// (원래 파일명 → 변경된 파일명)
    pub renamed: Vec<(String, String)>,
    pub bytes: u64,
}

/// 동반 범위에 따라 항목에서 전송할 멤버를 고른다.
pub fn select_members<'a>(entry: &'a Entry, companions: Companions) -> Vec<&'a PathBuf> {
    match companions {
        Companions::Both => entry.members.iter().collect(),
        Companions::RawOnly => entry.members_of_kind(Kind::Raw),
        Companions::ImageOnly => entry.members_of_kind(Kind::Image),
    }
}

/// 전송 대상 (소스 파일, 소속 라벨, 별점) 목록을 만든다(실행 전 미리보기에도 사용).
/// 별점은 split_by_label 시 라벨 없는(별점만 매긴) 항목의 분기 폴더 결정에 쓰인다.
pub fn plan(req: &TransferRequest) -> Vec<(PathBuf, Label, u8)> {
    let mut out = Vec::new();
    for e in req.entries {
        // 라벨 OR 별점(합집합). 무별점(0)은 별점 매칭 대상이 아니다.
        let label_hit = req.labels.contains(&e.label);
        let star_hit = e.stars >= 1 && req.stars.contains(&e.stars);
        if !(label_hit || star_hit) {
            continue;
        }
        for m in select_members(e, req.companions) {
            out.push((m.clone(), e.label, e.stars));
        }
    }
    out
}

/// 대상 디렉토리에서 충돌을 피한 경로를 만든다. 이름이 바뀌면 새 파일명을 반환.
fn unique_path(dir: &Path, file_name: &str, policy: ConflictPolicy) -> Option<(PathBuf, Option<String>)> {
    let candidate = dir.join(file_name);
    if !candidate.exists() {
        return Some((candidate, None));
    }
    match policy {
        ConflictPolicy::Skip => None,
        ConflictPolicy::AutoIncrement => {
            let p = Path::new(file_name);
            let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or(file_name);
            let ext = p.extension().and_then(|s| s.to_str());
            for n in 1..100_000u32 {
                let name = match ext {
                    Some(e) => format!("{stem}_{n:03}.{e}"),
                    None => format!("{stem}_{n:03}"),
                };
                let p = dir.join(&name);
                if !p.exists() {
                    return Some((p, Some(name)));
                }
            }
            None
        }
    }
}

/// 전송을 실행한다.
pub fn execute(req: &TransferRequest) -> TransferReport {
    let mut report = TransferReport::default();

    for (src, label, stars) in plan(req) {
        let target_dir = if req.split_by_label {
            req.dest.join(split_folder(label, stars))
        } else {
            req.dest.clone()
        };
        if let Err(e) = std::fs::create_dir_all(&target_dir) {
            report.failed.push((src.clone(), e.to_string()));
            continue;
        }

        let file_name = match src.file_name().and_then(|s| s.to_str()) {
            Some(n) => n.to_string(),
            None => {
                report.failed.push((src.clone(), "invalid filename".into()));
                continue;
            }
        };

        let (dst, renamed) = match unique_path(&target_dir, &file_name, req.conflict) {
            Some(v) => v,
            None => {
                report.skipped += 1;
                continue;
            }
        };

        let size = std::fs::metadata(&src).map(|m| m.len()).unwrap_or(0);
        let result = match req.action {
            Action::Copy => std::fs::copy(&src, &dst).map(|_| ()),
            Action::Move => move_file(&src, &dst),
        };

        match result {
            Ok(()) => {
                report.transferred += 1;
                report.bytes += size;
                match kind_of(&src) {
                    Some(Kind::Raw) => report.raw_count += 1,
                    Some(Kind::Image) => report.image_count += 1,
                    None => {}
                }
                if let Some(new_name) = renamed {
                    report.renamed.push((file_name, new_name));
                }
            }
            Err(e) => report.failed.push((src.clone(), e.to_string())),
        }
    }

    report
}

/// rename 우선, 교차 디바이스면 copy+remove로 이동.
fn move_file(src: &Path, dst: &Path) -> std::io::Result<()> {
    match std::fs::rename(src, dst) {
        Ok(()) => Ok(()),
        Err(_) => {
            std::fs::copy(src, dst)?;
            std::fs::remove_file(src)
        }
    }
}

fn label_folder(label: Label) -> &'static str {
    match label {
        Label::Pick => "pick",
        Label::Hold => "hold",
        Label::Reject => "reject",
        Label::Unrated => "unrated",
    }
}

/// split_by_label 시 분기 폴더명. 라벨이 있으면 라벨 폴더, 라벨 없이 별점만 매긴 항목은
/// `5star` 같은 별점 폴더로 보낸다(#23/#24 함정 방지: 별점-only 베스트컷이 `unrated/`로 가지 않게).
fn split_folder(label: Label, stars: u8) -> String {
    if label != Label::Unrated {
        label_folder(label).to_string()
    } else if stars >= 1 {
        format!("{}star", stars.min(5))
    } else {
        "unrated".to_string()
    }
}

// ── 파일번호 매칭 (RawPull 점프/필터 계승) ──────────────────────────

/// 입력 텍스트를 구분자(`, ; 탭 줄바꿈`)로 쪼개 중복 제거한 검색어 목록.
pub fn parse_terms(text: &str) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for raw in text.split(|c| matches!(c, ',' | ';' | '\t' | '\n' | '\r')) {
        let t = raw.trim();
        if !t.is_empty() && seen.insert(t.to_ascii_lowercase()) {
            out.push(t.to_string());
        }
    }
    out
}

/// 검색어에 매칭되는 항목 인덱스 목록(stem 기준 contains/exact).
pub fn match_indices(entries: &[Entry], terms: &[String], mode: MatchMode) -> Vec<usize> {
    let lowered: Vec<String> = terms.iter().map(|t| t.to_ascii_lowercase()).collect();
    let mut out = Vec::new();
    for (i, e) in entries.iter().enumerate() {
        let stem = e.stem.to_ascii_lowercase();
        let hit = lowered.iter().any(|t| match mode {
            MatchMode::Exact => &stem == t,
            MatchMode::Contains => stem.contains(t.as_str()),
        });
        if hit {
            out.push(i);
        }
    }
    out
}
