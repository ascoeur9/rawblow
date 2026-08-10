//! 특정 폴더 내 자동 분류·폴더 이동(#34).
//!
//! 셀렉(전송)과 **별개**의 기능이다. 폴더 안의 사진을 촬영일·카메라·렌즈·초점거리(#92)·확장자
//! 기준으로 하위폴더에 나눠 담은 뒤(이동 또는 복사), 분류된 폴더를 (하위 폴더 포함)
//! 다시 열어 셀렉할 수 있다. 전송과 동일하게 충돌 시 자동 일련번호, 진행 보고·취소를
//! 지원한다(#35 인프라 공유). EXIF 기준(날짜/카메라/렌즈/초점거리)은 항목(페어) 단위로 같은
//! 폴더에 묶어 RAW+JPG 페어를 유지하고, 확장자 기준은 파일 단위로 나눈다.

use crate::meta::read_exif;
use crate::model::{ext_lower, Entry};
use crate::transfer::{move_file, unique_path, Action, ConflictPolicy, Progress, TransferReport};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// 자동 분류 기준(#34). 마지막 사용 옵션 저장(#57)을 위해 serde 파생.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum OrganizeKey {
    /// 촬영일(EXIF DateTimeOriginal → `yyyy-mm-dd`). 없으면 파일 수정일, 그것도 없으면 미상.
    Date,
    /// 카메라 모델(EXIF). 없으면 미상.
    Camera,
    /// 렌즈 모델(EXIF). 없으면 미상.
    Lens,
    /// 초점거리(EXIF FocalLength → `35mm`). 1mm 단위 반올림, 없으면 미상(#92).
    Focal,
    /// 파일 확장자(대문자: RW2, JPG …). 파일 단위로 나뉜다(페어 분리됨).
    Extension,
}

pub struct OrganizeRequest<'a> {
    pub entries: &'a [Entry],
    pub key: OrganizeKey,
    pub action: Action,
    /// 분류 결과를 담을 루트. 보통 현재 열린 폴더(그 안에 하위폴더 생성).
    pub dest_root: PathBuf,
    pub conflict: ConflictPolicy,
}

/// 미상 분류 폴더명(언어 무관 영문 슬러그 — 다시 열 때 안정적).
fn unknown_folder(key: OrganizeKey) -> &'static str {
    match key {
        OrganizeKey::Date => "unknown-date",
        OrganizeKey::Camera => "unknown-camera",
        OrganizeKey::Lens => "unknown-lens",
        OrganizeKey::Focal => "unknown-focal",
        OrganizeKey::Extension => "unknown-ext",
    }
}

/// 파일명에 못 쓰는 문자를 `_`로 바꾸고 양끝을 정리. 빈 결과는 "unknown".
fn sanitize_folder(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| if "/\\:*?\"<>|".contains(c) { '_' } else { c })
        .collect();
    let trimmed = cleaned.trim().trim_matches('.').trim();
    if trimmed.is_empty() {
        "unknown".to_string()
    } else {
        trimmed.to_string()
    }
}

/// EXIF datetime("2026:06:06 13:31:51")의 날짜부를 `yyyy-mm-dd`로. 형식이 아니면 None.
fn date_from_exif(dt: &str) -> Option<String> {
    let date = dt.split_whitespace().next()?; // "2026:06:06"
    let parts: Vec<&str> = date.split(':').collect();
    if parts.len() == 3 && parts[0].len() == 4 && parts.iter().all(|p| p.chars().all(|c| c.is_ascii_digit())) {
        Some(format!("{}-{}-{}", parts[0], parts[1], parts[2]))
    } else {
        None
    }
}

/// 파일 수정일을 `yyyy-mm-dd`로(촬영 EXIF가 없을 때 폴백).
fn date_from_mtime(path: &Path) -> Option<String> {
    use chrono::{DateTime, Local};
    let mtime = std::fs::metadata(path).ok()?.modified().ok()?;
    let dt: DateTime<Local> = mtime.into();
    Some(dt.format("%Y-%m-%d").to_string())
}

/// EXIF FocalLength 표시값("35 mm", "10.5 mm")에서 mm 수치를 읽어 1mm 단위 폴더명(`35mm`)으로
/// 만든다(#92). 소수는 반올림한다(어안 10.5mm → `11mm`). 숫자를 못 읽거나 0 이하면 None →
/// 호출부에서 미상 폴더로 보낸다.
fn focal_folder(raw: &str) -> Option<String> {
    let num: String = raw
        .trim()
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    let mm: f64 = num.parse().ok()?;
    if !mm.is_finite() || mm <= 0.0 {
        return None;
    }
    Some(format!("{}mm", mm.round() as i64))
}

/// 확장자 기준 폴더명(대문자). 확장자가 없으면 미상.
fn ext_folder(path: &Path) -> String {
    match ext_lower(path) {
        Some(e) => e.to_ascii_uppercase(),
        None => unknown_folder(OrganizeKey::Extension).to_string(),
    }
}

/// 항목(페어) 단위 분류 폴더명(Date/Camera/Lens/Focal). EXIF는 대표 파일에서 한 번만 읽는다.
fn folder_for_entry(e: &Entry, key: OrganizeKey) -> String {
    match key {
        OrganizeKey::Extension => {
            // 호출부에서 멤버별로 처리하므로 도달하지 않음. 안전 폴백.
            unknown_folder(key).to_string()
        }
        OrganizeKey::Date => {
            let exif = read_exif(&e.display);
            let from_exif = exif.as_ref().and_then(|x| x.datetime.as_deref()).and_then(date_from_exif);
            from_exif
                .or_else(|| date_from_mtime(&e.display))
                .map(|s| sanitize_folder(&s))
                .unwrap_or_else(|| unknown_folder(key).to_string())
        }
        OrganizeKey::Camera => read_exif(&e.display)
            .and_then(|x| x.camera)
            .map(|s| sanitize_folder(&s))
            .filter(|s| s != "unknown")
            .unwrap_or_else(|| unknown_folder(key).to_string()),
        OrganizeKey::Lens => read_exif(&e.display)
            .and_then(|x| x.lens)
            .map(|s| sanitize_folder(&s))
            .filter(|s| s != "unknown")
            .unwrap_or_else(|| unknown_folder(key).to_string()),
        OrganizeKey::Focal => {
            let raw = read_exif(&e.display).and_then(|x| x.focal_length);
            raw.as_deref()
                .and_then(focal_folder)
                .unwrap_or_else(|| unknown_folder(key).to_string())
        }
    }
}

/// 분류 미리보기: (폴더명, 파일 수)를 폴더명 정렬로 반환. EXIF를 읽으므로 큰 폴더에선
/// 다소 느릴 수 있다(호출부에서 백그라운드 처리 권장). 충돌(_001) 회피는 반영하지 않는다.
pub fn preview(req: &OrganizeRequest) -> Vec<(String, usize)> {
    use std::collections::BTreeMap;
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for e in req.entries {
        let entry_folder = match req.key {
            OrganizeKey::Extension => None,
            _ => Some(folder_for_entry(e, req.key)),
        };
        for src in &e.members {
            let folder = entry_folder.clone().unwrap_or_else(|| ext_folder(src));
            *counts.entry(folder).or_default() += 1;
        }
    }
    counts.into_iter().collect()
}

/// 분류를 실행한다(진행 보고·취소 없이). 테스트·간단 호출용.
pub fn organize(req: &OrganizeRequest) -> TransferReport {
    organize_with_progress(req, &mut |_| true)
}

/// 진행 보고·취소를 받는 분류 실행(#34/#35). EXIF 읽기와 파일 이동을 항목 단위로
/// 인터리브해 진행바가 자연스럽게 움직인다. `on_progress`가 `false`면 그 시점에서
/// 중단(이미 옮긴 파일은 유지, `report.canceled=true`).
pub fn organize_with_progress(
    req: &OrganizeRequest,
    on_progress: &mut dyn FnMut(&Progress) -> bool,
) -> TransferReport {
    use crate::model::{kind_of, Kind};
    let mut report = TransferReport::default();
    let total: usize = req.entries.iter().map(|e| e.members.len()).sum();
    let mut progress = Progress {
        done: 0,
        total,
        bytes: 0,
        current: String::new(),
    };

    for e in req.entries {
        // EXIF 기준은 항목 단위로 폴더를 한 번 산출(페어 유지). 확장자는 멤버별.
        let entry_folder = match req.key {
            OrganizeKey::Extension => None,
            _ => Some(folder_for_entry(e, req.key)),
        };

        for src in &e.members {
            let file_name = match src.file_name().and_then(|s| s.to_str()) {
                Some(n) => n.to_string(),
                None => {
                    report.failed.push((src.clone(), "invalid filename".into()));
                    progress.done += 1;
                    continue;
                }
            };
            progress.current = file_name.clone();
            if !on_progress(&progress) {
                report.canceled = true;
                return report;
            }

            let folder = entry_folder.clone().unwrap_or_else(|| ext_folder(src));
            let target_dir = req.dest_root.join(&folder);

            // 이미 제자리에 있는 파일(같은 대상 폴더)은 옮기지 않고 통과(in-place 재분류 안전).
            if src.parent() == Some(target_dir.as_path()) {
                progress.done += 1;
                continue;
            }

            if let Err(err) = std::fs::create_dir_all(&target_dir) {
                report.failed.push((src.clone(), err.to_string()));
                progress.done += 1;
                continue;
            }

            let (dst, conflict_renamed) = match unique_path(&target_dir, &file_name, req.conflict) {
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
                    // Move인데 원본이 남아 있으면 원본 삭제 실패 — 정직하게 기록(#63, 전송 결과창 공용).
                    if req.action == Action::Move && src.exists() {
                        report.remove_failed.push(src.clone());
                    }
                    if let Some(new_name) = conflict_renamed {
                        if new_name != file_name {
                            report.renamed.push((file_name.clone(), new_name));
                        }
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

#[cfg(test)]
mod tests {
    use super::focal_folder;

    #[test]
    fn focal_folder_parses_exif_display_values() {
        // exif 크레이트의 FocalLength 표시값은 "35 mm" 꼴. 단위와 공백을 떼고 mm만 읽는다.
        assert_eq!(focal_folder("35 mm").as_deref(), Some("35mm"));
        assert_eq!(focal_folder("35.0 mm").as_deref(), Some("35mm"));
        assert_eq!(focal_folder(" 16 mm ").as_deref(), Some("16mm"));
        assert_eq!(focal_folder("200mm").as_deref(), Some("200mm"));
        // 소수 초점거리는 1mm 단위로 반올림(#92: 폴더는 1mm 단위).
        assert_eq!(focal_folder("10.5 mm").as_deref(), Some("11mm"));
        assert_eq!(focal_folder("34.9 mm").as_deref(), Some("35mm"));
    }

    #[test]
    fn focal_folder_rejects_unusable_values() {
        // 숫자를 못 읽거나 0 이하면 미상 폴더로 보내야 하므로 None.
        assert_eq!(focal_folder(""), None);
        assert_eq!(focal_folder("unknown"), None);
        assert_eq!(focal_folder("0 mm"), None);
        assert_eq!(focal_folder("."), None);
    }
}
