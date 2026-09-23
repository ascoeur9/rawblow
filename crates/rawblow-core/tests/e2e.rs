//! 코어 파이프라인 E2E: 스캔 → 사이드카 → 전송 → 정리 → 디코드, HEIC 항목 경로(#114),
//! ORIG 판정(#109), AI 컬링 판정 근거와 실제 판정의 일치(#91).
//! sample/ 없이 돌아가게 합성 JPEG/PNG/HEIF만 쓴다.

use image::{ImageBuffer, Rgb};
use rawblow_core::config::{self, Config};
use rawblow_core::model::{Entry, Kind, Label};
use rawblow_core::organize::{self, OrganizeKey, OrganizeRequest};
use rawblow_core::{decode, scan, sidecar, transfer};
use std::io::Cursor;
use std::path::{Path, PathBuf};

fn jpeg_bytes(w: u32, h: u32) -> Vec<u8> {
    let buf = ImageBuffer::from_fn(w, h, |x, y| {
        Rgb([
            (x * 255 / w.max(1)) as u8,
            (y * 255 / h.max(1)) as u8,
            90,
        ])
    });
    let mut out = Cursor::new(Vec::new());
    image::DynamicImage::ImageRgb8(buf)
        .write_to(&mut out, image::ImageFormat::Jpeg)
        .unwrap();
    out.into_inner()
}

fn write_jpeg(path: &Path, w: u32, h: u32) {
    std::fs::write(path, jpeg_bytes(w, h)).unwrap();
}

fn bx(typ: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let mut v = ((8 + body.len()) as u32).to_be_bytes().to_vec();
    v.extend_from_slice(typ);
    v.extend_from_slice(body);
    v
}

fn full(ver: u8, rest: &[u8]) -> Vec<u8> {
    let mut b = vec![ver, 0, 0, 0];
    b.extend_from_slice(rest);
    b
}

/// primary `hvc1`(쓰레기) + `thmb` JPEG. 그리드/컬링은 HEVC 없이 JPEG를 써야 한다(#114).
fn heif_hvc1_with_jpeg_thumb(jpeg: &[u8]) -> Vec<u8> {
    heif_with_primary(jpeg, 1)
}

/// `primary_id`가 1이면 hvc1(쓰레기)이 본 이미지, 2면 JPEG-in-HEIF.
fn heif_with_primary(jpeg: &[u8], primary_id: u16) -> Vec<u8> {
    heif_with_primary_props(jpeg, primary_id, &[])
}

/// 항목 2(jpeg)에 변환 속성(`irot`/`imir` 박스)을 ipma 순서대로 붙인다.
fn heif_with_primary_props(jpeg: &[u8], primary_id: u16, props: &[Vec<u8>]) -> Vec<u8> {
    let garbage = b"not-hevc";
    let mut infe1 = full(2, &[]);
    infe1.extend_from_slice(&1u16.to_be_bytes());
    infe1.extend_from_slice(&0u16.to_be_bytes());
    infe1.extend_from_slice(b"hvc1");
    infe1.push(0);
    let mut infe2 = full(2, &[]);
    infe2.extend_from_slice(&2u16.to_be_bytes());
    infe2.extend_from_slice(&0u16.to_be_bytes());
    infe2.extend_from_slice(b"jpeg");
    infe2.push(0);
    let mut iinf_body = full(0, &2u16.to_be_bytes());
    iinf_body.extend_from_slice(&bx(b"infe", &infe1));
    iinf_body.extend_from_slice(&bx(b"infe", &infe2));
    let iinf = bx(b"iinf", &iinf_body);
    let pitm = bx(b"pitm", &full(0, &primary_id.to_be_bytes()));
    let mut thmb = Vec::new();
    thmb.extend_from_slice(&1u16.to_be_bytes());
    thmb.extend_from_slice(&1u16.to_be_bytes());
    thmb.extend_from_slice(&2u16.to_be_bytes());
    let mut iref_body = full(0, &[]);
    iref_body.extend_from_slice(&bx(b"thmb", &thmb));
    let iref = bx(b"iref", &iref_body);

    let mut iloc_rest = vec![0x44, 0x00];
    iloc_rest.extend_from_slice(&2u16.to_be_bytes());
    iloc_rest.extend_from_slice(&1u16.to_be_bytes());
    iloc_rest.extend_from_slice(&0u16.to_be_bytes());
    iloc_rest.extend_from_slice(&1u16.to_be_bytes());
    let off1_at = iloc_rest.len();
    iloc_rest.extend_from_slice(&0u32.to_be_bytes());
    iloc_rest.extend_from_slice(&(garbage.len() as u32).to_be_bytes());
    iloc_rest.extend_from_slice(&2u16.to_be_bytes());
    iloc_rest.extend_from_slice(&0u16.to_be_bytes());
    iloc_rest.extend_from_slice(&1u16.to_be_bytes());
    let off2_at = iloc_rest.len();
    iloc_rest.extend_from_slice(&0u32.to_be_bytes());
    iloc_rest.extend_from_slice(&(jpeg.len() as u32).to_be_bytes());
    let iloc = bx(b"iloc", &full(0, &iloc_rest));

    let mut meta_body = full(0, &[]);
    meta_body.extend_from_slice(&pitm);
    meta_body.extend_from_slice(&iinf);
    let iloc_at = meta_body.len();
    meta_body.extend_from_slice(&iloc);
    meta_body.extend_from_slice(&iref);
    if !props.is_empty() {
        let ipco = bx(b"ipco", &props.concat());
        let mut ipma = full(0, &1u32.to_be_bytes());
        ipma.extend_from_slice(&2u16.to_be_bytes());
        ipma.push(props.len() as u8);
        for i in 1..=props.len() as u8 {
            ipma.push(0x80 | i); // essential + 1부터 번호
        }
        let mut iprp = ipco;
        iprp.extend_from_slice(&bx(b"ipma", &ipma));
        meta_body.extend_from_slice(&bx(b"iprp", &iprp));
    }
    let meta = bx(b"meta", &meta_body);
    let ftyp = bx(b"ftyp", b"heic\0\0\0\0mif1heic");
    let ftyp_len = ftyp.len();

    let mut mdat_body = Vec::new();
    mdat_body.extend_from_slice(garbage);
    let jpeg_rel = mdat_body.len();
    mdat_body.extend_from_slice(jpeg);
    let mdat = bx(b"mdat", &mdat_body);

    let mut file = ftyp;
    file.extend_from_slice(&meta);
    let mdat_at = file.len();
    file.extend_from_slice(&mdat);
    let g_off = (mdat_at + 8) as u32;
    let j_off = (mdat_at + 8 + jpeg_rel) as u32;
    let rest_at = ftyp_len + 8 + iloc_at + 8 + 4;
    let o1 = rest_at + off1_at;
    let o2 = rest_at + off2_at;
    file[o1..o1 + 4].copy_from_slice(&g_off.to_be_bytes());
    file[o2..o2 + 4].copy_from_slice(&j_off.to_be_bytes());
    file
}

#[test]
fn e2e_scan_sidecar_transfer_organize_decode() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("shoot");
    std::fs::create_dir_all(&src).unwrap();
    write_jpeg(&src.join("IMG_0001.JPG"), 64, 48);
    std::fs::write(src.join("IMG_0001.RW2"), b"raw-bytes").unwrap();
    write_jpeg(&src.join("IMG_0002.JPG"), 40, 30);

    // 스캔: RAW+JPG는 한 항목, JPG만 하나.
    let mut entries = scan::scan_folder(&src, false, rawblow_core::SortOrder::Name);
    assert_eq!(entries.len(), 2);
    let pair = entries
        .iter()
        .find(|e| e.stem.eq_ignore_ascii_case("IMG_0001"))
        .expect("pair");
    assert!(pair.has_raw && pair.has_image);
    assert_eq!(pair.display.extension().unwrap().to_ascii_uppercase(), "JPG");

    // 사이드카 저장·복원.
    entries[0].label = Label::Pick;
    entries[0].stars = 5;
    entries[1].label = Label::Reject;
    sidecar::save(&src, &entries).unwrap();
    let mut reloaded = scan::scan_folder(&src, false, rawblow_core::SortOrder::Name);
    let session = sidecar::load(&src).expect("session");
    sidecar::apply(&session, &mut reloaded, &src);
    assert_eq!(reloaded[0].label, entries[0].label);
    assert_eq!(reloaded[0].stars, 5);
    assert_eq!(reloaded[1].label, Label::Reject);

    // 한 폴더 모드 기본 dest = 원본/selected — 원본 폴더와 섞이지 않는다(#113 #115).
    let dest = Config::default().transfer_default_dest(Some(&src), transfer::TransferSplit::None);
    assert_eq!(dest, src.join(config::TRANSFER_FLAT_SUBFOLDER));
    let picks: Vec<Entry> = reloaded
        .iter()
        .filter(|e| e.label == Label::Pick)
        .cloned()
        .collect();
    let report = transfer::execute(&transfer::TransferRequest {
        entries: &picks,
        labels: vec![Label::Pick],
        stars: vec![],
        tags: vec![],
        action: transfer::Action::Copy,
        companions: transfer::Companions::Both,
        dest: dest.clone(),
        split: transfer::TransferSplit::None,
        conflict: transfer::ConflictPolicy::AutoIncrement,
        rename: None,
    });
    assert_eq!(report.transferred, 2, "JPG+RW2");
    assert!(dest.join("IMG_0001.JPG").exists());
    assert!(dest.join("IMG_0001.RW2").exists());
    assert!(src.join("IMG_0001.JPG").exists(), "copy라 원본 유지");
    assert!(
        !src.join("IMG_0001_001.JPG").exists(),
        "원본 폴더에 복사본이 생기면 안 됨"
    );
    let src_names: Vec<_> = std::fs::read_dir(&src)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n != ".rawblow")
        .collect();
    assert!(
        !src_names.iter().any(|n| n.contains("selected") && n.ends_with(".JPG")),
        "selected 파일이 원본 루트에 있으면 안 됨: {src_names:?}"
    );

    // 정리: 확장자 하위폴더.
    let org_root = tmp.path().join("organized");
    std::fs::create_dir_all(&org_root).unwrap();
    let org = organize::organize(&OrganizeRequest {
        entries: &reloaded,
        key: OrganizeKey::Extension,
        action: transfer::Action::Copy,
        dest_root: org_root.clone(),
        conflict: transfer::ConflictPolicy::AutoIncrement,
    });
    assert!(org.transferred >= 3);
    assert!(org_root.join("JPG").join("IMG_0001.JPG").exists() || org_root.join("JPG").join("IMG_0002.JPG").exists());

    // JPEG 디코드.
    let img = decode::decode_file(
        &src.join("IMG_0001.JPG"),
        decode::DecodeOptions {
            full_raw: false,
            max_edge: Some(32),
        },
    )
    .expect("jpeg decode");
    assert!(img.width > 0 && img.height > 0);
    assert_eq!(img.rgba.len(), (img.width * img.height * 4) as usize);
}

#[test]
fn e2e_heic_thumb_jpeg_item_skips_hevc() {
    // 그리드: 요청 크기에 걸맞은 jpeg 항목이면 HEVC(여기선 쓰레기라 풀면 실패)를 건드리지 않는다.
    let jpeg = jpeg_bytes(320, 240);
    let heif = heif_hvc1_with_jpeg_thumb(&jpeg);
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("iphone.heic");
    std::fs::write(&p, &heif).unwrap();

    let thumb = decode::decode_file(&p, decode::DecodeOptions { full_raw: false, max_edge: Some(320) })
        .expect("grid thumb uses jpeg item");
    assert_eq!((thumb.width, thumb.height), (320, 240));
}

#[test]
fn e2e_heic_cull_edge_does_not_use_small_thumb() {
    // 컬링(1024)을 320px 썸네일로 재면 HEIC만 판정 기준이 달라진다 — 작은 항목은 쓰지 않고
    // 본 이미지로 가야 한다(여기선 HEVC가 쓰레기라 실패가 곧 "썸네일을 안 썼다"는 증거).
    let jpeg = jpeg_bytes(320, 240);
    let heif = heif_hvc1_with_jpeg_thumb(&jpeg);
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("iphone.heic");
    std::fs::write(&p, &heif).unwrap();
    let cull = decode::decode_file(&p, decode::DecodeOptions { full_raw: false, max_edge: Some(1024) });
    assert!(cull.is_err(), "컬링이 작은 썸네일로 대체됨: {:?}", cull.map(|i| (i.width, i.height)));

    // 요청 크기에 충분한 항목(≥75%)이면 컬링도 쓴다.
    let big = heif_hvc1_with_jpeg_thumb(&jpeg_bytes(1024, 768));
    let p2 = dir.path().join("big_thumb.heic");
    std::fs::write(&p2, &big).unwrap();
    let img = decode::decode_file(&p2, decode::DecodeOptions { full_raw: false, max_edge: Some(1024) })
        .expect("large jpeg item is fine for culling");
    assert_eq!(img.width.max(img.height), 1024);
}

#[test]
fn e2e_junk_heic_fails() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("junk.heic");
    std::fs::write(&p, b"not a heif file").unwrap();
    assert!(decode::decode_file(
        &p,
        decode::DecodeOptions {
            full_raw: false,
            max_edge: Some(320)
        }
    )
    .is_err());
}

#[test]
fn e2e_heif_ispe_long_edge() {
    fn box_with(typ: &[u8; 4], payload: &[u8]) -> Vec<u8> {
        bx(typ, payload)
    }
    let ispe = |w: u32, h: u32| {
        let mut p = vec![0u8; 12];
        p[4..8].copy_from_slice(&w.to_be_bytes());
        p[8..12].copy_from_slice(&h.to_be_bytes());
        box_with(b"ispe", &p)
    };
    let mut ipco_p = ispe(320, 240);
    ipco_p.extend_from_slice(&ispe(4032, 3024));
    let ipco = box_with(b"ipco", &ipco_p);
    let iprp = box_with(b"iprp", &ipco);
    let mut meta_payload = vec![0u8; 4];
    meta_payload.extend_from_slice(&iprp);
    let meta = box_with(b"meta", &meta_payload);
    let mut ftyp = box_with(b"ftyp", b"heic\0\0\0\0");
    ftyp.extend_from_slice(&meta);
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("size.heic");
    std::fs::write(&p, &ftyp).unwrap();
    assert_eq!(decode::orig_long_edge(&p), Some(4032));
}

#[test]
fn e2e_pairing_prefers_heic_over_raw() {
    let e = Entry::from_members(
        "IMG_0001".into(),
        vec![
            PathBuf::from("IMG_0001.DNG"),
            PathBuf::from("IMG_0001.HEIC"),
        ],
    );
    assert_eq!(e.display.extension().unwrap(), "HEIC");
    assert!(e.has_raw && e.has_image);
    assert_eq!(rawblow_core::model::kind_of(&e.display), Some(Kind::Image));
}

#[test]
fn e2e_heic_main_view_does_not_fall_back_to_thumbnail() {
    // #114: 작은 `jpeg` 썸네일 항목은 그리드/컬링 전용이다. 본 화면(1920) 요청에서 쓰면
    // 12MP 자리에 48px 썸네일이 뜬다 — HEVC를 못 풀면 차라리 실패해야 한다.
    let jpeg = jpeg_bytes(48, 32);
    let heif = heif_hvc1_with_jpeg_thumb(&jpeg);
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("iphone.heic");
    std::fs::write(&p, &heif).unwrap();
    let full = decode::decode_file(
        &p,
        decode::DecodeOptions {
            full_raw: false,
            max_edge: Some(1920),
        },
    );
    assert!(
        full.is_err(),
        "본 화면 요청이 썸네일로 조용히 대체되면 안 됨: {:?}",
        full.map(|i| (i.width, i.height))
    );
}

#[test]
fn e2e_jpeg_in_heif_primary_is_used_at_full_size() {
    // primary가 JPEG면(heif-oxide가 거절하는 JPEG-in-HEIF) 그게 본 이미지다.
    let jpeg = jpeg_bytes(48, 32);
    let heif = heif_with_primary(&jpeg, 2);
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("jpeg_in_heif.heic");
    std::fs::write(&p, &heif).unwrap();
    let img = decode::decode_file(
        &p,
        decode::DecodeOptions {
            full_raw: true,
            max_edge: Some(1920),
        },
    )
    .expect("jpeg primary item");
    assert_eq!((img.width, img.height), (48, 32));
    assert!(img.full_raw, "#109: 본 이미지를 풀었으면 ORIG 성공으로 표시");
}

#[test]
fn e2e_orig_on_plain_image_reports_full_raw() {
    // #109 회귀: JPG/PNG는 ORIG 요청이 곧 원본 디코딩이다. full_raw=false로 오면 UI가
    // "원본 해상도를 찾지 못했다"는 거짓 안내를 띄운다.
    let dir = tempfile::tempdir().unwrap();
    let jpg = dir.path().join("plain.JPG");
    write_jpeg(&jpg, 64, 48);
    let orig = decode::decode_file(
        &jpg,
        decode::DecodeOptions {
            full_raw: true,
            max_edge: Some(8192),
        },
    )
    .expect("jpg orig");
    assert!(orig.full_raw, "JPG ORIG는 원본 그 자체");
    let preview = decode::decode_file(
        &jpg,
        decode::DecodeOptions {
            full_raw: false,
            max_edge: Some(1920),
        },
    )
    .expect("jpg preview");
    assert!(!preview.full_raw);

    let png = dir.path().join("plain.png");
    let buf = ImageBuffer::from_fn(32, 24, |x, _| Rgb([x as u8, 40, 200]));
    image::DynamicImage::ImageRgb8(buf).save(&png).unwrap();
    let png_orig = decode::decode_file(
        &png,
        decode::DecodeOptions {
            full_raw: true,
            max_edge: Some(8192),
        },
    )
    .expect("png orig");
    assert!(png_orig.full_raw);
}

#[test]
fn e2e_split_by_label_goes_to_label_subfolders() {
    // 나누기 모드(#113 #89): 원본 폴더를 루트로 두고 라벨별 하위폴더로 나눈다.
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("shoot");
    std::fs::create_dir_all(&src).unwrap();
    write_jpeg(&src.join("A.JPG"), 16, 16);
    write_jpeg(&src.join("B.JPG"), 16, 16);
    let mut entries = scan::scan_folder(&src, false, rawblow_core::SortOrder::Name);
    entries[0].label = Label::Pick;
    entries[1].label = Label::Reject;
    let dest = Config::default().transfer_default_dest(Some(&src), transfer::TransferSplit::Label);
    assert_eq!(dest, src, "나누기 모드는 원본 폴더가 루트");
    let report = transfer::execute(&transfer::TransferRequest {
        entries: &entries,
        labels: vec![Label::Pick, Label::Reject],
        stars: vec![],
        tags: vec![],
        action: transfer::Action::Copy,
        companions: transfer::Companions::Both,
        dest: dest.clone(),
        split: transfer::TransferSplit::Label,
        conflict: transfer::ConflictPolicy::AutoIncrement,
        rename: None,
    });
    assert_eq!(report.transferred, 2);
    assert!(report.failed.is_empty(), "{:?}", report.failed);
    let found = |name: &str| {
        walk(&dest).into_iter().filter(|p| p.file_name().is_some_and(|n| n == name)).count()
    };
    assert_eq!(found("A.JPG"), 2, "원본 + 라벨 하위폴더 사본");
    assert_eq!(found("B.JPG"), 2);
}

fn walk(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            out.extend(walk(&p));
        } else {
            out.push(p);
        }
    }
    out
}

#[test]
fn e2e_heic_thumb_and_cull_mark_not_orig() {
    // #109: 썸네일 항목으로 답한 결과는 ORIG 요청이어도 원본으로 표시하면 안 된다.
    let jpeg = jpeg_bytes(320, 240);
    let heif = heif_hvc1_with_jpeg_thumb(&jpeg);
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("iphone.heic");
    std::fs::write(&p, &heif).unwrap();
    let img = decode::decode_file(&p, decode::DecodeOptions { full_raw: true, max_edge: Some(320) })
        .expect("thumb item");
    assert!(!img.full_raw);
}

// ───────────── AI 컬링 판정 근거(#91): 근거 API가 실제 판정과 어긋나지 않는지 ─────────────

use rawblow_core::cull_ext::{self, CullItem, GroupCullParams};
use rawblow_core::quality::{self, CheckKind, CullCriteria, QualityReport, Verdict};

fn report(sharp: f32, expo: f32, tilt: f32, tilt_conf: f32, aesthetic: Option<f32>) -> QualityReport {
    QualityReport {
        exposure: quality::ExposureReport {
            mean: 0.5,
            highlight_clip: 0.0,
            shadow_clip: 0.0,
            dynamic_range: 0.8,
            score: expo,
        },
        focus: quality::FocusReport { sharpness: sharp, acutance: 0.0, in_focus: sharp > 0.3 },
        tilt: quality::TiltReport { degrees: tilt, confidence: tilt_conf },
        aesthetic,
        face: None,
        sharp_ai: None,
        object_match: None,
    }
}

#[test]
fn e2e_cull_checks_match_verdict() {
    let crit = CullCriteria { use_aesthetic: true, ..CullCriteria::default() };
    let grid = [0.05f32, 0.2, 0.5, 0.9];
    for &s in &grid {
        for &e in &grid {
            for &(t, conf) in &[(0.5f32, 0.9f32), (8.0, 0.9), (8.0, 0.05), (-6.0, 0.5)] {
                for &a in &[None, Some(0.2f32), Some(0.9)] {
                    let q = report(s, e, t, conf, a);
                    let checks = crit.checks(&q);
                    let any_fail = checks.iter().any(|c| c.fail);
                    assert_eq!(crit.verdict(&q) == Verdict::Bad, any_fail, "s={s} e={e} t={t} a={a:?}");
                    // 신뢰도 낮은 기울기·점수 없는 미적은 근거에 나오지 않는다(평가 안 함).
                    assert_eq!(checks.iter().any(|c| c.kind == CheckKind::Tilt), conf >= 0.2);
                    assert_eq!(checks.iter().any(|c| c.kind == CheckKind::Aesthetic), a.is_some());
                }
            }
        }
    }
}

#[test]
fn e2e_aesthetic_rank_matches_top_n() {
    let mut results: Vec<(usize, Verdict, Option<f32>)> = vec![
        (10, Verdict::Bad, Some(0.4)),
        (11, Verdict::Good, Some(0.9)),
        (12, Verdict::Good, None),
        (13, Verdict::Good, Some(0.7)),
        (14, Verdict::Bad, Some(0.95)),
    ];
    let ranks = quality::aesthetic_ranks(&results);
    assert_eq!(ranks.get(&14), Some(&1));
    assert_eq!(ranks.get(&11), Some(&2));
    assert_eq!(ranks.get(&12), None, "점수 없음 = 순위 없음");
    quality::finalize_cull_verdicts(&mut results, true, 2, 0.5);
    for (i, v, _) in &results {
        let in_top = ranks.get(i).is_some_and(|&r| r <= 2);
        assert_eq!(*v == Verdict::Good, in_top, "item {i}");
    }
}

#[test]
fn e2e_group_explain_matches_group_verdicts() {
    // 연사 3장(같은 시각대) + 떨어진 1장 + 메타 제외 1장. 연사는 rank 상위 1장만 유지.
    let meta = |iso: u32| cull_ext::PhotoMeta { iso: Some(iso), ..Default::default() };
    let mk = |good, rank, t: i64, iso| CullItem { good, rank, dhash: None, shot_time: Some(t), meta: meta(iso) };
    let items = vec![
        mk(true, 0.3, 100, 100),
        mk(true, 0.9, 101, 100),
        mk(true, 0.5, 102, 100),
        mk(true, 0.8, 5000, 100),
        mk(true, 0.99, 103, 6400),
    ];
    let mut p = GroupCullParams {
        use_meta: true,
        meta_filter: Default::default(),
        use_burst: true,
        burst_gap_secs: 5,
        burst_keep: 1,
        use_dedup: false,
        dedup_hamming: 6,
        dedup_keep: 1,
    };
    p.meta_filter.iso_max = Some(3200);
    let verdicts = cull_ext::apply_group_culling(&items, &p);
    let explained = cull_ext::explain_group_culling(&items, &p);
    for (v, o) in verdicts.iter().zip(&explained) {
        assert_eq!(*v, o.included.then_some(o.good));
    }
    assert_eq!(verdicts, vec![Some(false), Some(true), Some(false), Some(true), None]);
    let b0 = explained[0].burst.expect("burst rank");
    assert_eq!((b0.rank, b0.size, b0.keep), (3, 3, 1));
    assert!(b0.demoted());
    assert_eq!(explained[1].burst.map(|r| r.rank), Some(1));
    assert_eq!(explained[3].burst, None, "혼자인 컷은 그룹 근거 없음");
    assert!(!explained[4].included);
}

/// 좌상단만 빨간 합성 JPEG(w×h). 방향 검사용.
fn marked_jpeg(w: u32, h: u32) -> Vec<u8> {
    let buf = ImageBuffer::from_fn(w, h, |x, y| {
        if x < w / 4 && y < h / 4 { Rgb([250, 10, 10]) } else { Rgb([10, 10, 250]) }
    });
    let mut out = Cursor::new(Vec::new());
    image::DynamicImage::ImageRgb8(buf).write_to(&mut out, image::ImageFormat::Jpeg).unwrap();
    out.into_inner()
}

/// 디코드 결과에서 빨간 표식이 있는 모서리: (오른쪽?, 아래?).
fn red_corner(img: &decode::DecodedImage) -> (bool, bool) {
    let px = |x: u32, y: u32| {
        let i = ((y * img.width + x) * 4) as usize;
        img.rgba[i] > 128 && img.rgba[i + 2] < 128
    };
    let (w, h) = (img.width, img.height);
    let corners = [(false, false, 1, 1), (true, false, w - 2, 1), (false, true, 1, h - 2), (true, true, w - 2, h - 2)];
    let hits: Vec<(bool, bool)> = corners.iter().filter(|c| px(c.2, c.3)).map(|c| (c.0, c.1)).collect();
    assert_eq!(hits.len(), 1, "표식 모서리가 하나여야 함: {hits:?}");
    hits[0]
}

#[test]
fn e2e_jpeg_in_heif_applies_irot_and_imir() {
    // JPEG primary는 heif-oxide를 안 거치므로 컨테이너 회전을 직접 적용해야 한다(세로 사진이 눕지 않게).
    let jpeg = marked_jpeg(64, 32);
    let dir = tempfile::tempdir().unwrap();
    let dec = |name: &str, props: &[Vec<u8>]| {
        let p = dir.path().join(name);
        std::fs::write(&p, heif_with_primary_props(&jpeg, 2, props)).unwrap();
        decode::decode_file(&p, decode::DecodeOptions { full_raw: true, max_edge: Some(1920) }).expect(name)
    };
    let none = dec("none.heic", &[]);
    assert_eq!((none.width, none.height), (64, 32));
    assert_eq!(red_corner(&none), (false, false));

    // irot angle=1: 반시계 90° → 세로로 서고, 좌상단 표식은 좌하단으로.
    let ccw = dec("ccw.heic", &[bx(b"irot", &[1])]);
    assert_eq!((ccw.width, ccw.height), (32, 64));
    assert_eq!(red_corner(&ccw), (false, true));

    // irot angle=3: 시계 90° → 표식은 우상단으로.
    let cw = dec("cw.heic", &[bx(b"irot", &[3])]);
    assert_eq!((cw.width, cw.height), (32, 64));
    assert_eq!(red_corner(&cw), (true, false));

    // imir axis=1: 좌우 반전 → 우상단. axis=0: 상하 반전 → 좌하단(23008-12:2022).
    let mir = dec("mir.heic", &[bx(b"imir", &[1])]);
    assert_eq!((mir.width, mir.height), (64, 32));
    assert_eq!(red_corner(&mir), (true, false));
    let mirv = dec("mirv.heic", &[bx(b"imir", &[0])]);
    assert_eq!(red_corner(&mirv), (false, true));

    // 순서: 반시계 90 후 좌우 반전 → 좌하단이 우하단으로.
    let both = dec("both.heic", &[bx(b"irot", &[1]), bx(b"imir", &[1])]);
    assert_eq!((both.width, both.height), (32, 64));
    assert_eq!(red_corner(&both), (true, true));
}

#[test]
fn e2e_split_subfolder_pair_scan_sidecar_transfer() {
    // jpg/ · 원본/처럼 임의 이름의 하위 폴더에 나뉜 RAW+JPG도 한 항목으로 분류·전송된다.
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("shoot");
    let jpg = src.join("jpg");
    let raw = src.join("원본");
    std::fs::create_dir_all(&jpg).unwrap();
    std::fs::create_dir_all(&raw).unwrap();
    for n in 4..=7 {
        write_jpeg(&jpg.join(format!("DAZ_000{n}.JPG")), 64, 48);
        std::fs::write(raw.join(format!("DAZ_000{n}.NEF")), b"raw-bytes").unwrap();
    }

    let mut entries = scan::scan_folder(&src, true, rawblow_core::SortOrder::Name);
    assert_eq!(entries.len(), 4, "하위 폴더 분리 RAW+JPG는 번호당 한 항목");
    for e in &entries {
        assert!(e.shows_raw_badge(), "{} RAW+ 배지", e.stem);
        assert_eq!(e.members_of_kind(Kind::Raw).len(), 1);
        assert_eq!(e.display.extension().unwrap().to_ascii_uppercase(), "JPG");
    }

    entries[0].label = Label::Pick;
    entries[1].label = Label::Reject;
    sidecar::save(&src, &entries).unwrap();
    let mut reloaded = scan::scan_folder(&src, true, rawblow_core::SortOrder::Name);
    let session = sidecar::load(&src).expect("session");
    sidecar::apply(&session, &mut reloaded, &src);
    assert_eq!(reloaded[0].label, Label::Pick);
    assert_eq!(reloaded[1].label, Label::Reject);

    let dest = tmp.path().join("out");
    let picks: Vec<Entry> = reloaded
        .iter()
        .filter(|e| e.label == Label::Pick)
        .cloned()
        .collect();
    let report = transfer::execute(&transfer::TransferRequest {
        entries: &picks,
        labels: vec![Label::Pick],
        stars: vec![],
        tags: vec![],
        action: transfer::Action::Copy,
        companions: transfer::Companions::Both,
        dest: dest.clone(),
        split: transfer::TransferSplit::None,
        conflict: transfer::ConflictPolicy::AutoIncrement,
        rename: None,
    });
    assert_eq!(report.transferred, 2, "다른 폴더의 NEF도 동반 전송");
    assert!(dest.join("DAZ_0004.JPG").exists());
    assert!(dest.join("DAZ_0004.NEF").exists());
}
