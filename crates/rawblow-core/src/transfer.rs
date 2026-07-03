//! 셀렉 파일 전송 (F6, RawPull copier 이식·확장).
//!
//! 분류 라벨 기준으로 대상 파일을 복사/이동한다. 동반 범위(RAW/이미지/모두),
//! 라벨별 하위폴더, 충돌 시 자동 일련번호(덮어쓰기 금지)를 지원한다.
//! 파일번호(stem) 매칭(점프/필터)도 RawPull 방식으로 제공한다.

use crate::model::{kind_of, ColorTag, Entry, Kind, Label, MatchMode};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Action {
    Copy,
    Move,
}

/// 분류 기반 파일명 변경 규칙(#26). 전송(Copy/Move) 시에만 새 이름을 부여한다(원본 무손상).
#[derive(Clone, Debug)]
pub struct RenameRule {
    /// 토큰 템플릿. 지원: `{seq}`(`{seq:03}` 패딩), `{grade}`(A=5★…E=1★), `{gradeseq}`(A1,B2…),
    /// `{stars}`, `{label}`, `{tag}`, `{orig}`(원본 stem). RAW+이미지 페어는 같은 새 stem을 공유.
    pub template: String,
    pub numbering: Numbering,
}

/// 순번 부여 방식(#26).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Numbering {
    /// 선택/정렬 순서대로 1,2,3…
    Order,
    /// 별점 등급별로 묶어 A1,A2,…,B1,…(별점 내림차순). 0★(무별점)은 리네임 제외(원본명 유지).
    GradeGrouped,
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
    /// 전송 대상 별점 집합(1~5). 라벨·태그와 **합집합(OR)**으로 묶인다(#23):
    /// 항목의 라벨이 선택됐거나 별점·태그가 이 집합에 들면 전송 대상.
    pub stars: Vec<u8>,
    /// 전송 대상 컬러 태그 집합(#27). 라벨·별점과 합집합(OR). `None`(무태그)은 매칭 대상 아님.
    pub tags: Vec<ColorTag>,
    pub action: Action,
    pub companions: Companions,
    pub dest: PathBuf,
    pub split_by_label: bool,
    /// 태그별 하위폴더(`@green` 등)로 분기(#27).
    pub split_by_tag: bool,
    pub conflict: ConflictPolicy,
    /// 분류 기반 파일명 변경(#26). `None`이면 원본 이름 유지.
    pub rename: Option<RenameRule>,
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
    /// 사용자가 진행 중 취소해 도중에 멈췄으면 true(#35). 이미 옮긴 파일은 그대로 둔다.
    pub canceled: bool,
}

/// 전송/정리 진행 상황(#35). 백그라운드 스레드가 파일마다 콜백으로 보고하고,
/// UI는 이를 받아 프로그레스바·현재 파일명을 표시한다.
#[derive(Default, Debug, Clone)]
pub struct Progress {
    /// 처리(성공·건너뜀·실패) 완료한 파일 수.
    pub done: usize,
    /// 전체 대상 파일 수(분모).
    pub total: usize,
    /// 누적 전송 바이트.
    pub bytes: u64,
    /// 현재 처리 중인 파일명.
    pub current: String,
}

/// 동반 범위에 따라 항목에서 전송할 멤버를 고른다.
pub fn select_members(entry: &Entry, companions: Companions) -> Vec<&PathBuf> {
    match companions {
        Companions::Both => entry.members.iter().collect(),
        Companions::RawOnly => entry.members_of_kind(Kind::Raw),
        Companions::ImageOnly => entry.members_of_kind(Kind::Image),
    }
}

/// 전송 대상 여부: 라벨 OR 별점 OR 태그(합집합). 무별점(0)·무태그(None)는 매칭 대상이 아니다(#23/#27).
fn is_selected(req: &TransferRequest, e: &Entry) -> bool {
    let label_hit = req.labels.contains(&e.label);
    let star_hit = e.stars >= 1 && req.stars.contains(&e.stars);
    let tag_hit = e.tag != ColorTag::None && req.tags.contains(&e.tag);
    label_hit || star_hit || tag_hit
}

/// 전송 대상 (소스 파일, 소속 라벨, 별점) 목록을 만든다(실행 전 미리보기 카운트에 사용).
pub fn plan(req: &TransferRequest) -> Vec<(PathBuf, Label, u8)> {
    let mut out = Vec::new();
    for e in req.entries {
        if !is_selected(req, e) {
            continue;
        }
        for m in select_members(e, req.companions) {
            out.push((m.clone(), e.label, e.stars));
        }
    }
    out
}

/// 대상 디렉토리에서 충돌을 피한 경로를 만든다. 이름이 바뀌면 새 파일명을 반환.
pub(crate) fn unique_path(dir: &Path, file_name: &str, policy: ConflictPolicy) -> Option<(PathBuf, Option<String>)> {
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

/// 별점(0~5) → 등급 문자(A=5★ … E=1★, 0★은 빈 문자)(#26).
fn grade_letter(stars: u8) -> &'static str {
    match stars {
        5 => "A",
        4 => "B",
        3 => "C",
        2 => "D",
        1 => "E",
        _ => "",
    }
}

/// 파일명 템플릿 치환 컨텍스트(#26).
struct NameCtx<'a> {
    order_seq: usize, // 전체 전송 순서(1-기반)
    grade_seq: usize, // 같은 별점 등급 안에서의 순서(1-기반)
    grade: &'a str,   // 등급 문자
    stars: u8,
    label: &'a str, // 라벨 슬러그(pick/hold/…)
    tag: &'a str,   // 태그 슬러그(red/…/untagged)
    orig: &'a str,  // 원본 stem
    numbering: Numbering,
}

impl NameCtx<'_> {
    fn resolve(&self, token: &str) -> String {
        // "seq:03" → name="seq", pad=Some(3). 숫자 토큰을 0패딩.
        let (name, pad) = match token.split_once(':') {
            Some((n, spec)) => (n, spec.trim().parse::<usize>().ok()),
            None => (token, None),
        };
        let num = |v: usize| match pad {
            Some(w) => format!("{:0width$}", v, width = w),
            None => v.to_string(),
        };
        // {seq}는 번호 방식에 따라: Order=전체순서, GradeGrouped=등급내순서.
        let primary = if self.numbering == Numbering::GradeGrouped {
            self.grade_seq
        } else {
            self.order_seq
        };
        match name {
            "seq" => num(primary),
            "order" => num(self.order_seq),
            "grade" => self.grade.to_string(),
            "gradeseq" => format!("{}{}", self.grade, num(self.grade_seq)),
            "stars" => self.stars.to_string(),
            "label" => self.label.to_string(),
            "tag" => self.tag.to_string(),
            "orig" => self.orig.to_string(),
            _ => String::new(), // 알 수 없는 토큰 → 빈 문자.
        }
    }
}

/// 토큰 템플릿을 새 stem으로 치환(#26). `{name}`/`{name:0N}`. 파일명에 위험한 문자는 `_`로,
/// 결과가 비면 원본 stem으로 폴백.
fn render_name(template: &str, ctx: &NameCtx) -> String {
    let mut out = String::new();
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        if let Some(close) = after.find('}') {
            out.push_str(&ctx.resolve(&after[..close]));
            rest = &after[close + 1..];
        } else {
            out.push_str(&rest[open..]);
            rest = "";
            break;
        }
    }
    out.push_str(rest);
    let cleaned: String = out
        .chars()
        .map(|c| if "/\\:*?\"<>|".contains(c) { '_' } else { c })
        .collect();
    let trimmed = cleaned.trim().trim_matches('.').trim();
    if trimmed.is_empty() {
        ctx.orig.to_string()
    } else {
        trimmed.to_string()
    }
}

/// 전송을 실행한다. 항목(엔트리) 단위로 순회하며 #26 리네임(페어 동일 stem)·#27 태그 분기를 반영.
pub fn execute(req: &TransferRequest) -> TransferReport {
    execute_with_progress(req, &mut |_| true)
}

/// 진행 보고·취소를 받는 전송 실행(#35). `on_progress`는 파일을 처리하기 직전 호출되며,
/// `false`를 반환하면 그 시점에서 중단한다(이미 옮긴 파일은 유지, `report.canceled=true`).
/// 큰 폴더·느린 드라이브에서 UI가 멈춘 것처럼 보이지 않도록 백그라운드 스레드에서 호출한다.
pub fn execute_with_progress(
    req: &TransferRequest,
    on_progress: &mut dyn FnMut(&Progress) -> bool,
) -> TransferReport {
    let mut report = TransferReport::default();
    let mut grade_counter = [0usize; 6]; // 별점 0~5별 등급 내 순번
    let mut order = 0usize; // 전체 전송 순서
    let total = plan(req).len();
    let mut progress = Progress {
        done: 0,
        total,
        bytes: 0,
        current: String::new(),
    };

    for e in req.entries {
        if !is_selected(req, e) {
            continue;
        }
        let members = select_members(e, req.companions);
        if members.is_empty() {
            continue;
        }
        order += 1;
        let gi = e.stars.min(5) as usize;
        grade_counter[gi] += 1;

        // 새 stem(#26): 항목 1개당 한 번 계산 → 모든 동반 멤버가 같은 stem을 공유(페어 유지).
        // GradeGrouped + 0★(무별점)은 리네임 제외(원본명 유지).
        let new_stem = req.rename.as_ref().and_then(|rule| {
            if rule.numbering == Numbering::GradeGrouped && e.stars == 0 {
                return None;
            }
            let ctx = NameCtx {
                order_seq: order,
                grade_seq: grade_counter[gi],
                grade: grade_letter(e.stars),
                stars: e.stars,
                label: label_folder(e.label),
                tag: e.tag.slug(),
                orig: &e.stem,
                numbering: rule.numbering,
            };
            Some(render_name(&rule.template, &ctx))
        });

        // 분기 폴더: 라벨(있으면) + 태그(있으면)(#27).
        let mut target_dir = req.dest.clone();
        if req.split_by_label {
            target_dir = target_dir.join(split_folder(e.label, e.stars));
        }
        if req.split_by_tag && e.tag != ColorTag::None {
            target_dir = target_dir.join(format!("@{}", e.tag.slug()));
        }
        if let Err(err) = std::fs::create_dir_all(&target_dir) {
            for src in &members {
                report.failed.push(((*src).clone(), err.to_string()));
            }
            continue;
        }

        for src in members {
            let file_name = match src.file_name().and_then(|s| s.to_str()) {
                Some(n) => n.to_string(),
                None => {
                    report.failed.push((src.clone(), "invalid filename".into()));
                    progress.done += 1;
                    continue;
                }
            };
            // 진행 보고 + 취소 확인(파일 처리 직전). 취소면 지금까지 결과로 즉시 반환.
            progress.current = file_name.clone();
            if !on_progress(&progress) {
                report.canceled = true;
                return report;
            }
            // 새 이름 = 템플릿 stem + 원본 확장자(없으면 stem). 리네임 없으면 원본명 그대로.
            let out_name = match &new_stem {
                Some(stem) => match src.extension().and_then(|s| s.to_str()) {
                    Some(ext) => format!("{stem}.{ext}"),
                    None => stem.clone(),
                },
                None => file_name.clone(),
            };

            let (dst, conflict_renamed) = match unique_path(&target_dir, &out_name, req.conflict) {
                Some(v) => v,
                None => {
                    report.skipped += 1;
                    progress.done += 1;
                    continue;
                }
            };

            let size = std::fs::metadata(src).map(|m| m.len()).unwrap_or(0);
            let result = match req.action {
                Action::Copy => std::fs::copy(src, &dst).map(|_| ()),
                Action::Move => move_file(src, &dst),
            };

            match result {
                Ok(()) => {
                    report.transferred += 1;
                    report.bytes += size;
                    match kind_of(src) {
                        Some(Kind::Raw) => report.raw_count += 1,
                        Some(Kind::Image) => report.image_count += 1,
                        None => {}
                    }
                    let final_name = conflict_renamed.unwrap_or(out_name);
                    if final_name != file_name {
                        report.renamed.push((file_name, final_name));
                    }
                }
                Err(err) => report.failed.push((src.clone(), err.to_string())),
            }
            progress.done += 1;
            progress.bytes = report.bytes;
        }
    }

    report
}

/// 전송하지 않고 (원본 파일명 → 새 파일명) 미리보기를 만든다(#26 다이얼로그 프리뷰).
/// 충돌(_001) 회피는 반영하지 않는다(대상 폴더 상태와 무관한 순수 이름 미리보기). `limit`개까지.
pub fn rename_preview(req: &TransferRequest, limit: usize) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut grade_counter = [0usize; 6];
    let mut order = 0usize;
    for e in req.entries {
        if !is_selected(req, e) {
            continue;
        }
        let members = select_members(e, req.companions);
        if members.is_empty() {
            continue;
        }
        order += 1;
        let gi = e.stars.min(5) as usize;
        grade_counter[gi] += 1;
        let new_stem = req.rename.as_ref().and_then(|rule| {
            if rule.numbering == Numbering::GradeGrouped && e.stars == 0 {
                return None;
            }
            let ctx = NameCtx {
                order_seq: order,
                grade_seq: grade_counter[gi],
                grade: grade_letter(e.stars),
                stars: e.stars,
                label: label_folder(e.label),
                tag: e.tag.slug(),
                orig: &e.stem,
                numbering: rule.numbering,
            };
            Some(render_name(&rule.template, &ctx))
        });
        for src in members {
            let file_name = src.file_name().and_then(|s| s.to_str()).unwrap_or("").to_string();
            let new_name = match &new_stem {
                Some(stem) => match src.extension().and_then(|s| s.to_str()) {
                    Some(ext) => format!("{stem}.{ext}"),
                    None => stem.clone(),
                },
                None => file_name.clone(),
            };
            out.push((file_name, new_name));
            if out.len() >= limit {
                return out;
            }
        }
    }
    out
}

/// rename 우선, 교차 디바이스면 copy+remove로 이동. 부분 실패 시 상태를 정리한다:
/// - copy 실패(디스크풀 등) → 대상의 불완전 사본을 제거하고 오류 반환(잘린 파일 잔존 방지).
/// - copy 성공 후 원본 삭제 실패 → **성공으로 처리**. 파일은 이미 대상에 온전히 있으므로,
///   실패로 보고하면 재시도가 `_001` 중복본을 만든다(원본 잔존이 중복 생성보다 안전).
///   Windows에서 흔한 원인인 읽기전용 속성(메모리카드에서 온 파일)은 해제 후 1회 재시도한다.
pub(crate) fn move_file(src: &Path, dst: &Path) -> std::io::Result<()> {
    if std::fs::rename(src, dst).is_ok() {
        return Ok(());
    }
    copy_then_remove(src, dst)
}

/// 교차 디바이스 이동 폴백 본체(위 계약 참조). move_file에서 분리해 직접 테스트한다.
fn copy_then_remove(src: &Path, dst: &Path) -> std::io::Result<()> {
    if let Err(e) = std::fs::copy(src, dst) {
        let _ = std::fs::remove_file(dst); // 불완전 사본 정리(없으면 무시)
        return Err(e);
    }
    if std::fs::remove_file(src).is_err() {
        retry_remove_readonly(src);
    }
    Ok(())
}

/// Windows: 읽기전용 속성 때문에 삭제가 거부된 파일을 속성 해제 후 1회 재시도(best-effort).
/// Unix에선 파일 삭제가 파일 자체 권한과 무관(디렉토리 권한 문제)해 재시도가 무의미하다.
#[cfg(windows)]
fn retry_remove_readonly(src: &Path) {
    if let Ok(meta) = std::fs::metadata(src) {
        let mut perm = meta.permissions();
        perm.set_readonly(false);
        if std::fs::set_permissions(src, perm).is_ok() {
            let _ = std::fs::remove_file(src);
        }
    }
}

#[cfg(not(windows))]
fn retry_remove_readonly(_src: &Path) {}

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
    for raw in text.split([',', ';', '\t', '\n', '\r']) {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copy_then_remove_moves_content() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("a.jpg");
        let dst = tmp.path().join("b.jpg");
        std::fs::write(&src, b"payload").unwrap();
        copy_then_remove(&src, &dst).unwrap();
        assert!(!src.exists());
        assert_eq!(std::fs::read(&dst).unwrap(), b"payload");
    }

    #[test]
    fn copy_then_remove_copy_failure_leaves_no_partial() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("a.jpg");
        std::fs::write(&src, b"payload").unwrap();
        let dst = tmp.path().join("no-such-dir").join("b.jpg");
        assert!(copy_then_remove(&src, &dst).is_err());
        assert!(src.exists(), "실패 시 원본은 보존");
        assert!(!dst.exists(), "대상에 불완전 사본이 남지 않음");
    }

    /// copy 성공 후 원본 삭제가 거부돼도 성공으로 처리(재시도발 `_001` 중복 방지 계약).
    /// Unix에서 부모 디렉토리 쓰기 권한을 빼앗아 remove_file 실패를 재현한다.
    #[cfg(unix)]
    #[test]
    fn copy_then_remove_remove_denied_counts_as_success() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let src_dir = tmp.path().join("locked");
        std::fs::create_dir_all(&src_dir).unwrap();
        let src = src_dir.join("a.jpg");
        let dst = tmp.path().join("b.jpg");
        std::fs::write(&src, b"payload").unwrap();
        std::fs::set_permissions(&src_dir, std::fs::Permissions::from_mode(0o555)).unwrap();

        let result = copy_then_remove(&src, &dst);

        // tempdir 정리를 위해 권한 원복(검증 전에 해도 무방 — 결과는 이미 확정).
        std::fs::set_permissions(&src_dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(result.is_ok(), "대상에 온전히 복사됐으면 성공으로 보고");
        assert_eq!(std::fs::read(&dst).unwrap(), b"payload");
        assert!(src.exists(), "삭제 거부된 원본은 잔존(중복 생성보다 안전)");
    }
}
