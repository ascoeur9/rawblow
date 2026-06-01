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

/// DCT 축소 디코딩(jpeg-decoder `scale`)이 **정확한 크기·종횡비·방향**으로, 손상(회색/균일/
/// 전치) 없이 동작하는지 합성 그라디언트 JPEG로 검증한다(#1 성능 개선 회귀 가드).
/// 샘플 사진이 없어도 결정적으로 돌아간다.
#[test]
fn dct_scaled_decode_is_correct_size_and_not_corrupt() {
    use image::{ImageBuffer, Rgb};
    // 좌→우로 빨강이, 위→아래로 초록이 증가하는 1600×1000 그라디언트(EXIF 없음 → 무회전).
    let (w, h) = (1600u32, 1000u32);
    let buf = ImageBuffer::from_fn(w, h, |x, y| {
        let r = (x * 255 / (w - 1)) as u8;
        let g = (y * 255 / (h - 1)) as u8;
        Rgb([r, g, 64])
    });
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("grad.jpg");
    buf.save_with_format(&path, image::ImageFormat::Jpeg).unwrap();

    // 썸네일(320: 1/4 DCT 경유), 중간(1200: 풀 DCT 후 축소), 원본변(1600)을 각각 검증.
    for edge in [320u32, 1200, 1600] {
        let d = decode::decode_file(&path, decode::DecodeOptions { full_raw: false, max_edge: Some(edge) })
            .expect("그라디언트 JPEG 디코딩");
        let long = d.width.max(d.height);
        assert_eq!(long, edge.min(w), "긴 변이 정확히 max_edge로 축소되어야: edge={edge} got {}x{}", d.width, d.height);
        let ratio = d.width as f32 / d.height as f32;
        assert!((ratio - 1.6).abs() < 0.05, "종횡비(1.6) 유지: edge={edge} ratio={ratio}");
        assert_eq!(d.rgba.len(), (d.width * d.height * 4) as usize, "RGBA 버퍼 크기 일치");
        // 그라디언트 방향 보존(회색·균일·전치 아님). JPEG 손실 감안 +40 여유.
        let at = |px: u32, py: u32, ch: usize| -> i32 {
            d.rgba[(((py * d.width + px) * 4) as usize) + ch] as i32
        };
        let (rx, by) = (d.width - 1, d.height - 1);
        assert!(at(0, 0, 0) + 40 < at(rx, 0, 0), "빨강 좌→우 증가 보존: edge={edge}");
        assert!(at(0, 0, 1) + 40 < at(0, by, 1), "초록 위→아래 증가 보존: edge={edge}");
    }

    // max_edge=None: 원본 크기 그대로(축소·DCT scale 모두 스킵).
    let full = decode::decode_file(&path, decode::DecodeOptions { full_raw: false, max_edge: None })
        .expect("원본 디코딩");
    assert_eq!((full.width, full.height), (w, h), "None이면 원본 크기 유지");
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
fn star_filter_exact_and_any() {
    use rawblow_core::StarFilter;
    // Any는 별점 무시.
    for s in 0..=5u8 {
        assert!(StarFilter::Any.accepts(s));
    }
    // Exact(n)은 정확히 n점만(0=미부여 포함).
    assert!(StarFilter::Exact(3).accepts(3));
    assert!(!StarFilter::Exact(3).accepts(2));
    assert!(!StarFilter::Exact(3).accepts(4));
    assert!(StarFilter::Exact(0).accepts(0));
    assert!(!StarFilter::Exact(0).accepts(1));
}

#[test]
fn label_and_star_filters_are_independent() {
    use rawblow_core::{Filter, StarFilter};
    // 라벨=Pick AND 별점=정확히5 인 항목만 통과해야 한다(AND 결합, filtered()와 동일 규칙).
    let items: [(Label, u8); 4] = [
        (Label::Pick, 5),
        (Label::Pick, 3),
        (Label::Hold, 5),
        (Label::Unrated, 0),
    ];
    let (f, sf) = (Filter::Pick, StarFilter::Exact(5));
    let pass: Vec<usize> = items
        .iter()
        .enumerate()
        .filter(|(_, (l, s))| f.accepts(*l) && sf.accepts(*s))
        .map(|(i, _)| i)
        .collect();
    assert_eq!(pass, vec![0], "Pick AND ★5는 첫 항목만 통과");
}

#[test]
fn kind_classification() {
    assert_eq!(rawblow_core::model::kind_of(Path::new("a.RW2")), Some(Kind::Raw));
    assert_eq!(rawblow_core::model::kind_of(Path::new("a.jpg")), Some(Kind::Image));
    assert_eq!(rawblow_core::model::kind_of(Path::new("a.txt")), None);
}
