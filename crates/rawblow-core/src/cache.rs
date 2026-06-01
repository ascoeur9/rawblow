//! 썸네일 디스크 캐시 (#22).
//!
//! 그리드/필름스트립 썸네일을 OS 캐시 폴더에 JPEG로 보관해, 폴더를 닫았다 다시 열어도
//! 재디코딩 없이 즉시 표시한다. 캐시 키는 `(포맷버전 + 절대경로 + 파일크기 + 수정시각 +
//! 최대변)`의 해시 — 원본 파일이 바뀌면 키가 달라져 자동으로 재디코딩된다(stale 방지).
//! 저장은 임시파일 → rename 으로 원자적이라, 동시 디코딩/중도 종료에도 깨진 파일이 안 남는다.
//!
//! 캐시는 best-effort다: 읽기/쓰기 실패는 모두 무시하고 정상 디코딩 경로로 폴백한다.

use crate::decode::DecodedImage;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// 캐시 포맷 버전. 썸네일 생성 방식이 바뀌면 올려서 기존 캐시를 무효화한다.
const CACHE_VERSION: u32 = 1;
/// 저장 JPEG 품질(썸네일이라 85면 충분히 작고 깨끗하다). env `RB_CACHE_Q`로 스윕 가능.
fn jpeg_quality() -> u8 {
    static C: std::sync::OnceLock<u8> = std::sync::OnceLock::new();
    *C.get_or_init(|| {
        std::env::var("RB_CACHE_Q")
            .ok()
            .and_then(|s| s.parse::<u8>().ok())
            .filter(|&q| q >= 1 && q <= 100)
            .unwrap_or(85)
    })
}

/// 임시파일 이름 충돌 방지용 전역 시퀀스(같은 키를 여러 스레드가 동시에 쓰는 경우 대비).
static TMP_SEQ: AtomicU64 = AtomicU64::new(0);
/// 정리(trim)가 동시에 여러 번 도는 것을 막는 플래그(중복 삭제·과다 제거 방지).
static TRIM_RUNNING: AtomicBool = AtomicBool::new(false);

/// FNV-1a(64비트) 누산. std DefaultHasher와 달리 알고리즘이 코드에 고정돼 있어,
/// Rust 툴체인/표준라이브러리 버전이 바뀌어도 같은 입력은 항상 같은 키를 낸다(재시작·업그레이드 후에도 캐시 유지).
fn fnv1a(h: &mut u64, bytes: &[u8]) {
    for &b in bytes {
        *h ^= b as u64;
        *h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
}

/// 파일 신원(경로+크기+수정시각)과 썸네일 변 길이로부터 안정적인 캐시 키(16진수)를 만든다.
/// 메타데이터를 못 읽으면 None → 캐시 비활성(정상 디코딩).
pub fn thumb_key(path: &Path, max_edge: u32) -> Option<String> {
    let meta = std::fs::metadata(path).ok()?;
    let len = meta.len();
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    fnv1a(&mut h, &CACHE_VERSION.to_le_bytes());
    fnv1a(&mut h, path.to_string_lossy().as_bytes());
    fnv1a(&mut h, &len.to_le_bytes());
    fnv1a(&mut h, &mtime.to_le_bytes());
    fnv1a(&mut h, &max_edge.to_le_bytes());
    Some(format!("{h:016x}"))
}

fn entry_path(cache_dir: &Path, key: &str) -> PathBuf {
    cache_dir.join(format!("{key}.jpg"))
}

/// 캐시에 해당 키 항목이 존재하는지 메타데이터만으로 확인한다(디코딩 없음).
/// 백그라운드 프리페치(#1·#2)에서 이미 캐시된 썸네일의 재디코딩을 건너뛰는 데 쓴다.
pub fn exists(cache_dir: &Path, key: &str) -> bool {
    std::fs::metadata(entry_path(cache_dir, key))
        .map(|m| m.is_file())
        .unwrap_or(false)
}

/// 캐시에서 썸네일을 읽는다(없거나 파손 시 None).
pub fn load(cache_dir: &Path, key: &str) -> Option<DecodedImage> {
    let bytes = std::fs::read(entry_path(cache_dir, key)).ok()?;
    let rgba = image::load_from_memory(&bytes).ok()?.to_rgba8();
    let (width, height) = (rgba.width(), rgba.height());
    if width == 0 || height == 0 {
        return None;
    }
    Some(DecodedImage {
        width,
        height,
        rgba: rgba.into_raw(),
        color_managed: true, // 저장 시점에 이미 sRGB로 매니징됨
        full_raw: false,
    })
}

/// 썸네일을 캐시에 JPEG로 저장한다(임시파일 → rename, 원자적). 실패는 무시한다.
pub fn store(cache_dir: &Path, key: &str, img: &DecodedImage) {
    if img.width == 0 || img.height == 0 {
        return;
    }
    let expected = (img.width as usize) * (img.height as usize) * 4;
    if img.rgba.len() != expected {
        return; // 손상된 버퍼는 저장하지 않는다.
    }
    if std::fs::create_dir_all(cache_dir).is_err() {
        return;
    }
    // RGBA → RGB. 썸네일은 불투명 사진이라 알파 제거는 무손실 의미다.
    let mut rgb = Vec::with_capacity((img.width as usize) * (img.height as usize) * 3);
    for px in img.rgba.chunks_exact(4) {
        rgb.extend_from_slice(&px[..3]);
    }
    let mut buf = Vec::new();
    {
        let mut enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, jpeg_quality());
        if enc
            .encode(&rgb, img.width, img.height, image::ExtendedColorType::Rgb8)
            .is_err()
        {
            return;
        }
    }
    let seq = TMP_SEQ.fetch_add(1, Ordering::Relaxed);
    let tmp_path = cache_dir.join(format!(".tmp-{}-{}", std::process::id(), seq));
    if std::fs::write(&tmp_path, &buf).is_ok() {
        if std::fs::rename(&tmp_path, entry_path(cache_dir, key)).is_err() {
            let _ = std::fs::remove_file(&tmp_path); // rename 실패 시 임시파일 청소.
        }
    }
}

/// 캐시 폴더의 총 바이트 수(파일 합). 폴더가 없으면 0.
pub fn dir_size(cache_dir: &Path) -> u64 {
    let mut total = 0u64;
    if let Ok(rd) = std::fs::read_dir(cache_dir) {
        for e in rd.flatten() {
            if let Ok(m) = e.metadata() {
                if m.is_file() {
                    total += m.len();
                }
            }
        }
    }
    total
}

/// 캐시가 `max_bytes`를 초과하면 mtime이 오래된 `.jpg`부터 지워 상한 이하로 맞춘다(best-effort).
/// `max_bytes == 0`이면 무제한(아무 것도 안 함). 동시 호출은 1개만 실행되고 나머지는 즉시 반환한다.
/// (현재 보고 있는 폴더의 썸네일은 최근에 쓰여 mtime이 최신이라 가장 늦게 제거된다.)
pub fn trim(cache_dir: &Path, max_bytes: u64) {
    if max_bytes == 0 {
        return; // 무제한.
    }
    if TRIM_RUNNING.swap(true, Ordering::AcqRel) {
        return; // 이미 정리 중.
    }
    let mut files: Vec<(PathBuf, u64, std::time::SystemTime)> = Vec::new();
    let mut total: u64 = 0;
    if let Ok(rd) = std::fs::read_dir(cache_dir) {
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().and_then(|s| s.to_str()) != Some("jpg") {
                continue; // 캐시 .jpg만 대상(.tmp-* 등 제외).
            }
            if let Ok(m) = e.metadata() {
                if m.is_file() {
                    total += m.len();
                    files.push((p, m.len(), m.modified().unwrap_or(std::time::UNIX_EPOCH)));
                }
            }
        }
    }
    if total > max_bytes {
        files.sort_by_key(|(_, _, t)| *t); // 오래된 것 먼저.
        for (p, sz, _) in files {
            if total <= max_bytes {
                break;
            }
            if std::fs::remove_file(&p).is_ok() {
                total = total.saturating_sub(sz);
            }
        }
    }
    TRIM_RUNNING.store(false, Ordering::Release);
}

/// 캐시 파일을 모두 지운다(폴더 자체는 남긴다). 폴더가 없어도 Ok.
pub fn clear(cache_dir: &Path) -> std::io::Result<()> {
    if let Ok(rd) = std::fs::read_dir(cache_dir) {
        for e in rd.flatten() {
            let p = e.path();
            if p.is_file() {
                let _ = std::fs::remove_file(p);
            }
        }
    }
    Ok(())
}
