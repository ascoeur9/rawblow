//! 스캔 제외: macOS `._` 메타데이터 파일, 휴지통·시스템 폴더(`$RECYCLE.BIN`·`.Trashes`·`System Volume Information`).

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

fn g(list: &[&[&str]]) -> Vec<Vec<String>> {
    let mut out: Vec<Vec<String>> = list.iter().map(|ms| ms.iter().map(|s| s.to_string()).collect()).collect();
    out.sort();
    out
}

#[test]
fn appledouble_files_skipped_and_real_files_still_pair() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    tree(
        root,
        &[
            "DSC_0001.NEF",
            "DSC_0001.JPG",
            "._DSC_0001.NEF",
            "._DSC_0001.JPG",
            "sub/IMG_0002.CR3",
            "sub/IMG_0002.JPG",
            "sub/._IMG_0002.CR3",
            "sub/deep/._IMG_0003.JPG",
            // `._`로 시작하지 않으면 그대로 목록에 둔다.
            "sub/a._b.JPG",
        ],
    );
    let got = groups(root, &scan::scan_folder(root, true, SortOrder::Name));
    assert_eq!(
        got,
        g(&[
            &["DSC_0001.JPG", "DSC_0001.NEF"],
            &["sub/IMG_0002.CR3", "sub/IMG_0002.JPG"],
            &["sub/a._b.JPG"],
        ])
    );
}

#[test]
fn appledouble_files_skipped_without_subfolders() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    tree(root, &["P1000001.RW2", "P1000001.JPG", "._P1000001.RW2", "._P1000002.HEIC", "sub/P1000003.JPG"]);
    let got = groups(root, &scan::scan_folder(root, false, SortOrder::Name));
    assert_eq!(got, g(&[&["P1000001.JPG", "P1000001.RW2"]]));
}

#[test]
fn recycle_and_system_dirs_skipped_at_any_depth() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    tree(
        root,
        &[
            "$RECYCLE.BIN/S-1-5-21/$RABC123.NEF",
            ".Trashes/501/DSC_0009.JPG",
            "System Volume Information/X.JPG",
            "a/$Recycle.Bin/S-1-5-21/$RDEF456.JPG",
            "a/b/.TRASHES/W.NEF",
            "a/b/c/system volume information/V.jpg",
            "a/b/keep.NEF",
            // 목록에 없는 이름은 그대로(비슷한 이름·숨김·점 폴더 포함).
            "RECYCLE.BIN/r.JPG",
            "Trashes/t.JPG",
            ".hidden/h.JPG",
            ".rawblow/k.JPG",
        ],
    );
    let got = groups(root, &scan::scan_folder(root, true, SortOrder::Name));
    assert_eq!(
        got,
        g(&[&[".hidden/h.JPG"], &[".rawblow/k.JPG"], &["RECYCLE.BIN/r.JPG"], &["Trashes/t.JPG"], &["a/b/keep.NEF"]])
    );
}

#[test]
fn opening_excluded_dir_directly_lists_its_photos() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    tree(
        root,
        &[
            "$Recycle.Bin/S-1-5-21/$R1.NEF",
            "$Recycle.Bin/$R2.JPG",
            "$Recycle.Bin/._$R2.JPG",
            ".Trashes/501/T.JPG",
            "System Volume Information/S.HEIC",
        ],
    );
    let bin = root.join("$Recycle.Bin");
    assert_eq!(groups(&bin, &scan::scan_folder(&bin, true, SortOrder::Name)), g(&[&["$R2.JPG"], &["S-1-5-21/$R1.NEF"]]));
    assert_eq!(groups(&bin, &scan::scan_folder(&bin, false, SortOrder::Name)), g(&[&["$R2.JPG"]]));
    let trash = root.join(".Trashes");
    assert_eq!(groups(&trash, &scan::scan_folder(&trash, true, SortOrder::Name)), g(&[&["501/T.JPG"]]));
    let svi = root.join("System Volume Information");
    assert_eq!(groups(&svi, &scan::scan_folder(&svi, false, SortOrder::Name)), g(&[&["S.HEIC"]]));
}
