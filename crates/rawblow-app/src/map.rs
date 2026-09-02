//! GPS 위치 미니 지도(#38). 촬영 좌표 중심의 OSM 정적 지도를 합성한다.
//! 타일은 디스크 캐시 → 네트워크(ureq, `update-check` 기능과 의존성 공유) 순으로
//! 얻는다. OSMF 타일 정책: User-Agent 명시, "© OpenStreetMap contributors"
//! attribution(표시는 호출부 패널에서), 소량 요청만(마커 주변 최대 6장).
//! 동기 HTTP이므로 `compose`는 반드시 백그라운드 스레드에서 호출한다.

use std::path::{Path, PathBuf};

pub const TILE: f64 = 256.0;

/// Web Mercator: (위도, 경도, 줌) → OSM 타일 좌표(소수부 포함).
pub fn tile_xy(lat: f64, lon: f64, z: u8) -> (f64, f64) {
    let n = 2f64.powi(z as i32);
    let x = (lon + 180.0) / 360.0 * n;
    let lat_r = lat.to_radians();
    let y = (1.0 - (lat_r.tan() + 1.0 / lat_r.cos()).ln() / std::f64::consts::PI) / 2.0 * n;
    (x, y)
}

/// 외부 브라우저용 OSM 링크(마커 포함).
pub fn osm_url(lat: f64, lon: f64, z: u8) -> String {
    format!("https://www.openstreetmap.org/?mlat={lat}&mlon={lon}#map={z}/{lat}/{lon}")
}

pub struct MapImage {
    pub w: u32,
    pub h: u32,
    pub rgba: Vec<u8>,
}

/// 좌표 중심 `w`×`h` 픽셀 지도 합성. 타일 일부 실패는 회색으로 남기고, 전부
/// 실패(오프라인 + 캐시 없음)면 None. 마커는 호출부가 패널 중앙에 그린다.
pub fn compose(lat: f64, lon: f64, z: u8, w: u32, h: u32, cache: &Path) -> Option<MapImage> {
    let (tx, ty) = tile_xy(lat, lon, z);
    let (cx, cy) = (tx * TILE, ty * TILE);
    let left = cx - w as f64 / 2.0;
    let top = cy - h as f64 / 2.0;
    let n = 1i64 << z;
    let x0 = (left / TILE).floor() as i64;
    let y0 = (top / TILE).floor() as i64;
    let x1 = ((left + w as f64 - 1.0) / TILE).floor() as i64;
    let y1 = ((top + h as f64 - 1.0) / TILE).floor() as i64;
    // 배경: 타일 실패 시 보이는 어두운 회색.
    let mut out = vec![0u8; (w * h * 4) as usize];
    for px in out.chunks_exact_mut(4) {
        px.copy_from_slice(&[0x14, 0x17, 0x1c, 0xff]);
    }
    let mut any = false;
    for tyi in y0..=y1 {
        if tyi < 0 || tyi >= n {
            continue; // 극지방 밖.
        }
        for txi in x0..=x1 {
            let wx = txi.rem_euclid(n) as u32; // 경도 래핑.
            let Some(png) = tile_bytes(z, wx, tyi as u32, cache) else { continue };
            let Ok(img) = image::load_from_memory(&png) else { continue };
            let tile = img.to_rgba8();
            blit(&mut out, w, h, &tile, (txi as f64 * TILE - left).round() as i64, (tyi as f64 * TILE - top).round() as i64);
            any = true;
        }
    }
    any.then_some(MapImage { w, h, rgba: out })
}

/// 타일 RGBA를 출력 버퍼에 복사(클리핑).
fn blit(out: &mut [u8], ow: u32, oh: u32, tile: &image::RgbaImage, ox: i64, oy: i64) {
    let (tw, th) = (tile.width() as i64, tile.height() as i64);
    for ty in 0..th {
        let dy = oy + ty;
        if dy < 0 || dy >= oh as i64 {
            continue;
        }
        let sx0 = (-ox).clamp(0, tw);
        let sx1 = (ow as i64 - ox).clamp(0, tw);
        if sx0 >= sx1 {
            continue;
        }
        let src = &tile.as_raw()[((ty * tw + sx0) * 4) as usize..((ty * tw + sx1) * 4) as usize];
        let doff = ((dy * ow as i64 + ox + sx0) * 4) as usize;
        out[doff..doff + src.len()].copy_from_slice(src);
    }
}

/// 지도 타일 디스크 캐시 디렉토리(썸네일 캐시 옆 `map-cache`).
pub fn cache_dir() -> PathBuf {
    let thumb = rawblow_core::config::cache_dir();
    thumb.parent().map(|p| p.join("map-cache")).unwrap_or_else(|| thumb.join("map-cache"))
}

/// 타일 PNG 바이트: 캐시 우선, 없으면 네트워크(성공 시 캐시 저장).
fn tile_bytes(z: u8, x: u32, y: u32, cache: &Path) -> Option<Vec<u8>> {
    let file = cache.join(format!("{z}_{x}_{y}.png"));
    if let Ok(b) = std::fs::read(&file) {
        if !b.is_empty() {
            return Some(b);
        }
    }
    let b = fetch_tile(z, x, y)?;
    let _ = std::fs::create_dir_all(cache);
    let _ = std::fs::write(&file, &b);
    let dir = cache.to_path_buf();
    std::thread::spawn(move || {
        rawblow_core::cache::trim_ext(&dir, 256 * 1024 * 1024, "png");
    });
    Some(b)
}

/// OSM 타일 HTTP GET. ureq는 `update-check` 기능에 묶인 optional 의존성이라
/// 같은 게이트를 공유한다(끈 빌드는 캐시 전용 = 사실상 미표시).
#[cfg(feature = "update-check")]
fn fetch_tile(z: u8, x: u32, y: u32) -> Option<Vec<u8>> {
    use std::io::Read;
    use std::time::Duration;
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(5))
        .timeout_read(Duration::from_secs(8))
        .build();
    let resp = agent
        .get(&format!("https://tile.openstreetmap.org/{z}/{x}/{y}.png"))
        .set("User-Agent", concat!("RawBlow/", env!("CARGO_PKG_VERSION"), " (photo viewer; github.com/ascoeur9/rawblow)"))
        .call()
        .ok()?;
    let mut buf = Vec::new();
    resp.into_reader().take(2 * 1024 * 1024).read_to_end(&mut buf).ok()?;
    (!buf.is_empty()).then_some(buf)
}

#[cfg(not(feature = "update-check"))]
fn fetch_tile(_z: u8, _x: u32, _y: u32) -> Option<Vec<u8>> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 타일 수식 검증: 알려진 기준점들(OSM slippy map 문서 예시와 동일 수식).
    #[test]
    fn tile_math_known_points() {
        // (0,0)은 모든 줌에서 정중앙 타일.
        let (x, y) = tile_xy(0.0, 0.0, 1);
        assert!((x - 1.0).abs() < 1e-9 && (y - 1.0).abs() < 1e-9);
        // 서울시청 근방(37.5665, 126.9780), z=13: x=(126.978+180)/360*8192≈6985.5.
        let (x, y) = tile_xy(37.5665, 126.9780, 13);
        assert_eq!(x.floor() as u32, 6985);
        assert_eq!(y.floor() as u32, 3172);
        // 남반구는 y가 절반(2^z/2)보다 아래.
        let (_, ys) = tile_xy(-33.8678, 151.21, 10);
        assert!(ys > 512.0);
    }

    /// 라이브 네트워크 통합(수동): OSM 타일을 실제로 받아 합성·캐시되는지.
    /// 실행: `cargo test -p rawblow-app map_compose_live -- --ignored`
    #[test]
    #[ignore = "network"]
    fn map_compose_live() {
        let dir = std::env::temp_dir().join("rb-map-test");
        let img = compose(37.5665, 126.978, 13, 220, 150, &dir).expect("타일 합성");
        assert_eq!((img.w, img.h), (220, 150));
        // 단색 배경이 아니라 실제 타일 픽셀이 박혔는지(색 다양성).
        let mut distinct = std::collections::HashSet::new();
        for px in img.rgba.chunks_exact(4) {
            distinct.insert([px[0], px[1], px[2]]);
        }
        assert!(distinct.len() > 16, "지도 픽셀 다양성 {}", distinct.len());
        // 두 번째 호출은 캐시에서(네트워크 무관) 동일 결과.
        let img2 = compose(37.5665, 126.978, 13, 220, 150, &dir).expect("캐시 합성");
        assert_eq!(img.rgba, img2.rgba);
    }

    /// blit 클리핑: 출력 밖으로 나가는 타일이 패닉 없이 잘려 들어가는지.
    #[test]
    fn blit_clips() {
        let mut out = vec![0u8; 64 * 64 * 4];
        let tile = image::RgbaImage::from_pixel(256, 256, image::Rgba([255, 0, 0, 255]));
        blit(&mut out, 64, 64, &tile, -200, -200); // 우하단 일부만 걸침.
        blit(&mut out, 64, 64, &tile, 60, 60); // 좌상단 일부만 걸침.
        assert_eq!(&out[0..4], &[255, 0, 0, 255]); // (0,0)은 첫 blit으로 칠해짐.
        let last = (63 * 64 + 63) * 4;
        assert_eq!(&out[last..last + 4], &[255, 0, 0, 255]);
    }
}
