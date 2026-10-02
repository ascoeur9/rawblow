//! 폴더 분리 페어링 규칙: 다른 폴더의 같은 번호는 짝이 하나로 분명할 때만 한 항목.

use rawblow_core::model::Entry;
use rawblow_core::{scan, SortOrder};
use std::path::Path;

fn tree(root: &Path, files: &[&str]) {
    for rel in files {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, rel.as_bytes()).unwrap();
    }
}

/// 항목별 멤버(루트 기준 상대 경로, `/` 구분) 정렬 목록.
fn groups(root: &Path, entries: &[Entry]) -> Vec<Vec<String>> {
    let mut out: Vec<Vec<String>> = entries
        .iter()
        .map(|e| {
            let mut ms: Vec<String> = e
                .members
                .iter()
                .map(|m| m.strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/"))
                .collect();
            ms.sort();
            ms
        })
        .collect();
    out.sort();
    out
}

fn scan_groups(files: &[&str]) -> Vec<Vec<String>> {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    tree(root, files);
    groups(root, &scan::scan_folder(root, true, SortOrder::Name))
}

fn g(list: &[&[&str]]) -> Vec<Vec<String>> {
    let mut out: Vec<Vec<String>> = list.iter().map(|ms| ms.iter().map(|s| s.to_string()).collect()).collect();
    out.sort();
    out
}

#[test]
fn split_one_raw_folder_one_jpg_folder_merges() {
    // jpg/ · 원본/ 1:1 — 폴더 이름과 무관하게 한 항목.
    let got = scan_groups(&["jpg/DAZ_0004.JPG", "원본/DAZ_0004.NEF", "jpg/DAZ_0005.JPG", "원본/DAZ_0005.NEF"]);
    assert_eq!(
        got,
        g(&[&["jpg/DAZ_0004.JPG", "원본/DAZ_0004.NEF"], &["jpg/DAZ_0005.JPG", "원본/DAZ_0005.NEF"]])
    );
}

#[test]
fn split_raw_jpg_heic_folders_merge_into_one() {
    // RAW/ · JPG/ · HEIC/(#97): 이미지 종류가 겹치지 않으면 셋이 한 항목.
    let got = scan_groups(&["RAW/P1000001.NEF", "JPG/P1000001.JPG", "HEIC/P1000001.HEIC"]);
    assert_eq!(got, g(&[&["HEIC/P1000001.HEIC", "JPG/P1000001.JPG", "RAW/P1000001.NEF"]]));
}

#[test]
fn split_one_raw_with_jpg_and_heic_in_one_image_folder_merges() {
    // 한 이미지 폴더에 JPG·HEIC가 함께 있어도 종류가 하나씩이면 짝이 분명하다.
    let got = scan_groups(&["RAW/IMG_1.CR3", "out/IMG_1.JPG", "out/IMG_1.HEIC"]);
    assert_eq!(got, g(&[&["RAW/IMG_1.CR3", "out/IMG_1.HEIC", "out/IMG_1.JPG"]]));
}

#[test]
fn two_days_same_number_stay_four_items() {
    // day1·day2가 같은 번호를 쓰면 RAW만 폴더가 둘 → 어느 짝인지 모르므로 하나도 합치지 않는다.
    let got = scan_groups(&[
        "day1/RAW/IMG_0001.CR2",
        "day1/JPG/IMG_0001.JPG",
        "day2/RAW/IMG_0001.CR2",
        "day2/JPG/IMG_0001.JPG",
    ]);
    assert_eq!(
        got,
        g(&[
            &["day1/JPG/IMG_0001.JPG"],
            &["day1/RAW/IMG_0001.CR2"],
            &["day2/JPG/IMG_0001.JPG"],
            &["day2/RAW/IMG_0001.CR2"],
        ])
    );
}

#[test]
fn two_jpg_folders_one_raw_folder_stay_separate() {
    // 같은 종류(JPG) 이미지 폴더가 둘이면(카메라 JPG + 보정본 등) 짝이 모호 → 3항목.
    let got = scan_groups(&["jpg/DAZ_0004.JPG", "보정/DAZ_0004.jpg", "원본/DAZ_0004.NEF"]);
    assert_eq!(got, g(&[&["jpg/DAZ_0004.JPG"], &["보정/DAZ_0004.jpg"], &["원본/DAZ_0004.NEF"]]));
}

#[test]
fn jpg_and_jpeg_count_as_one_family() {
    // .jpg와 .jpeg는 같은 종류 — 두 폴더에 나뉘면 모호.
    let got = scan_groups(&["a/X.JPG", "b/X.jpeg", "raw/X.NEF"]);
    assert_eq!(got.len(), 3, "{got:?}");
}

#[test]
fn complete_pair_never_merges_with_other_folders() {
    // A에 이미 RAW+JPG 짝이 있으면 B의 JPG만, C의 RAW만과 합치지 않는다.
    let got = scan_groups(&["A/IMG_7.NEF", "A/IMG_7.JPG", "B/IMG_7.JPG"]);
    assert_eq!(got, g(&[&["A/IMG_7.JPG", "A/IMG_7.NEF"], &["B/IMG_7.JPG"]]));
    let got = scan_groups(&["A/IMG_7.NEF", "A/IMG_7.JPG", "B/IMG_7.JPG", "C/IMG_7.NEF"]);
    assert_eq!(got, g(&[&["A/IMG_7.JPG", "A/IMG_7.NEF"], &["B/IMG_7.JPG"], &["C/IMG_7.NEF"]]));
}

#[test]
fn two_raw_folders_one_jpg_folder_stay_separate() {
    let got = scan_groups(&["raw1/DSC_0001.NEF", "raw2/DSC_0001.NEF", "jpg/DSC_0001.JPG"]);
    assert_eq!(got, g(&[&["jpg/DSC_0001.JPG"], &["raw1/DSC_0001.NEF"], &["raw2/DSC_0001.NEF"]]));
}

#[test]
fn raw_only_folders_never_merge_with_each_other() {
    let got = scan_groups(&["P1000003.RW2", "새 폴더/P1000003.RW2"]);
    assert_eq!(got.len(), 2, "양쪽 다 RAW면 별개 항목");
}

#[test]
fn split_pair_stem_is_case_insensitive() {
    let got = scan_groups(&["RAW/dsc_0009.nef", "JPG/DSC_0009.JPG"]);
    assert_eq!(got, g(&[&["JPG/DSC_0009.JPG", "RAW/dsc_0009.nef"]]));
}

#[test]
fn ambiguous_stem_does_not_affect_other_stems() {
    // 모호한 번호만 따로 남고, 같은 폴더의 다른 번호는 그대로 1:1 병합.
    let got = scan_groups(&["raw/A.NEF", "raw/B.NEF", "jpg/A.JPG", "jpg/B.JPG", "edit/B.JPG"]);
    assert_eq!(
        got,
        g(&[&["edit/B.JPG"], &["jpg/A.JPG", "raw/A.NEF"], &["jpg/B.JPG"], &["raw/B.NEF"]])
    );
}
