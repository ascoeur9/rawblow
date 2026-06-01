//! rawblow-core 통합 테스트. 실제 sample 데이터(RW2/JPG)를 사용한다.

use rawblow_core::model::{Entry, Kind, Label, MatchMode};
use rawblow_core::{decode, scan, sidecar, transfer};
use std::path::{Path, PathBuf};

fn sample_dir() -> PathBuf {
    // 워크스페이스 루트의 sample/ (CARGO_MANIFEST_DIR = crates/rawblow-core)
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../sample")
        .canonicalize()
        .expect("sample dir must exist")
}

#[test]
fn natural_sort_orders_numbers_correctly() {
    use std::cmp::Ordering;
    assert_eq!(scan::natural_cmp("IMG_2", "IMG_10"), Ordering::Less);
    assert_eq!(scan::natural_cmp("P1063603", "P1063700"), Ordering::Less);
    assert_eq!(scan::natural_cmp("a10", "a9"), Ordering::Greater);
    assert_eq!(scan::natural_cmp("file", "file"), Ordering::Equal);
}

#[test]
fn scan_groups_and_sorts_sample() {
    let dir = sample_dir();
    let entries = scan::scan_folder(&dir, false, rawblow_core::SortOrder::Name);
    assert!(!entries.is_empty(), "sample 루트에 항목이 있어야 함");
    // 자연 정렬 단조성 확인.
    for w in entries.windows(2) {
        let a = w[0].display.file_name().unwrap().to_string_lossy().to_string();
        let b = w[1].display.file_name().unwrap().to_string_lossy().to_string();
        assert_ne!(scan::natural_cmp(&a, &b), std::cmp::Ordering::Greater);
    }
}

#[test]
fn recursive_scan_includes_subfolder() {
    let dir = sample_dir();
    let flat = scan::scan_folder(&dir, false, rawblow_core::SortOrder::Name).len();
    let deep = scan::scan_folder(&dir, true, rawblow_core::SortOrder::Name).len();
    assert!(deep > flat, "재귀 스캔이 하위 폴더(홍대 모임)를 포함해야 함");
}

#[test]
fn pairing_prefers_image_and_flags_raw_badge() {
    // 합성 멤버로 페어링 규칙 검증(파일시스템 무관).
    let e = Entry::from_members(
        "IMG_0001".into(),
        vec![PathBuf::from("IMG_0001.RW2"), PathBuf::from("IMG_0001.JPG")],
    );
    assert_eq!(e.display.extension().unwrap(), "JPG", "이미지를 우선 표시");
    assert!(e.has_raw && e.has_image);
    assert!(e.shows_raw_badge(), "RAW+JPG면 RAW+ 배지");

    let raw_only = Entry::from_members("IMG_0002".into(), vec![PathBuf::from("IMG_0002.RW2")]);
    assert_eq!(raw_only.display.extension().unwrap(), "RW2");
    assert!(!raw_only.shows_raw_badge());
}

#[test]
fn extract_embedded_jpeg_from_real_rw2() {
    let dir = sample_dir();
    // 루트의 RW2 하나를 찾는다.
    let rw2 = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .find(|p| p.extension().and_then(|s| s.to_str()).map(|s| s.eq_ignore_ascii_case("rw2")).unwrap_or(false))
        .expect("샘플에 RW2가 있어야 함");

    let bytes = std::fs::read(&rw2).unwrap();
    let jpeg = decode::extract_embedded_jpeg(&bytes).expect("RW2에서 임베디드 JPEG 추출");
    assert_eq!(&jpeg[0..3], &[0xFF, 0xD8, 0xFF], "JPEG SOI로 시작");

    // 실제로 디코딩되어 합리적 해상도가 나오는지.
    let img = decode::decode_file(&rw2, decode::DecodeOptions::default()).expect("RW2 디코딩");
    assert!(img.width >= 1920 && img.height >= 1080, "프리뷰가 충분히 큼: {}x{}", img.width, img.height);
    assert_eq!(img.rgba.len(), (img.width * img.height * 4) as usize);
}

#[test]
fn decode_real_jpg() {
    let dir = sample_dir();
    let jpg = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .find(|p| p.extension().and_then(|s| s.to_str()).map(|s| s.eq_ignore_ascii_case("jpg")).unwrap_or(false))
        .expect("샘플에 JPG가 있어야 함");
    let img = decode::decode_file(&jpg, decode::DecodeOptions::default()).expect("JPG 디코딩");
    assert!(img.width > 0 && img.height > 0);
}

#[test]
fn sidecar_roundtrip_restores_labels() {
    let tmp = tempfile::tempdir().unwrap();
    let folder = tmp.path();
    // 가짜 파일 2개 생성(스캔 가능하도록 빈 파일).
    std::fs::write(folder.join("IMG_1.JPG"), b"x").unwrap();
    std::fs::write(folder.join("IMG_2.JPG"), b"x").unwrap();

    let mut entries = scan::scan_folder(folder, false, rawblow_core::SortOrder::Name);
    assert_eq!(entries.len(), 2);
    entries[0].label = Label::Pick;
    entries[1].label = Label::Reject;

    sidecar::save(folder, &entries).unwrap();
    assert!(sidecar::sidecar_path(folder).exists());
    assert!(sidecar::sidecar_txt_path(folder).exists());

    // 라벨 리셋 후 복원.
    let mut reloaded = scan::scan_folder(folder, false, rawblow_core::SortOrder::Name);
    let session = sidecar::load(folder).expect("사이드카 로드");
    sidecar::apply(&session, &mut reloaded);

    let pick = reloaded.iter().find(|e| e.stem.eq_ignore_ascii_case("IMG_1")).unwrap();
    let rej = reloaded.iter().find(|e| e.stem.eq_ignore_ascii_case("IMG_2")).unwrap();
    assert_eq!(pick.label, Label::Pick);
    assert_eq!(rej.label, Label::Reject);
}

#[test]
fn transfer_copies_selected_with_conflict_rename() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    let dst = tmp.path().join("dst");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::create_dir_all(&dst).unwrap();
    std::fs::write(src.join("IMG_1.JPG"), b"aaa").unwrap();
    std::fs::write(src.join("IMG_1.RW2"), b"bbbbb").unwrap();
    // 대상에 동일 파일명 선점 → 충돌 유도.
    std::fs::write(dst.join("IMG_1.JPG"), b"existing").unwrap();

    let mut entries = scan::scan_folder(&src, false, rawblow_core::SortOrder::Name);
    assert_eq!(entries.len(), 1);
    entries[0].label = Label::Pick;

    let req = transfer::TransferRequest {
        entries: &entries,
        labels: vec![Label::Pick],
        stars: vec![],
        action: transfer::Action::Copy,
        companions: transfer::Companions::Both,
        dest: dst.clone(),
        split_by_label: false,
        conflict: transfer::ConflictPolicy::AutoIncrement,
    };
    let report = transfer::execute(&req);
    assert_eq!(report.transferred, 2, "JPG+RW2 둘 다 전송");
    assert_eq!(report.raw_count, 1);
    assert_eq!(report.image_count, 1);
    assert_eq!(report.renamed.len(), 1, "JPG 충돌로 1건 리네임");
    assert!(dst.join("IMG_1_001.JPG").exists(), "충돌 파일은 일련번호로");
    assert!(dst.join("IMG_1.RW2").exists());
    // 원본 보존(copy).
    assert!(src.join("IMG_1.JPG").exists());
}

#[test]
fn transfer_split_by_label_subfolders() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    let dst = tmp.path().join("dst");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(src.join("A.JPG"), b"a").unwrap();
    std::fs::write(src.join("B.JPG"), b"b").unwrap();

    let mut entries = scan::scan_folder(&src, false, rawblow_core::SortOrder::Name);
    entries[0].label = Label::Pick;
    entries[1].label = Label::Hold;

    let req = transfer::TransferRequest {
        entries: &entries,
        labels: vec![Label::Pick, Label::Hold],
        stars: vec![],
        action: transfer::Action::Copy,
        companions: transfer::Companions::Both,
        dest: dst.clone(),
        split_by_label: true,
        conflict: transfer::ConflictPolicy::AutoIncrement,
    };
    let report = transfer::execute(&req);
    assert_eq!(report.transferred, 2);
    assert!(dst.join("pick").exists() || dst.join("hold").exists());
    // A는 pick, B는 hold 폴더로.
    let a_in_pick = dst.join("pick").join("A.JPG").exists();
    let b_in_hold = dst.join("hold").join("B.JPG").exists();
    assert!(a_in_pick && b_in_hold, "라벨별 하위폴더 분기");
}

#[test]
fn rawpull_jump_matching() {
    let entries = vec![
        Entry::from_members("P1063603".into(), vec![PathBuf::from("P1063603.RW2")]),
        Entry::from_members("P1063700".into(), vec![PathBuf::from("P1063700.RW2")]),
        Entry::from_members("IMG_0123".into(), vec![PathBuf::from("IMG_0123.JPG")]),
    ];
    // 줄바꿈·쉼표·탭 혼합 입력.
    let terms = transfer::parse_terms("P1063603, 0123\tP1063700\n");
    assert_eq!(terms.len(), 3);

    let exact = transfer::match_indices(&entries, &["P1063603".into()], MatchMode::Exact);
    assert_eq!(exact, vec![0]);

    let contains = transfer::match_indices(&entries, &["0123".into()], MatchMode::Contains);
    assert_eq!(contains, vec![2], "contains 매칭");
}

#[test]
fn transfer_by_stars_union_with_labels() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    let dst = tmp.path().join("dst");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(src.join("A.JPG"), b"a").unwrap(); // pick 라벨
    std::fs::write(src.join("B.JPG"), b"b").unwrap(); // 무라벨 + 5★
    std::fs::write(src.join("C.JPG"), b"c").unwrap(); // 무라벨 + 2★ (대상 아님)

    let mut entries = scan::scan_folder(&src, false, rawblow_core::SortOrder::Name);
    for e in entries.iter_mut() {
        match e.stem.as_str() {
            "A" => e.label = Label::Pick,
            "B" => e.stars = 5,
            "C" => e.stars = 2,
            _ => {}
        }
    }

    // 라벨=Pick OR 별점∈{5} → A(라벨), B(별점)만. C(2★)는 제외.
    let req = transfer::TransferRequest {
        entries: &entries,
        labels: vec![Label::Pick],
        stars: vec![5],
        action: transfer::Action::Copy,
        companions: transfer::Companions::Both,
        dest: dst.clone(),
        split_by_label: false,
        conflict: transfer::ConflictPolicy::AutoIncrement,
    };
    let report = transfer::execute(&req);
    assert_eq!(report.transferred, 2, "Pick(A) + 5★(B)만 전송");
    assert!(dst.join("A.JPG").exists());
    assert!(dst.join("B.JPG").exists());
    assert!(!dst.join("C.JPG").exists(), "2★는 대상 아님");
}

#[test]
fn transfer_star_only_split_goes_to_star_folder_not_unrated() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    let dst = tmp.path().join("dst");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(src.join("A.JPG"), b"a").unwrap(); // Pick 라벨
    std::fs::write(src.join("B.JPG"), b"b").unwrap(); // 무라벨 + 5★

    let mut entries = scan::scan_folder(&src, false, rawblow_core::SortOrder::Name);
    for e in entries.iter_mut() {
        match e.stem.as_str() {
            "A" => e.label = Label::Pick,
            "B" => e.stars = 5,
            _ => {}
        }
    }
    let req = transfer::TransferRequest {
        entries: &entries,
        labels: vec![Label::Pick],
        stars: vec![5],
        action: transfer::Action::Copy,
        companions: transfer::Companions::Both,
        dest: dst.clone(),
        split_by_label: true,
        conflict: transfer::ConflictPolicy::AutoIncrement,
    };
    let report = transfer::execute(&req);
    assert_eq!(report.transferred, 2);
    assert!(dst.join("pick").join("A.JPG").exists(), "라벨 항목은 pick 폴더로");
    assert!(dst.join("5star").join("B.JPG").exists(), "별점-only 항목은 5star 폴더로");
    assert!(!dst.join("unrated").join("B.JPG").exists(), "unrated 폴더로 가면 안 됨");
}

#[test]
fn sidecar_roundtrip_restores_stars() {
    let tmp = tempfile::tempdir().unwrap();
    let folder = tmp.path();
    std::fs::write(folder.join("IMG_1.JPG"), b"x").unwrap();
    std::fs::write(folder.join("IMG_2.JPG"), b"x").unwrap();

    let mut entries = scan::scan_folder(folder, false, rawblow_core::SortOrder::Name);
    // 무라벨 + 별점만 — 별점 단독으로도 보존돼야 한다.
    entries[0].stars = 4;
    entries[1].label = Label::Pick;
    entries[1].stars = 5;

    sidecar::save(folder, &entries).unwrap();
    let mut reloaded = scan::scan_folder(folder, false, rawblow_core::SortOrder::Name);
    let session = sidecar::load(folder).expect("사이드카 로드");
    sidecar::apply(&session, &mut reloaded);

    let s1 = reloaded.iter().find(|e| e.stem.eq_ignore_ascii_case("IMG_1")).unwrap();
    let s2 = reloaded.iter().find(|e| e.stem.eq_ignore_ascii_case("IMG_2")).unwrap();
    assert_eq!(s1.stars, 4, "무라벨이어도 별점 보존");
    assert_eq!(s1.label, Label::Unrated);
    assert_eq!(s2.stars, 5);
    assert_eq!(s2.label, Label::Pick);
}

#[test]
fn thumb_cache_roundtrip() {
    use rawblow_core::cache;
    use rawblow_core::decode::DecodedImage;
    let tmp = tempfile::tempdir().unwrap();
    let cdir = tmp.path().join("cache");
    // 2x2 빨강 이미지.
    let img = DecodedImage {
        width: 2,
        height: 2,
        rgba: vec![255, 0, 0, 255, 255, 0, 0, 255, 255, 0, 0, 255, 255, 0, 0, 255],
        color_managed: true,
        full_raw: false,
    };
    let key = "deadbeef";
    assert!(cache::load(&cdir, key).is_none(), "처음엔 미스");
    cache::store(&cdir, key, &img);
    let got = cache::load(&cdir, key).expect("저장 후 히트");
    assert_eq!((got.width, got.height), (2, 2));
    assert!(cache::dir_size(&cdir) > 0);
    cache::clear(&cdir).unwrap();
    assert_eq!(cache::dir_size(&cdir), 0, "비우면 0");
    assert!(cache::load(&cdir, key).is_none(), "비운 뒤 미스");
}

#[test]
fn thumb_cache_trim_enforces_limit() {
    use rawblow_core::cache;
    use rawblow_core::decode::DecodedImage;
    let tmp = tempfile::tempdir().unwrap();
    let cdir = tmp.path().join("cache");
    let img = DecodedImage {
        width: 8,
        height: 8,
        rgba: vec![128u8; 8 * 8 * 4],
        color_managed: true,
        full_raw: false,
    };
    for i in 0..6 {
        cache::store(&cdir, &format!("key{i:02}"), &img);
    }
    let before = cache::dir_size(&cdir);
    assert!(before > 0);
    let max = before / 2;
    cache::trim(&cdir, max);
    let after = cache::dir_size(&cdir);
    assert!(after <= max, "정리 후 상한 이하여야 함: {after} <= {max}");
    assert!(after < before, "일부는 삭제돼야 함");
    // 0(무제한)이면 아무 것도 안 지운다.
    cache::trim(&cdir, 0);
    assert_eq!(cache::dir_size(&cdir), after, "무제한은 그대로 유지");
}

#[test]
fn kind_classification() {
    assert_eq!(rawblow_core::model::kind_of(Path::new("a.RW2")), Some(Kind::Raw));
    assert_eq!(rawblow_core::model::kind_of(Path::new("a.jpg")), Some(Kind::Image));
    assert_eq!(rawblow_core::model::kind_of(Path::new("a.txt")), None);
}
