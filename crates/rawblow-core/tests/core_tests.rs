//! rawblow-core 통합 테스트. 실제 sample 데이터(RW2/JPG)를 사용한다.

use rawblow_core::model::{ColorTag, Entry, Kind, Label, MatchMode};
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
fn pairing_merges_raw_and_jpg_split_across_folders() {
    // RAW는 루트, JPG는 하위 "새 폴더" — 폴더만 다른 동반 페어는 한 항목으로.
    let root = std::env::temp_dir().join("rb_split_pair_test");
    let _ = std::fs::remove_dir_all(&root);
    let sub = root.join("새 폴더");
    std::fs::create_dir_all(&sub).unwrap();
    std::fs::write(root.join("P1000001.RW2"), b"x").unwrap();
    std::fs::write(sub.join("P1000001.JPG"), b"x").unwrap();
    std::fs::write(root.join("P1000002.RW2"), b"x").unwrap(); // RAW 단독은 그대로
    std::fs::write(root.join("P1000003.RW2"), b"x").unwrap(); // 같은 stem이 양쪽 다 RAW면
    std::fs::write(sub.join("P1000003.RW2"), b"x").unwrap(); //   (다른 촬영일 수 있음) 별개 유지

    let entries = scan::scan_folder(&root, true, rawblow_core::SortOrder::Name);
    let find = |s: &str| {
        entries
            .iter()
            .filter(|e| e.stem.eq_ignore_ascii_case(s))
            .collect::<Vec<_>>()
    };
    let p1 = find("P1000001");
    assert_eq!(p1.len(), 1, "폴더 분리 RAW+JPG는 한 항목");
    assert!(p1[0].has_raw && p1[0].has_image && p1[0].shows_raw_badge());
    assert_eq!(p1[0].display.extension().unwrap(), "JPG", "이미지를 우선 표시");
    assert_eq!(find("P1000002").len(), 1);
    assert_eq!(find("P1000003").len(), 2, "양쪽 다 RAW면 별개 항목 유지");
    let _ = std::fs::remove_dir_all(&root);
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

    // 실제로 디코딩되어 합리적 해상도가 나오는지. 세로 촬영 샘플도 통과하도록
    // 장변/단변 기준(방향 무관)으로 검사한다.
    let img = decode::decode_file(&rw2, decode::DecodeOptions::default()).expect("RW2 디코딩");
    let (long, short) = (img.width.max(img.height), img.width.min(img.height));
    assert!(long >= 1920 && short >= 1080, "프리뷰가 충분히 큼: {}x{}", img.width, img.height);
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

/// ORIG(full_raw)의 IFD 기반 임베디드 읽기를 **합성 RW2**로 검증한다(#perf Cycle3 회귀 가드).
/// RW2 IFD0의 type-7 JPEG blob(tag 0x0127/0x002e)을 그 구간만 읽어 디코딩하는 경로가
/// 정상·회전·가비지스킵·EOF안전하게 동작하는지 실제 카메라 파일 없이 결정적으로 확인한다.
#[test]
fn orig_ifd_embedded_synthetic_rw2() {
    use image::{ImageBuffer, Rgb};
    fn make_jpeg(w: u32, h: u32) -> Vec<u8> {
        let buf = ImageBuffer::from_fn(w, h, |x, y| {
            Rgb([(x * 255 / (w - 1)) as u8, (y * 255 / (h - 1)) as u8, 64u8])
        });
        let mut out = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgb8(buf)
            .write_to(&mut out, image::ImageFormat::Jpeg)
            .unwrap();
        out.into_inner()
    }
    // LE RW2(매직 0x55) + IFD0: 첫 엔트리 = 큰 JPEG(tag 0x0127, type7, count=len, val=offset),
    // 이어서 extra 엔트리들. JPEG 바이트는 IFD 뒤에 덧붙인다. claim_len으로 len 과대선언(EOF 초과) 모의.
    fn build_rw2(jpeg: &[u8], extra: &[(u16, u16, u32, u32)], claim_len: Option<u32>) -> Vec<u8> {
        let entry_count = extra.len() as u16 + 1;
        let ifd_size = 2 + (entry_count as usize) * 12 + 4;
        let blob_offset = 8 + ifd_size;
        let mut f = Vec::new();
        f.extend_from_slice(b"II");
        f.extend_from_slice(&0x0055u16.to_le_bytes());
        f.extend_from_slice(&8u32.to_le_bytes());
        f.extend_from_slice(&entry_count.to_le_bytes());
        let len = claim_len.unwrap_or(jpeg.len() as u32);
        f.extend_from_slice(&0x0127u16.to_le_bytes());
        f.extend_from_slice(&7u16.to_le_bytes());
        f.extend_from_slice(&len.to_le_bytes());
        f.extend_from_slice(&(blob_offset as u32).to_le_bytes());
        for &(tag, typ, cnt, val) in extra {
            f.extend_from_slice(&tag.to_le_bytes());
            f.extend_from_slice(&typ.to_le_bytes());
            f.extend_from_slice(&cnt.to_le_bytes());
            f.extend_from_slice(&val.to_le_bytes());
        }
        f.extend_from_slice(&0u32.to_le_bytes());
        assert_eq!(f.len(), blob_offset);
        f.extend_from_slice(jpeg);
        f
    }
    fn write(name: &str, data: &[u8]) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join("rb_ifd_test");
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join(name);
        std::fs::write(&p, data).unwrap();
        p
    }
    let orig = |p: &std::path::Path| {
        decode::decode_file(p, decode::DecodeOptions { full_raw: true, max_edge: Some(8192) })
    };

    // 1) 정상: 3200px 임베디드를 IFD 구간만 읽어 원본 크기로 반환.
    let p = write("happy.rw2", &build_rw2(&make_jpeg(3200, 2000), &[], None));
    let img = orig(&p).expect("happy decode");
    assert_eq!((img.width, img.height), (3200, 2000), "IFD 임베디드 원본 크기");

    // 2) 회전(Orientation=6, rotate90): 3200×2000 가로 → 2000×3200 세로.
    let p = write("orient.rw2", &build_rw2(&make_jpeg(3200, 2000), &[(0x0112, 3, 1, 6)], None));
    let img = orig(&p).expect("orient decode");
    assert_eq!((img.width, img.height), (2000, 3200), "Orientation=6 회전 적용(이중회전 아님)");

    // 3) 잘못된 오프셋(EOF 초과): read_range가 EOF로 안전 → 패닉 없이 폴백으로 복구.
    let mut bogus = build_rw2(&make_jpeg(3200, 2000), &[], None);
    let off_pos = 8 + 2 + 2 + 2 + 4; // 첫 엔트리 value(offset) 필드
    bogus[off_pos..off_pos + 4].copy_from_slice(&0xF000_0000u32.to_le_bytes());
    let p = write("bogus.rw2", &bogus);
    let _ = orig(&p); // 크래시하지 않으면 통과(패닉 시 테스트 실패).

    // 4) 절단: count(len)을 실제보다 5MB 크게 선언 → read_range가 가능한 만큼만 읽어 정상 디코딩.
    let jpeg = make_jpeg(3200, 2000);
    let claim = jpeg.len() as u32 + 5_000_000;
    let p = write("trunc.rw2", &build_rw2(&jpeg, &[], Some(claim)));
    let img = orig(&p).expect("trunc decode");
    assert_eq!((img.width, img.height), (3200, 2000), "과대선언 len에도 EOF까지만 읽어 정상");
}

/// RAW 빠른 프리뷰(full_raw=false)가 요청 해상도에 맞는 IFD 임베디드 JPEG를 골라야 한다.
/// 1600px 요청은 작은 1920px 프리뷰를 쓰고, 2560px 요청은 풀해상도 임베디드를 DCT 축소해
/// ORIG 전환 없이도 더 선명한 기본 미리보기를 제공한다(#preview-audit).
#[test]
fn raw_preview_uses_hires_if_requested_edge_exceeds_small_preview() {
    use image::{ImageBuffer, Rgb};

    fn make_jpeg(w: u32, h: u32) -> Vec<u8> {
        let buf = ImageBuffer::from_fn(w, h, |x, y| {
            Rgb([(x * 255 / (w - 1)) as u8, (y * 255 / (h - 1)) as u8, 64u8])
        });
        let mut out = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgb8(buf)
            .write_to(&mut out, image::ImageFormat::Jpeg)
            .unwrap();
        out.into_inner()
    }

    fn build_rw2_with_two_embedded(small: &[u8], large: &[u8]) -> Vec<u8> {
        let entry_count = 2u16;
        let ifd_size = 2 + entry_count as usize * 12 + 4;
        let small_off = 8 + ifd_size;
        let large_off = small_off + small.len();
        let mut f = Vec::new();
        f.extend_from_slice(b"II");
        f.extend_from_slice(&0x0055u16.to_le_bytes());
        f.extend_from_slice(&8u32.to_le_bytes());
        f.extend_from_slice(&entry_count.to_le_bytes());
        // Panasonic RW2에서 흔한 작은 프리뷰(0x002e) + 풀해상도 JPEG(0x0127) 구조.
        for (tag, data, off) in [
            (0x002eu16, small, small_off),
            (0x0127u16, large, large_off),
        ] {
            f.extend_from_slice(&tag.to_le_bytes());
            f.extend_from_slice(&7u16.to_le_bytes()); // UNDEFINED blob
            f.extend_from_slice(&(data.len() as u32).to_le_bytes());
            f.extend_from_slice(&(off as u32).to_le_bytes());
        }
        f.extend_from_slice(&0u32.to_le_bytes());
        assert_eq!(f.len(), small_off);
        f.extend_from_slice(small);
        assert_eq!(f.len(), large_off);
        f.extend_from_slice(large);
        f
    }

    let small = make_jpeg(1920, 1200);
    let large = make_jpeg(3200, 2000);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("two_previews.rw2");
    std::fs::write(&path, build_rw2_with_two_embedded(&small, &large)).unwrap();

    let fast = decode::decode_file(
        &path,
        decode::DecodeOptions {
            full_raw: false,
            max_edge: Some(1600),
        },
    )
    .expect("1600px preview");
    assert_eq!(
        (fast.width, fast.height),
        (1600, 1000),
        "1600px 요청은 작은 프리뷰에서 축소"
    );

    let hires = decode::decode_file(
        &path,
        decode::DecodeOptions {
            full_raw: false,
            max_edge: Some(2560),
        },
    )
    .expect("2560px preview");
    assert_eq!(
        (hires.width, hires.height),
        (2560, 1600),
        "2560px 요청은 큰 임베디드에서 축소"
    );
}

/// CR2(Canon)식 StripOffsets(0x0111)+StripByteCounts(0x0117) 임베디드 JPEG를 합성 TIFF로
/// 검증한다(#perf C162 회귀 가드). RW2의 type-7과 달리 strip 태그로 JPEG를 가리키는 경로.
#[test]
fn orig_ifd_strip_embedded_synthetic_cr2() {
    use image::{ImageBuffer, Rgb};
    fn make_jpeg(w: u32, h: u32) -> Vec<u8> {
        let buf = ImageBuffer::from_fn(w, h, |x, y| {
            Rgb([(x * 255 / (w - 1)) as u8, (y * 255 / (h - 1)) as u8, 64u8])
        });
        let mut out = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgb8(buf)
            .write_to(&mut out, image::ImageFormat::Jpeg)
            .unwrap();
        out.into_inner()
    }
    // LE TIFF(매직 0x2a) + IFD0: StripOffsets·StripByteCounts(둘 다 type-4 LONG, count1)가
    // 뒤에 덧붙인 JPEG를 가리킨다 + Orientation. JPEG는 5000px(풀해상도급).
    let jpeg = make_jpeg(5000, 3333);
    let n_entries = 3u16;
    let ifd_size = 2 + (n_entries as usize) * 12 + 4;
    let blob_off = 8 + ifd_size;
    let mut f = Vec::new();
    f.extend_from_slice(b"II");
    f.extend_from_slice(&0x002au16.to_le_bytes());
    f.extend_from_slice(&8u32.to_le_bytes());
    f.extend_from_slice(&n_entries.to_le_bytes());
    let mut entry = |tag: u16, typ: u16, cnt: u32, val: u32, f: &mut Vec<u8>| {
        f.extend_from_slice(&tag.to_le_bytes());
        f.extend_from_slice(&typ.to_le_bytes());
        f.extend_from_slice(&cnt.to_le_bytes());
        f.extend_from_slice(&val.to_le_bytes());
    };
    entry(0x0111, 4, 1, blob_off as u32, &mut f); // StripOffsets
    entry(0x0117, 4, 1, jpeg.len() as u32, &mut f); // StripByteCounts
    entry(0x0112, 3, 1, 1, &mut f); // Orientation=1
    f.extend_from_slice(&0u32.to_le_bytes());
    assert_eq!(f.len(), blob_off);
    f.extend_from_slice(&jpeg);

    let dir = std::env::temp_dir().join("rb_cr2_test");
    std::fs::create_dir_all(&dir).unwrap();
    let p = dir.join("strip.cr2");
    std::fs::write(&p, &f).unwrap();

    // ORIG: StripOffsets JPEG(5000px)를 그 구간만 읽어 풀해상도로.
    let img = decode::decode_file(&p, decode::DecodeOptions { full_raw: true, max_edge: Some(8192) })
        .expect("CR2 strip ORIG");
    assert_eq!((img.width, img.height), (5000, 3333), "StripOffsets 임베디드 풀해상도");
}

/// ORIG는 임베디드 후보를 **바이트 길이가 아니라 디코딩 해상도**로 골라야 한다(#perf C163 회귀 가드).
/// SubIFD/IFD 체인 스캔을 켜면 본 프리뷰(고해상·저용량)보다 **바이트가 큰 저해상** JPEG가 후보에
/// 섞일 수 있다(실측: CR2 ORIG가 5616→1448로 퇴화). 길이순이 아니라 해상도순으로 골라야 한다.
/// 고해상(5000, 그라디언트=저용량)을 SubIFD에, 저해상(3500, 노이즈=고용량)을 IFD0 StripOffsets에
/// 두고 ORIG가 5000을 선택하는지 확인한다.
#[test]
fn orig_ifd_picks_by_resolution_not_byte_length() {
    use image::{ImageBuffer, Rgb};
    fn gradient_jpeg(w: u32, h: u32) -> Vec<u8> {
        let buf = ImageBuffer::from_fn(w, h, |x, y| {
            Rgb([(x * 255 / (w - 1)) as u8, (y * 255 / (h - 1)) as u8, 64u8])
        });
        let mut out = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgb8(buf).write_to(&mut out, image::ImageFormat::Jpeg).unwrap();
        out.into_inner()
    }
    // 의사난수 노이즈(고엔트로피 → JPEG가 잘 안 줄어 고용량) 저해상.
    fn noise_jpeg(w: u32, h: u32) -> Vec<u8> {
        let buf = ImageBuffer::from_fn(w, h, |x, y| {
            let mut s = x.wrapping_mul(2654435761) ^ y.wrapping_mul(40503);
            s ^= s >> 13;
            s = s.wrapping_mul(0x5bd1e995);
            s ^= s >> 15;
            Rgb([s as u8, (s >> 8) as u8, (s >> 16) as u8])
        });
        let mut out = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgb8(buf).write_to(&mut out, image::ImageFormat::Jpeg).unwrap();
        out.into_inner()
    }
    let hires = gradient_jpeg(5000, 3333); // 고해상·저용량
    let lores = noise_jpeg(3500, 2300); // 저해상·고용량
    assert!(
        lores.len() > hires.len(),
        "전제: 저해상 노이즈가 고해상 그라디언트보다 큰 바이트여야 회귀를 모의 ({} vs {})",
        lores.len(),
        hires.len()
    );

    // LE TIFF: IFD0(StripOffsets→저해상, SubIFDs→SubIFD, Orientation), SubIFD(StripOffsets→고해상).
    let ifd0_entries = 4u16;
    let subifd_off = 8 + 2 + (ifd0_entries as usize) * 12 + 4;
    let sub_entries = 2u16;
    let lores_off = subifd_off + 2 + (sub_entries as usize) * 12 + 4;
    let hires_off = lores_off + lores.len();

    let mut entry = |tag: u16, typ: u16, cnt: u32, val: u32, f: &mut Vec<u8>| {
        f.extend_from_slice(&tag.to_le_bytes());
        f.extend_from_slice(&typ.to_le_bytes());
        f.extend_from_slice(&cnt.to_le_bytes());
        f.extend_from_slice(&val.to_le_bytes());
    };
    let mut f = Vec::new();
    f.extend_from_slice(b"II");
    f.extend_from_slice(&0x002au16.to_le_bytes());
    f.extend_from_slice(&8u32.to_le_bytes());
    f.extend_from_slice(&ifd0_entries.to_le_bytes());
    entry(0x0111, 4, 1, lores_off as u32, &mut f); // StripOffsets → 저해상(고용량)
    entry(0x0117, 4, 1, lores.len() as u32, &mut f); // StripByteCounts
    entry(0x014a, 4, 1, subifd_off as u32, &mut f); // SubIFDs → 고해상
    entry(0x0112, 3, 1, 1, &mut f); // Orientation=1
    f.extend_from_slice(&0u32.to_le_bytes()); // next-IFD
    assert_eq!(f.len(), subifd_off);
    f.extend_from_slice(&sub_entries.to_le_bytes());
    entry(0x0111, 4, 1, hires_off as u32, &mut f); // SubIFD StripOffsets → 고해상(저용량)
    entry(0x0117, 4, 1, hires.len() as u32, &mut f); // StripByteCounts
    f.extend_from_slice(&0u32.to_le_bytes()); // next-IFD
    assert_eq!(f.len(), lores_off);
    f.extend_from_slice(&lores);
    assert_eq!(f.len(), hires_off);
    f.extend_from_slice(&hires);

    let dir = std::env::temp_dir().join("rb_res_test");
    std::fs::create_dir_all(&dir).unwrap();
    let p = dir.join("res.cr2");
    std::fs::write(&p, &f).unwrap();

    let img = decode::decode_file(&p, decode::DecodeOptions { full_raw: true, max_edge: Some(8192) })
        .expect("resolution-pick ORIG");
    assert_eq!(
        (img.width, img.height),
        (5000, 3333),
        "ORIG는 바이트가 작아도 해상도가 큰 임베디드(SubIFD)를 선택해야 함"
    );
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
        tags: vec![],
        split_by_tag: false,
        rename: None,
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
        tags: vec![],
        split_by_tag: false,
        rename: None,
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
        tags: vec![],
        split_by_tag: false,
        rename: None,
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
        tags: vec![],
        split_by_tag: false,
        rename: None,
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

/// #26: 별점 등급별 리네임(A1,A2,B1…). RAW+이미지 페어는 같은 새 stem을 공유해야 한다.
#[test]
fn transfer_rename_grade_grouped_keeps_pairs() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    let dst = tmp.path().join("dst");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(src.join("AAA.JPG"), b"a").unwrap();
    std::fs::write(src.join("AAA.RW2"), b"a").unwrap(); // 페어
    std::fs::write(src.join("BBB.JPG"), b"b").unwrap();
    std::fs::write(src.join("CCC.JPG"), b"c").unwrap();

    let mut entries = scan::scan_folder(&src, false, rawblow_core::SortOrder::Name);
    for e in entries.iter_mut() {
        e.stars = match e.stem.as_str() {
            "AAA" => 5,
            "BBB" => 5,
            "CCC" => 4,
            _ => 0,
        };
    }
    let req = transfer::TransferRequest {
        entries: &entries,
        labels: vec![],
        stars: vec![5, 4],
        tags: vec![],
        action: transfer::Action::Copy,
        companions: transfer::Companions::Both,
        dest: dst.clone(),
        split_by_label: false,
        split_by_tag: false,
        conflict: transfer::ConflictPolicy::AutoIncrement,
        rename: Some(transfer::RenameRule {
            template: "{gradeseq}".into(),
            numbering: transfer::Numbering::GradeGrouped,
        }),
    };
    let report = transfer::execute(&req);
    assert_eq!(report.transferred, 4, "AAA(2 페어)+BBB+CCC");
    assert!(dst.join("A1.JPG").exists(), "AAA 5★ → A1.JPG");
    assert!(dst.join("A1.RW2").exists(), "페어 RAW도 같은 stem A1");
    assert!(dst.join("A2.JPG").exists(), "BBB 5★ → A2.JPG");
    assert!(dst.join("B1.JPG").exists(), "CCC 4★ → B1.JPG");
}

/// #26: 선택 순서 + 패딩 + 원본 토큰("{seq:03}_{orig}").
#[test]
fn transfer_rename_order_padding_and_orig() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    let dst = tmp.path().join("dst");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(src.join("XQ.JPG"), b"x").unwrap();
    std::fs::write(src.join("ZP.JPG"), b"z").unwrap();

    let mut entries = scan::scan_folder(&src, false, rawblow_core::SortOrder::Name);
    for e in entries.iter_mut() {
        e.label = Label::Pick;
    }
    let req = transfer::TransferRequest {
        entries: &entries,
        labels: vec![Label::Pick],
        stars: vec![],
        tags: vec![],
        action: transfer::Action::Copy,
        companions: transfer::Companions::Both,
        dest: dst.clone(),
        split_by_label: false,
        split_by_tag: false,
        conflict: transfer::ConflictPolicy::AutoIncrement,
        rename: Some(transfer::RenameRule {
            template: "{seq:03}_{orig}".into(),
            numbering: transfer::Numbering::Order,
        }),
    };
    transfer::execute(&req);
    assert!(dst.join("001_XQ.JPG").exists(), "1번째 → 001_XQ");
    assert!(dst.join("002_ZP.JPG").exists(), "2번째 → 002_ZP");
}

/// #27: 컬러 태그를 전송 선택 기준(합집합)으로, 태그별 하위폴더(@green) 분기.
#[test]
fn transfer_tag_union_and_split_subfolder() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    let dst = tmp.path().join("dst");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(src.join("G.JPG"), b"g").unwrap();
    std::fs::write(src.join("R.JPG"), b"r").unwrap();
    std::fs::write(src.join("N.JPG"), b"n").unwrap();

    let mut entries = scan::scan_folder(&src, false, rawblow_core::SortOrder::Name);
    for e in entries.iter_mut() {
        e.tag = match e.stem.as_str() {
            "G" => ColorTag::Teal,
            "R" => ColorTag::Orange,
            _ => ColorTag::None,
        };
    }
    let req = transfer::TransferRequest {
        entries: &entries,
        labels: vec![],
        stars: vec![],
        tags: vec![ColorTag::Teal], // Teal만 선택
        action: transfer::Action::Copy,
        companions: transfer::Companions::Both,
        dest: dst.clone(),
        split_by_label: false,
        split_by_tag: true,
        conflict: transfer::ConflictPolicy::AutoIncrement,
        rename: None,
    };
    let report = transfer::execute(&req);
    assert_eq!(report.transferred, 1, "Teal만 전송");
    assert!(dst.join("@teal").join("G.JPG").exists(), "태그별 하위폴더");
    assert!(!dst.join("@orange").exists(), "선택 안 한 태그는 전송 안 됨");
}

/// #27: 사이드카가 태그를 보존(구버전 호환). 태그만 있는(라벨/별점 없는) 항목도 저장된다.
#[test]
fn sidecar_tag_roundtrip() {
    let tmp = tempfile::tempdir().unwrap();
    let folder = tmp.path();
    std::fs::write(folder.join("ONLY_TAG.JPG"), b"x").unwrap();
    std::fs::write(folder.join("PICKED.JPG"), b"y").unwrap();

    let mut entries = scan::scan_folder(folder, false, rawblow_core::SortOrder::Name);
    for e in entries.iter_mut() {
        match e.stem.as_str() {
            "ONLY_TAG" => e.tag = ColorTag::Teal, // 라벨/별점 없이 태그만
            "PICKED" => e.label = Label::Pick,
            _ => {}
        }
    }
    sidecar::save(folder, &entries).unwrap();

    let mut reloaded = scan::scan_folder(folder, false, rawblow_core::SortOrder::Name);
    let session = sidecar::load(folder).expect("세션 로드");
    sidecar::apply(&session, &mut reloaded);

    let only = reloaded.iter().find(|e| e.stem == "ONLY_TAG").unwrap();
    assert_eq!(only.tag, ColorTag::Teal, "태그만 있는 항목도 저장·복원");
    let picked = reloaded.iter().find(|e| e.stem == "PICKED").unwrap();
    assert_eq!(picked.label, Label::Pick);
    assert_eq!(picked.tag, ColorTag::None);
}

// ── #35 진행 보고·취소 ─────────────────────────────────────────────

#[test]
fn transfer_progress_reports_and_completes() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    let dst = tmp.path().join("dst");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(src.join("A.JPG"), b"aa").unwrap();
    std::fs::write(src.join("B.JPG"), b"bbb").unwrap();
    std::fs::write(src.join("C.JPG"), b"cccc").unwrap();

    let mut entries = scan::scan_folder(&src, false, rawblow_core::SortOrder::Name);
    for e in entries.iter_mut() {
        e.label = Label::Pick;
    }
    let req = transfer::TransferRequest {
        entries: &entries,
        labels: vec![Label::Pick],
        stars: vec![],
        action: transfer::Action::Copy,
        companions: transfer::Companions::Both,
        dest: dst.clone(),
        split_by_label: false,
        conflict: transfer::ConflictPolicy::AutoIncrement,
        tags: vec![],
        split_by_tag: false,
        rename: None,
    };
    let mut last_total = 0usize;
    let mut calls = 0usize;
    let report = transfer::execute_with_progress(&req, &mut |p| {
        calls += 1;
        last_total = p.total;
        true
    });
    assert_eq!(report.transferred, 3);
    assert!(!report.canceled);
    assert_eq!(last_total, 3, "분모는 전체 대상 파일 수");
    assert_eq!(calls, 3, "파일마다 한 번씩 진행 보고");
}

#[test]
fn transfer_progress_cancel_stops_midway() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    let dst = tmp.path().join("dst");
    std::fs::create_dir_all(&src).unwrap();
    for n in 0..5 {
        std::fs::write(src.join(format!("F{n}.JPG")), b"x").unwrap();
    }
    let mut entries = scan::scan_folder(&src, false, rawblow_core::SortOrder::Name);
    for e in entries.iter_mut() {
        e.label = Label::Pick;
    }
    let req = transfer::TransferRequest {
        entries: &entries,
        labels: vec![Label::Pick],
        stars: vec![],
        action: transfer::Action::Copy,
        companions: transfer::Companions::Both,
        dest: dst.clone(),
        split_by_label: false,
        conflict: transfer::ConflictPolicy::AutoIncrement,
        tags: vec![],
        split_by_tag: false,
        rename: None,
    };
    // 두 번째 파일 직전에 취소(첫 파일 전 호출에서 true, 그 다음 false).
    let mut seen = 0usize;
    let report = transfer::execute_with_progress(&req, &mut |_| {
        seen += 1;
        seen < 2
    });
    assert!(report.canceled, "취소 플래그 설정");
    assert_eq!(report.transferred, 1, "취소 전 1건만 전송");
}

// ── #34 폴더 자동 분류 ─────────────────────────────────────────────

#[test]
fn organize_by_extension_splits_pairs() {
    use rawblow_core::organize::{self, OrganizeKey, OrganizeRequest};
    let tmp = tempfile::tempdir().unwrap();
    let folder = tmp.path();
    std::fs::write(folder.join("A.JPG"), b"a").unwrap();
    std::fs::write(folder.join("A.RW2"), b"aaaa").unwrap();
    std::fs::write(folder.join("B.JPG"), b"b").unwrap();

    let entries = scan::scan_folder(folder, false, rawblow_core::SortOrder::Name);
    let req = OrganizeRequest {
        entries: &entries,
        key: OrganizeKey::Extension,
        action: transfer::Action::Move,
        dest_root: folder.to_path_buf(),
        conflict: transfer::ConflictPolicy::AutoIncrement,
    };
    let report = organize::organize(&req);
    assert_eq!(report.transferred, 3);
    assert!(folder.join("JPG").join("A.JPG").exists());
    assert!(folder.join("JPG").join("B.JPG").exists());
    assert!(folder.join("RW2").join("A.RW2").exists(), "확장자별로 페어가 갈라짐");
    assert!(!folder.join("A.JPG").exists(), "이동이므로 원본 자리에 없음");
}

#[test]
fn organize_by_camera_keeps_pairs_together() {
    use rawblow_core::organize::{self, OrganizeKey, OrganizeRequest};
    let tmp = tempfile::tempdir().unwrap();
    let folder = tmp.path();
    // EXIF 없는 더미 → "unknown-camera"로 묶임. 페어(RAW+JPG)는 같은 폴더 유지가 핵심.
    std::fs::write(folder.join("A.JPG"), b"a").unwrap();
    std::fs::write(folder.join("A.RW2"), b"aaaa").unwrap();

    let entries = scan::scan_folder(folder, false, rawblow_core::SortOrder::Name);
    assert_eq!(entries.len(), 1, "A.JPG+A.RW2는 한 항목");
    let req = OrganizeRequest {
        entries: &entries,
        key: OrganizeKey::Camera,
        action: transfer::Action::Copy,
        dest_root: folder.to_path_buf(),
        conflict: transfer::ConflictPolicy::AutoIncrement,
    };
    let report = organize::organize(&req);
    assert_eq!(report.transferred, 2);
    assert!(folder.join("unknown-camera").join("A.JPG").exists());
    assert!(folder.join("unknown-camera").join("A.RW2").exists(), "페어는 같은 폴더로");
}

#[test]
fn organize_skips_files_already_in_place() {
    use rawblow_core::organize::{self, OrganizeKey, OrganizeRequest};
    let tmp = tempfile::tempdir().unwrap();
    let folder = tmp.path();
    let jpg_dir = folder.join("JPG");
    std::fs::create_dir_all(&jpg_dir).unwrap();
    std::fs::write(jpg_dir.join("A.JPG"), b"a").unwrap();

    // 이미 JPG/ 안에 있는 파일을 확장자 기준으로 다시 정리해도 다시 옮기지 않는다(in-place 안전).
    let entries = scan::scan_folder(folder, true, rawblow_core::SortOrder::Name);
    let req = OrganizeRequest {
        entries: &entries,
        key: OrganizeKey::Extension,
        action: transfer::Action::Move,
        dest_root: folder.to_path_buf(),
        conflict: transfer::ConflictPolicy::AutoIncrement,
    };
    let report = organize::organize(&req);
    assert_eq!(report.transferred, 0, "이미 제자리면 이동 없음");
    assert!(jpg_dir.join("A.JPG").exists());
}

// ── AF 포인트 추출(#37) — 실파일 교차검증(ExifTool 13.59 덤프 기준, plan-af-points §4-3) ──

/// 샘플이 없으면 조용히 건너뛴다(soft-skip). AF 테스트는 합성 단위테스트(af.rs)가
/// 기본 가드이고, 이 테스트는 실파일 좌표가 ExifTool과 일치하는지 추가 확인용.
fn sample_file(rel: &str) -> Option<PathBuf> {
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../sample").join(rel);
    p.exists().then_some(p)
}

#[test]
fn af_panasonic_rw2_matches_exiftool() {
    let Some(p) = sample_file("P1186492.RW2") else {
        eprintln!("skip: sample/P1186492.RW2 없음");
        return;
    };
    let af = rawblow_core::af::parse_af(&p).expect("RW2 AF 추출");
    assert_eq!(af.source, "panasonic");
    assert_eq!(af.points.len(), 1);
    let pt = af.points[0];
    // ExifTool: AFPointPosition "0.65 0.46", AFAreaSize "0.0498046875 0.07421875".
    assert!((pt.cx - 0.65).abs() < 0.005, "cx={}", pt.cx);
    assert!((pt.cy - 0.46).abs() < 0.005, "cy={}", pt.cy);
    assert!((pt.w - 0.0498046875).abs() < 1e-6, "w={}", pt.w);
    assert!((pt.h - 0.07421875).abs() < 1e-6, "h={}", pt.h);
}

#[test]
fn af_canon_cr2_matches_exiftool() {
    let Some(p) = sample_file("IMG_0005.CR2") else {
        eprintln!("skip: sample/IMG_0005.CR2 없음");
        return;
    };
    let af = rawblow_core::af::parse_af(&p).expect("CR2 AF 추출");
    assert_eq!(af.source, "canon-afinfo2");
    // ExifTool: NumAFPoints 9, AFImageWidth/Height 5616/3744,
    // AFAreaXPositions -1173 -561 0 561 1173 561 0 -561 0, InFocus 1,2,8, Selected 0~8.
    assert_eq!(af.points.len(), 9);
    let inf: Vec<usize> = af.points.iter().enumerate().filter(|(_, p)| p.in_focus).map(|(i, _)| i).collect();
    assert_eq!(inf, vec![1, 2, 8]);
    assert!(af.points.iter().all(|p| p.selected));
    let p1 = af.points[1];
    assert!((p1.cx - (0.5 - 561.0 / 5616.0)).abs() < 1e-9, "cx={}", p1.cx);
    // Y=280(위로 양수) → cy = 0.5 - 280/3744.
    assert!((p1.cy - (0.5 - 280.0 / 3744.0)).abs() < 1e-9, "cy={}", p1.cy);
    // 모든 점이 0..1 안.
    assert!(af.points.iter().all(|p| (0.0..=1.0).contains(&p.cx) && (0.0..=1.0).contains(&p.cy)));
}

#[test]
fn af_sony_arw_matches_exiftool() {
    let Some(p) = sample_file("organize_test/_DSC0504.ARW") else {
        eprintln!("skip: sample/organize_test/_DSC0504.ARW 없음");
        return;
    };
    let af = rawblow_core::af::parse_af(&p).expect("ARW AF 추출");
    assert_eq!(af.source, "sony");
    let pt = af.points[0];
    // ExifTool: FocusLocation "7952 5304 5976 2552".
    assert!((pt.cx - 5976.0 / 7952.0).abs() < 1e-9, "cx={}", pt.cx);
    assert!((pt.cy - 2552.0 / 5304.0).abs() < 1e-9, "cy={}", pt.cy);
}

// ── GPS 추출(#38) — 합성 TIFF(GPS IFD)로 결정적 검증(샘플 불필요) ──────────

#[test]
fn gps_extraction_from_synthetic_tiff() {
    // LE TIFF: IFD0[GPSInfoIFDPointer] → GPS IFD[LatRef,Lat,LonRef,Lon,AltRef,Alt].
    fn put16(b: &mut Vec<u8>, v: u16) { b.extend_from_slice(&v.to_le_bytes()); }
    fn put32(b: &mut Vec<u8>, v: u32) { b.extend_from_slice(&v.to_le_bytes()); }
    fn rat(b: &mut Vec<u8>, n: u32, d: u32) { put32(b, n); put32(b, d); }

    let mut b = Vec::new();
    b.extend_from_slice(b"II");
    put16(&mut b, 42);
    put32(&mut b, 8);
    // IFD0: 엔트리 1(GPS IFD 포인터 0x8825). 8+2+12+4 = 26.
    put16(&mut b, 1);
    put16(&mut b, 0x8825);
    put16(&mut b, 4);
    put32(&mut b, 1);
    put32(&mut b, 26);
    put32(&mut b, 0);
    // GPS IFD: 엔트리 6개 → 26+2+6*12+4 = 104 부터 데이터.
    assert_eq!(b.len(), 26);
    put16(&mut b, 6);
    // 0x01 GPSLatitudeRef = "S" (남위 → 음수).
    put16(&mut b, 0x0001); put16(&mut b, 2); put32(&mut b, 2); b.extend_from_slice(b"S\0\0\0");
    // 0x02 GPSLatitude = 33° 52' 4.8" (시드니 근방) @ offset 104.
    put16(&mut b, 0x0002); put16(&mut b, 5); put32(&mut b, 3); put32(&mut b, 104);
    // 0x03 GPSLongitudeRef = "E".
    put16(&mut b, 0x0003); put16(&mut b, 2); put32(&mut b, 2); b.extend_from_slice(b"E\0\0\0");
    // 0x04 GPSLongitude = 151° 12' 36" @ offset 128.
    put16(&mut b, 0x0004); put16(&mut b, 5); put32(&mut b, 3); put32(&mut b, 128);
    // 0x05 GPSAltitudeRef = 1 (해수면 아래).
    put16(&mut b, 0x0005); put16(&mut b, 1); put32(&mut b, 1); put32(&mut b, 1);
    // 0x06 GPSAltitude = 12.5m @ offset 152.
    put16(&mut b, 0x0006); put16(&mut b, 5); put32(&mut b, 1); put32(&mut b, 152);
    put32(&mut b, 0); // next IFD.
    assert_eq!(b.len(), 104);
    rat(&mut b, 33, 1); rat(&mut b, 52, 1); rat(&mut b, 48, 10); // 33°52'4.8"
    rat(&mut b, 151, 1); rat(&mut b, 12, 1); rat(&mut b, 36, 1); // 151°12'36"
    rat(&mut b, 125, 10); // 12.5m

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("gps.tif");
    std::fs::write(&path, &b).unwrap();
    let info = rawblow_core::meta::read_exif(&path).expect("EXIF 읽기");
    let gps = info.gps.expect("GPS 추출");
    assert!((gps.lat - -(33.0 + 52.0 / 60.0 + 4.8 / 3600.0)).abs() < 1e-9, "lat={}", gps.lat);
    assert!((gps.lon - (151.0 + 12.0 / 60.0 + 36.0 / 3600.0)).abs() < 1e-9, "lon={}", gps.lon);
    assert_eq!(gps.alt, Some(-12.5));
}

/// 미리보기 디코딩 속도 측정 — RW2/CR2 각 1920px, release 빌드 권장(debug는 5~6× 느림).
/// `cargo test --release -p rawblow-core -- preview_decode_timing --nocapture`
#[test]
fn preview_decode_timing() {
    use std::time::Instant;
    let files: &[(&str, u32)] = &[
        ("P1186492.RW2", 1920),
        ("IMG_0005.CR2", 1920),
    ];
    for (rel, edge) in files {
        let Some(p) = sample_file(rel) else {
            eprintln!("skip: sample/{rel} 없음");
            continue;
        };
        let mut times = Vec::new();
        for _ in 0..3 {
            let t0 = Instant::now();
            let img = decode::decode_file(
                &p,
                decode::DecodeOptions { full_raw: false, max_edge: Some(*edge) },
            )
            .unwrap_or_else(|e| panic!("{rel} 디코딩 실패: {e}"));
            times.push(t0.elapsed());
            assert!(img.width > 0 && img.height > 0);
        }
        let best_ms = times.iter().map(|d| d.as_millis()).min().unwrap();
        eprintln!("[preview_timing] {rel} {edge}px → {best_ms}ms (best-of-3)");
        assert!(best_ms < 500, "{rel} {edge}px 프리뷰 너무 느림: {best_ms}ms (상한 500ms)");
    }
}

#[test]
fn gps_extraction_from_real_tagged_jpg() {
    // ExifTool로 지오태깅한 QA 샘플(sample/gps_test/, gitignored). 없으면 skip.
    // 오너 QA용으로 A_ 접두사가 붙은 사본도 허용(정렬 우선용 리네임으로 skip되던 문제).
    let Some(p) = sample_file("gps_test/GPS_SEOUL.JPG")
        .or_else(|| sample_file("gps_test/A_GPS_SEOUL.JPG"))
    else {
        eprintln!("skip: sample/gps_test/(A_)GPS_SEOUL.JPG 없음");
        return;
    };
    let gps = rawblow_core::meta::read_exif(&p).and_then(|i| i.gps).expect("GPS 추출");
    assert!((gps.lat - 37.5665).abs() < 1e-4, "lat={}", gps.lat);
    assert!((gps.lon - 126.978).abs() < 1e-4, "lon={}", gps.lon);
    // 남반구 부호.
    if let Some(p2) = sample_file("gps_test/GPS_SYDNEY.JPG") {
        let g2 = rawblow_core::meta::read_exif(&p2).and_then(|i| i.gps).expect("GPS 추출");
        assert!(g2.lat < 0.0 && (g2.lat + 33.8568).abs() < 1e-4, "lat={}", g2.lat);
    }
    // GPS 없는 파일은 None.
    if let Some(p3) = sample_file("gps_test/GPS_NONE.JPG") {
        assert!(rawblow_core::meta::read_exif(&p3).map_or(true, |i| i.gps.is_none()));
    }
}
