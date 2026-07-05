//! 폴더 스캔, stem 페어링, 자연 정렬 (F3).

use crate::model::{is_supported, kind_of, Entry, Kind, SortOrder};
use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

/// 폴더를 스캔해 동일 stem을 묶은 Entry 목록을 정렬해 반환한다.
///
/// 페어링 키는 `(부모 디렉토리, 소문자 stem)` — 다른 하위 폴더의 동일 번호는
/// 원칙적으로 별개 항목이다. RAW+JPG/HEIC가 같은 폴더·같은 stem이면 한 항목.
///
/// 예외(폴더 분리 동반 페어): 같은 stem이 **정확히 두 폴더**에 있고 한쪽은 RAW만,
/// 다른쪽은 이미지만일 때는 한 항목으로 합친다 — 카메라/사용자가 RAW와 JPG를
/// `루트 + "새 폴더"`, `RAW/ + JPG/`처럼 나눠 둔 흔한 구조. 세 폴더 이상이거나
/// 양쪽 종류가 섞이면 모호하므로 합치지 않는다(서로 다른 촬영의 번호 충돌 보호).
pub fn scan_folder(folder: &Path, recursive: bool, sort: SortOrder) -> Vec<Entry> {
    let max_depth = if recursive { usize::MAX } else { 1 };
    let mut groups: BTreeMap<(PathBuf, String), Vec<PathBuf>> = BTreeMap::new();

    for dent in WalkDir::new(folder)
        .min_depth(1)
        .max_depth(max_depth)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = dent.path();
        if dent.file_type().is_file() && is_supported(path) {
            let parent = path
                .parent()
                .map(|p| p.to_path_buf())
                .unwrap_or_default();
            let stem = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_string();
            let key = (parent, stem.to_ascii_lowercase());
            groups.entry(key).or_default().push(path.to_path_buf());
        }
    }

    // 폴더 분리 동반 페어 병합: stem 기준으로 재묶고, RAW만/이미지만 1:1이면 합친다.
    let mut by_stem: BTreeMap<String, Vec<Vec<PathBuf>>> = BTreeMap::new();
    for ((_, stem_l), members) in groups {
        by_stem.entry(stem_l).or_default().push(members);
    }
    let mut merged: Vec<Vec<PathBuf>> = Vec::new();
    for (_, mut sets) in by_stem {
        let only = |ms: &[PathBuf], k: Kind| ms.iter().all(|p| kind_of(p) == Some(k));
        if sets.len() == 2
            && ((only(&sets[0], Kind::Raw) && only(&sets[1], Kind::Image))
                || (only(&sets[0], Kind::Image) && only(&sets[1], Kind::Raw)))
        {
            let mut all = sets.pop().unwrap();
            all.extend(sets.pop().unwrap());
            merged.push(all);
        } else {
            merged.append(&mut sets);
        }
    }

    let mut entries: Vec<Entry> = merged
        .into_iter()
        .map(|members| {
            // 표시용 stem은 첫 멤버의 실제(원본 대소문자) stem을 사용.
            let stem = members[0]
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_string();
            Entry::from_members(stem, members)
        })
        .collect();

    sort_entries(&mut entries, sort);
    entries
}

/// 주어진 기준으로 항목을 정렬한다.
pub fn sort_entries(entries: &mut [Entry], sort: SortOrder) {
    match sort {
        SortOrder::Name | SortOrder::CaptureTime => {
            // CaptureTime은 스캔 단계에서 EXIF를 읽지 않으므로 파일명 자연정렬로 폴백한다.
            // (촬영시각 정렬은 EXIF 로드 이후 GUI에서 재정렬 가능.)
            entries.sort_by(|a, b| natural_cmp(&display_name(a), &display_name(b)));
        }
        SortOrder::Modified => {
            entries.sort_by(|a, b| {
                let ma = mtime(&a.display);
                let mb = mtime(&b.display);
                mb.cmp(&ma) // 최신 우선
                    .then_with(|| natural_cmp(&display_name(a), &display_name(b)))
            });
        }
    }
}

/// 촬영시각순 정렬 순서(#56): `keys[i]` = (촬영시각 에포크 초, 파일명)인 목록의 정렬 순열을
/// 돌려준다. 시각 오름차순, 동시각은 파일명 자연정렬로 안정화. 촬영시각이 없는 항목
/// (EXIF 미탑재 JPG 등)은 맨 뒤로 가되 그들끼리는 파일명 자연정렬을 유지한다.
/// EXIF는 스캔 시점에 없으므로 GUI가 백그라운드 로드 후 이 함수로 재정렬한다.
pub fn capture_order(keys: &[(Option<i64>, String)]) -> Vec<usize> {
    let mut idx: Vec<usize> = (0..keys.len()).collect();
    idx.sort_by(|&a, &b| match (keys[a].0, keys[b].0) {
        (Some(x), Some(y)) => x.cmp(&y).then_with(|| natural_cmp(&keys[a].1, &keys[b].1)),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => natural_cmp(&keys[a].1, &keys[b].1),
    });
    idx
}

fn display_name(e: &Entry) -> String {
    e.display
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_string()
}

fn mtime(path: &Path) -> std::time::SystemTime {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .unwrap_or(std::time::UNIX_EPOCH)
}

/// 자연 정렬 비교: 숫자 구간은 수치로, 그 외는 대소문자 무시 사전식으로 비교.
/// 예) `IMG_2 < IMG_10`, `P1063603 < P1063700`.
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let mut ai = a.chars().peekable();
    let mut bi = b.chars().peekable();
    loop {
        match (ai.peek().copied(), bi.peek().copied()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(ca), Some(cb)) => {
                if ca.is_ascii_digit() && cb.is_ascii_digit() {
                    let na = take_digits(&mut ai);
                    let nb = take_digits(&mut bi);
                    // 앞의 0을 제거한 유효숫자 비교 → 길이 → 원문(0 패딩 차이).
                    let ta = na.trim_start_matches('0');
                    let tb = nb.trim_start_matches('0');
                    let ord = ta.len().cmp(&tb.len()).then_with(|| ta.cmp(tb));
                    if ord != Ordering::Equal {
                        return ord;
                    }
                    // 수치가 같으면 0 패딩이 적은 쪽(짧은 원문)을 앞에.
                    let pad = na.len().cmp(&nb.len());
                    if pad != Ordering::Equal {
                        return pad;
                    }
                } else {
                    let la = ca.to_ascii_lowercase();
                    let lb = cb.to_ascii_lowercase();
                    if la != lb {
                        return la.cmp(&lb);
                    }
                    ai.next();
                    bi.next();
                }
            }
        }
    }
}

fn take_digits(it: &mut std::iter::Peekable<std::str::Chars>) -> String {
    let mut s = String::new();
    while let Some(c) = it.peek().copied() {
        if c.is_ascii_digit() {
            s.push(c);
            it.next();
        } else {
            break;
        }
    }
    s
}
