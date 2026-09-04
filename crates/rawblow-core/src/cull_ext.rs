//! 컬링 확장 기준(#50 후속): 기존 CV/미적 점수 외에 **추가 인수**로 거를 수 있는 신호들.
//! 가능 여부 검증용으로 코어 로직을 자족적으로 구현하고 테스트한다(UI 배선은 별도).
//!
//! - Tier1: EXIF 메타데이터 기준 — 방향(세로/가로), ISO·셔터·조리개·초점거리, 카메라/렌즈.
//! - Tier2: 연사(버스트) 그룹별 베스트-N — 촬영시각으로 묶고 점수로 그룹 내 상위 N장만 채택.
//! - Tier3a: 시각적 근접중복 클러스터링 — dHash(퍼셉추얼 해시)로 유사컷을 묶어 베스트만.
//!
//! 모두 모델 불필요(순수 CV/메타). CLIP 임베딩 기반 의미적 클러스터링·프롬프트 축은 `cull_clip`(별도).

use crate::decode::DecodedImage;
use std::collections::HashSet;

// ───────────────────────── Tier1: EXIF 수치 파싱 ─────────────────────────
// ExifInfo는 표시용 문자열(kamadak-exif display)이라, 비교를 위해 수치로 푼다.

/// "6400" / "ISO 6400" → 6400. 숫자만 추출.
pub fn parse_iso(s: &str) -> Option<u32> {
    let digits: String = s.chars().filter(|c| c.is_ascii_digit()).collect();
    digits.parse().ok().filter(|&v| v > 0)
}

/// "f/2.8" / "2.8" / "F2.8" → 2.8.
pub fn parse_aperture(s: &str) -> Option<f32> {
    let t = s.trim().trim_start_matches(['f', 'F', '/']);
    t.split_whitespace().next()?.parse().ok()
}

/// "85 mm" / "85.0 mm" / "85mm" → 85.0.
pub fn parse_focal_mm(s: &str) -> Option<f32> {
    let num: String = s
        .trim()
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    num.parse().ok()
}

/// 노출시간 → 초. "1/250" / "1/250 s" / "0.5 s" / "0.004" 모두 지원.
pub fn parse_shutter_secs(s: &str) -> Option<f32> {
    let t = s.trim().trim_end_matches(['s', ' ']).trim();
    if let Some((n, d)) = t.split_once('/') {
        let n: f32 = n.trim().parse().ok()?;
        let d: f32 = d.trim().parse().ok()?;
        if d != 0.0 {
            return Some(n / d);
        }
        return None;
    }
    t.parse().ok()
}

/// EXIF datetime("YYYY:MM:DD HH:MM:SS") → epoch 초(UTC 무관, **상대 비교용**).
/// 파싱 실패/형식 불일치는 None. SubSec은 미지원(초 해상도) — 연사 정밀화는 별도.
pub fn parse_exif_datetime(s: &str) -> Option<i64> {
    let (date, time) = s.trim().split_once(' ')?;
    let mut d = date.split([':', '-']);
    let (y, mo, da) = (
        d.next()?.parse::<i64>().ok()?,
        d.next()?.parse::<i64>().ok()?,
        d.next()?.parse::<i64>().ok()?,
    );
    let mut t = time.split(':');
    let (h, mi, se) = (
        t.next()?.parse::<i64>().ok()?,
        t.next()?.parse::<i64>().ok()?,
        t.next().unwrap_or("0").parse::<i64>().ok()?,
    );
    if !(1..=12).contains(&mo) || !(1..=31).contains(&da) {
        return None;
    }
    Some(days_from_civil(y, mo, da) * 86_400 + h * 3600 + mi * 60 + se)
}

/// Howard Hinnant 알고리즘: (y,m,d) → epoch(1970-01-01) 기준 일수.
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = (if y >= 0 { y } else { y - 399 }) / 400;
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// 사진 한 장의 컬링용 수치 메타(전부 옵션 — 없으면 해당 기준은 통과로 간주).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PhotoMeta {
    pub iso: Option<u32>,
    pub aperture_f: Option<f32>,
    pub shutter_secs: Option<f32>,
    pub focal_mm: Option<f32>,
    pub datetime_secs: Option<i64>,
    /// **화면에 보이는 방향의** 가로·세로 픽셀. `from_exif`가 EXIF Orientation을 적용해
    /// 채우므로(센서 기준 값이 아니다) `is_portrait`가 실제 표시와 일치한다.
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub camera: Option<String>,
    pub lens: Option<String>,
}

impl PhotoMeta {
    /// 표시용 ExifInfo 문자열을 수치로 파싱해 채운다.
    pub fn from_exif(e: &crate::meta::ExifInfo) -> Self {
        // EXIF의 width/height는 센서 기준(미회전)이라 세로 컷도 가로 숫자로 들어온다.
        // 그대로 쓰면 "세로만" 필터가 세로 사진을 전부 걸러낸다 — 표시 방향으로 바꿔 담는다.
        let (w, h) = match e.display_size() {
            Some((w, h)) => (Some(w), Some(h)),
            None => (e.width, e.height),
        };
        PhotoMeta {
            iso: e.iso.as_deref().and_then(parse_iso),
            aperture_f: e.aperture.as_deref().and_then(parse_aperture),
            shutter_secs: e.shutter.as_deref().and_then(parse_shutter_secs),
            focal_mm: e.focal_length.as_deref().and_then(parse_focal_mm),
            datetime_secs: e.datetime.as_deref().and_then(parse_exif_datetime),
            width: w,
            height: h,
            camera: e.camera.clone(),
            lens: e.lens.clone(),
        }
    }

    /// 세로(true)/가로(false). width/height 없으면 None.
    pub fn is_portrait(&self) -> Option<bool> {
        match (self.width, self.height) {
            (Some(w), Some(h)) if w != h => Some(h > w),
            _ => None,
        }
    }
}

/// 방향 기준.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Orientation {
    Portrait,
    Landscape,
}

/// Tier1 메타 필터(모든 필드 옵션 = 무제한). `passes`가 false면 그 컷 제외(또는 Bad).
#[derive(Debug, Clone, Default)]
pub struct MetaFilter {
    pub orientation: Option<Orientation>,
    pub iso_max: Option<u32>,
    pub iso_min: Option<u32>,
    pub aperture_max: Option<f32>, // 조리개 f값 상한(밝은 렌즈만: f<=값)
    pub aperture_min: Option<f32>,
    pub shutter_min_secs: Option<f32>, // 손떨림 거르기: 1/focal 등 하한
    pub shutter_max_secs: Option<f32>,
    pub focal_min_mm: Option<f32>,
    pub focal_max_mm: Option<f32>,
    pub camera_contains: Option<String>,
    pub lens_contains: Option<String>,
}

impl MetaFilter {
    /// 메타 기준 통과 여부. 값이 없는(미상) 항목은 **통과**로 본다(과잉 제외 방지).
    pub fn passes(&self, m: &PhotoMeta) -> bool {
        if let Some(o) = self.orientation {
            if let Some(p) = m.is_portrait() {
                let want_portrait = o == Orientation::Portrait;
                if p != want_portrait {
                    return false;
                }
            }
        }
        if let (Some(max), Some(iso)) = (self.iso_max, m.iso) {
            if iso > max {
                return false;
            }
        }
        if let (Some(min), Some(iso)) = (self.iso_min, m.iso) {
            if iso < min {
                return false;
            }
        }
        if let (Some(max), Some(a)) = (self.aperture_max, m.aperture_f) {
            if a > max {
                return false;
            }
        }
        if let (Some(min), Some(a)) = (self.aperture_min, m.aperture_f) {
            if a < min {
                return false;
            }
        }
        if let (Some(min), Some(s)) = (self.shutter_min_secs, m.shutter_secs) {
            if s < min {
                return false;
            }
        }
        if let (Some(max), Some(s)) = (self.shutter_max_secs, m.shutter_secs) {
            if s > max {
                return false;
            }
        }
        if let (Some(min), Some(f)) = (self.focal_min_mm, m.focal_mm) {
            if f < min {
                return false;
            }
        }
        if let (Some(max), Some(f)) = (self.focal_max_mm, m.focal_mm) {
            if f > max {
                return false;
            }
        }
        if let (Some(sub), Some(cam)) = (&self.camera_contains, &m.camera) {
            if !cam.to_lowercase().contains(&sub.to_lowercase()) {
                return false;
            }
        }
        if let (Some(sub), Some(lens)) = (&self.lens_contains, &m.lens) {
            if !lens.to_lowercase().contains(&sub.to_lowercase()) {
                return false;
            }
        }
        true
    }
}

// ───────────────────────── Tier2: 연사 그룹별 베스트-N ─────────────────────────

/// 입력 순서대로 인접 촬영시각 간격이 `max_gap_secs` 이하면 같은 버스트로 묶는다.
/// datetime 없는 항목은 단독 그룹(묶을 수 없음). 반환은 인덱스 그룹들(입력 순서 보존).
pub fn group_bursts(times: &[Option<i64>], max_gap_secs: i64) -> Vec<Vec<usize>> {
    let mut groups: Vec<Vec<usize>> = Vec::new();
    let mut cur: Vec<usize> = Vec::new();
    let mut prev: Option<i64> = None;
    for (i, t) in times.iter().enumerate() {
        match (prev, t) {
            (Some(p), Some(now)) if (now - p).abs() <= max_gap_secs => {
                cur.push(i);
            }
            _ => {
                if !cur.is_empty() {
                    groups.push(std::mem::take(&mut cur));
                }
                cur.push(i);
            }
        }
        prev = *t;
    }
    if !cur.is_empty() {
        groups.push(cur);
    }
    groups
}

/// 각 그룹에서 점수 상위 `top_n`개 인덱스를 채택(Good) 집합으로 반환. 나머지는 제외(Bad).
/// 점수 동률은 인덱스 작은 쪽(먼저 찍힌 컷) 우선.
pub fn select_best_per_group(
    groups: &[Vec<usize>],
    scores: &[f32],
    top_n: usize,
) -> HashSet<usize> {
    let mut keep = HashSet::new();
    for g in groups {
        let mut idx: Vec<usize> = g.clone();
        idx.sort_by(|&a, &b| {
            scores[b]
                .partial_cmp(&scores[a])
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(a.cmp(&b))
        });
        for &i in idx.iter().take(top_n.max(1)) {
            keep.insert(i);
        }
    }
    keep
}

// ───────────────────────── Tier3a: dHash 근접중복 클러스터링 ─────────────────────────

/// 9×8 그레이스케일로 줄여 가로 인접차분 → 64비트 difference hash. 시각적으로 유사한
/// 이미지는 해밍거리가 작다(연사·재구도·미세 변화 묶기에 적합). 모델 불필요.
pub fn dhash(img: &DecodedImage) -> u64 {
    const W: usize = 9;
    const H: usize = 8;
    if img.width == 0 || img.height == 0 {
        return 0;
    }
    // 최근접 샘플로 9×8 그레이 축소.
    let mut gray = [[0u16; W]; H];
    for (y, row) in gray.iter_mut().enumerate() {
        for (x, cell) in row.iter_mut().enumerate() {
            let sx = (x * img.width as usize / W).min(img.width as usize - 1);
            let sy = (y * img.height as usize / H).min(img.height as usize - 1);
            let p = (sy * img.width as usize + sx) * 4;
            let (r, g, b) = (img.rgba[p] as u16, img.rgba[p + 1] as u16, img.rgba[p + 2] as u16);
            *cell = (r * 30 + g * 59 + b * 11) / 100; // 휘도 근사
        }
    }
    let mut hash = 0u64;
    let mut bit = 0;
    for row in gray.iter() {
        for x in 0..W - 1 {
            if row[x] < row[x + 1] {
                hash |= 1 << bit;
            }
            bit += 1;
        }
    }
    hash
}

/// 두 해시의 해밍거리(다른 비트 수, 0..64).
pub fn hamming(a: u64, b: u64) -> u32 {
    (a ^ b).count_ones()
}

/// 해밍거리 `max_hamming` 이하를 같은 클러스터로 union-find 그룹핑. 반환은 인덱스 그룹들.
/// (연사가 아니어도 그림이 비슷하면 묶임 — 시간 기반 group_bursts의 시각적 상위호환)
pub fn cluster_near_dups(hashes: &[u64], max_hamming: u32) -> Vec<Vec<usize>> {
    let n = hashes.len();
    let mut parent: Vec<usize> = (0..n).collect();
    fn find(p: &mut [usize], x: usize) -> usize {
        let mut r = x;
        while p[r] != r {
            r = p[r];
        }
        let mut c = x;
        while p[c] != r {
            let next = p[c];
            p[c] = r;
            c = next;
        }
        r
    }
    for i in 0..n {
        for j in (i + 1)..n {
            if hamming(hashes[i], hashes[j]) <= max_hamming {
                let (ri, rj) = (find(&mut parent, i), find(&mut parent, j));
                if ri != rj {
                    parent[ri] = rj;
                }
            }
        }
    }
    let mut map: std::collections::BTreeMap<usize, Vec<usize>> = std::collections::BTreeMap::new();
    for i in 0..n {
        let r = find(&mut parent, i);
        map.entry(r).or_default().push(i);
    }
    map.into_values().collect()
}

// ───────────────────────── 그룹 컬링 통합(메타 제외 → 연사 → 중복) ─────────────────────────

/// 한 컷의 컬링 입력. `good`=CV/미적 1차 판정, `rank`=그룹 내 베스트 선택 점수(미적>선명도).
#[derive(Debug, Clone)]
pub struct CullItem {
    pub good: bool,
    pub rank: f32,
    pub dhash: Option<u64>,
    pub shot_time: Option<i64>,
    pub meta: PhotoMeta,
}

/// 그룹 컬링 파라미터(UI config에서 변환).
#[derive(Debug, Clone)]
pub struct GroupCullParams {
    pub use_meta: bool,
    pub meta_filter: MetaFilter,
    pub use_burst: bool,
    pub burst_gap_secs: i64,
    pub burst_keep: usize,
    pub use_dedup: bool,
    pub dedup_hamming: u32,
    pub dedup_keep: usize,
}

fn keep_top(orig: &[usize], ranks: &[f32], keep: usize) -> HashSet<usize> {
    let mut v = orig.to_vec();
    v.sort_by(|&a, &b| {
        ranks[b].partial_cmp(&ranks[a]).unwrap_or(std::cmp::Ordering::Equal).then(a.cmp(&b))
    });
    v.into_iter().take(keep.max(1)).collect()
}

/// 메타 필터로 대상을 좁히고(불통과=제외, 손대지 않음), 연사·시각중복 그룹마다 rank 상위 keep만
/// Good 유지(나머지는 Bad로 강등). 반환: 각 인덱스별 `None`=제외, `Some(true/false)`=Good/Bad.
pub fn apply_group_culling(items: &[CullItem], p: &GroupCullParams) -> Vec<Option<bool>> {
    let n = items.len();
    let mut incl_mask = vec![true; n];
    if p.use_meta {
        for (i, it) in items.iter().enumerate() {
            incl_mask[i] = p.meta_filter.passes(&it.meta);
        }
    }
    let included: Vec<usize> = (0..n).filter(|&i| incl_mask[i]).collect();
    let ranks: Vec<f32> = items.iter().map(|it| it.rank).collect();
    let mut good: Vec<bool> = items.iter().map(|it| it.good).collect();

    // 연사: included를 원래(촬영) 순서대로 시각 간격 그룹핑.
    if p.use_burst {
        let times: Vec<Option<i64>> = included.iter().map(|&i| items[i].shot_time).collect();
        for g in group_bursts(&times, p.burst_gap_secs) {
            let orig: Vec<usize> = g.iter().map(|&k| included[k]).collect();
            let keep = keep_top(&orig, &ranks, p.burst_keep);
            for &oi in &orig {
                if !keep.contains(&oi) {
                    good[oi] = false;
                }
            }
        }
    }
    // 시각중복: dhash 있는 included만 클러스터(없으면 단독 취급).
    if p.use_dedup {
        let valid: Vec<usize> = included.iter().cloned().filter(|&i| items[i].dhash.is_some()).collect();
        let vhash: Vec<u64> = valid.iter().map(|&i| items[i].dhash.unwrap()).collect();
        for c in cluster_near_dups(&vhash, p.dedup_hamming) {
            let orig: Vec<usize> = c.iter().map(|&k| valid[k]).collect();
            let keep = keep_top(&orig, &ranks, p.dedup_keep);
            for &oi in &orig {
                if !keep.contains(&oi) {
                    good[oi] = false;
                }
            }
        }
    }
    (0..n).map(|i| incl_mask[i].then_some(good[i])).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decode::DecodedImage;

    fn solid(w: u32, h: u32, rgb: [u8; 3]) -> DecodedImage {
        let mut rgba = vec![0u8; (w * h * 4) as usize];
        for px in rgba.as_chunks_mut::<4>().0 {
            px[0] = rgb[0];
            px[1] = rgb[1];
            px[2] = rgb[2];
            px[3] = 255;
        }
        DecodedImage { width: w, height: h, rgba, color_managed: false, full_raw: false }
    }

    fn gradient(w: u32, h: u32, seed: u32) -> DecodedImage {
        let mut rgba = vec![0u8; (w * h * 4) as usize];
        for y in 0..h {
            for x in 0..w {
                let p = ((y * w + x) * 4) as usize;
                let v = (((x ^ y).wrapping_add(seed)) % 256) as u8;
                rgba[p] = v;
                rgba[p + 1] = v;
                rgba[p + 2] = v;
                rgba[p + 3] = 255;
            }
        }
        DecodedImage { width: w, height: h, rgba, color_managed: false, full_raw: false }
    }

    // ---- Tier1: 파서 ----
    #[test]
    fn parsers_handle_kamadak_formats() {
        assert_eq!(parse_iso("6400"), Some(6400));
        assert_eq!(parse_iso("ISO 100"), Some(100));
        assert_eq!(parse_aperture("f/2.8"), Some(2.8));
        assert_eq!(parse_aperture("2.0"), Some(2.0));
        assert_eq!(parse_focal_mm("85 mm"), Some(85.0));
        assert_eq!(parse_focal_mm("24.0 mm"), Some(24.0));
        assert_eq!(parse_shutter_secs("1/250").unwrap(), 1.0 / 250.0);
        assert_eq!(parse_shutter_secs("1/250 s").unwrap(), 1.0 / 250.0);
        assert_eq!(parse_shutter_secs("0.5 s").unwrap(), 0.5);
    }

    #[test]
    fn exif_datetime_orders_correctly() {
        let a = parse_exif_datetime("2026:06:25 21:36:54").unwrap();
        let b = parse_exif_datetime("2026:06:25 21:36:55").unwrap();
        let c = parse_exif_datetime("2026:06:25 21:37:54").unwrap();
        assert_eq!(b - a, 1, "1초 차");
        assert_eq!(c - a, 60, "1분 차");
        assert!(parse_exif_datetime("garbage").is_none());
        // 날짜 경계(자정 넘김)
        let n1 = parse_exif_datetime("2026:06:25 23:59:59").unwrap();
        let n2 = parse_exif_datetime("2026:06:26 00:00:00").unwrap();
        assert_eq!(n2 - n1, 1);
    }

    // ---- Tier1: 방향/메타 필터 ----
    #[test]
    fn orientation_and_meta_filter() {
        let portrait = PhotoMeta { width: Some(4000), height: Some(6000), ..Default::default() };
        let landscape = PhotoMeta { width: Some(6000), height: Some(4000), ..Default::default() };
        assert_eq!(portrait.is_portrait(), Some(true));
        assert_eq!(landscape.is_portrait(), Some(false));

        let only_portrait = MetaFilter { orientation: Some(Orientation::Portrait), ..Default::default() };
        assert!(only_portrait.passes(&portrait));
        assert!(!only_portrait.passes(&landscape));

        // EXIF는 세로 컷도 센서 기준(가로) 숫자로 기록한다 — Orientation 6/8이면
        // from_exif가 맞바꿔 담아야 "세로만" 필터가 세로 사진을 통과시킨다.
        let rotated = crate::meta::ExifInfo {
            width: Some(6960),
            height: Some(4640),
            orientation: 8,
            ..Default::default()
        };
        let m = PhotoMeta::from_exif(&rotated);
        assert_eq!((m.width, m.height), (Some(4640), Some(6960)));
        assert_eq!(m.is_portrait(), Some(true));
        assert!(only_portrait.passes(&m));
        // Orientation 1은 그대로.
        let upright = crate::meta::ExifInfo { orientation: 1, ..rotated.clone() };
        let m = PhotoMeta::from_exif(&upright);
        assert_eq!((m.width, m.height), (Some(6960), Some(4640)));
        assert_eq!(m.is_portrait(), Some(false));

        // ISO 상한 6400: 고감도 컷 제외
        let hi = PhotoMeta { iso: Some(12800), ..Default::default() };
        let lo = PhotoMeta { iso: Some(400), ..Default::default() };
        let iso_cap = MetaFilter { iso_max: Some(6400), ..Default::default() };
        assert!(!iso_cap.passes(&hi));
        assert!(iso_cap.passes(&lo));

        // 망원만(초점거리 >= 70mm)
        let tele = PhotoMeta { focal_mm: Some(200.0), ..Default::default() };
        let wide = PhotoMeta { focal_mm: Some(24.0), ..Default::default() };
        let tele_only = MetaFilter { focal_min_mm: Some(70.0), ..Default::default() };
        assert!(tele_only.passes(&tele));
        assert!(!tele_only.passes(&wide));

        // 카메라 한정
        let m = PhotoMeta { camera: Some("LUMIX S1R II".into()), ..Default::default() };
        let lumix = MetaFilter { camera_contains: Some("lumix".into()), ..Default::default() };
        let nikon = MetaFilter { camera_contains: Some("nikon".into()), ..Default::default() };
        assert!(lumix.passes(&m));
        assert!(!nikon.passes(&m));

        // 미상 값은 통과(과잉 제외 방지)
        assert!(iso_cap.passes(&PhotoMeta::default()));
    }

    // ---- Tier2: 연사 그룹별 베스트-N ----
    #[test]
    fn burst_grouping_by_time() {
        let base = parse_exif_datetime("2026:06:25 12:00:00").unwrap();
        // 0,1,2초(연사) … 10초 갭 … 11,12초(연사) … None(시각없음)
        let times = vec![
            Some(base),
            Some(base + 1),
            Some(base + 2),
            Some(base + 12),
            Some(base + 13),
            None,
        ];
        let groups = group_bursts(&times, 2);
        assert_eq!(groups, vec![vec![0, 1, 2], vec![3, 4], vec![5]]);
    }

    #[test]
    fn best_n_per_burst() {
        // 그룹 [0,1,2] 점수 0.2/0.9/0.5, 그룹 [3,4] 점수 0.7/0.1
        let groups = vec![vec![0usize, 1, 2], vec![3, 4]];
        let scores = vec![0.2f32, 0.9, 0.5, 0.7, 0.1];
        let keep = select_best_per_group(&groups, &scores, 1);
        // 각 그룹 최고: idx1(0.9), idx3(0.7)
        assert!(keep.contains(&1) && keep.contains(&3));
        assert_eq!(keep.len(), 2);
        // top2: 그룹1에서 1,2 / 그룹2에서 3,4 모두
        let keep2 = select_best_per_group(&groups, &scores, 2);
        assert_eq!(keep2.len(), 4);
        assert!(!keep2.contains(&0), "그룹1 최하점(0.2)만 탈락");
    }

    // ---- Tier3a: dHash 근접중복 ----
    #[test]
    fn dhash_groups_similar_images() {
        let a = gradient(200, 150, 0);
        let a2 = gradient(200, 150, 0); // 동일
        let a_noisy = {
            // a와 거의 같되 미세 변화(연사 흉내): 한 줄만 살짝 바꿈
            let mut img = gradient(200, 150, 0);
            for x in 0..200usize {
                let p = (x) * 4;
                img.rgba[p] = img.rgba[p].wrapping_add(3);
            }
            img
        };
        let different = solid(200, 150, [128, 64, 200]);

        let ha = dhash(&a);
        let ha2 = dhash(&a2);
        let hn = dhash(&a_noisy);
        let hd = dhash(&different);

        assert_eq!(ha, ha2, "동일 이미지 동일 해시");
        assert!(hamming(ha, hn) <= 6, "미세변화는 가까움: {}", hamming(ha, hn));
        assert!(hamming(ha, hd) > 10, "다른 그림은 멈: {}", hamming(ha, hd));

        let hashes = vec![ha, hn, ha2, hd];
        let clusters = cluster_near_dups(&hashes, 6);
        // [0,1,2]가 한 클러스터, [3]은 단독
        assert_eq!(clusters.len(), 2, "유사 3장 + 다른 1장 = 2클러스터: {clusters:?}");
        let big = clusters.iter().max_by_key(|c| c.len()).unwrap();
        assert_eq!(big.len(), 3);
    }

    #[test]
    fn near_dup_keep_best() {
        // 유사컷 클러스터에서 점수 최고만 남기기(중복 제거 컬링)
        let a = gradient(160, 120, 0);
        let b = gradient(160, 120, 0);
        let c = solid(160, 120, [10, 200, 80]);
        let hashes = vec![dhash(&a), dhash(&b), dhash(&c)];
        let clusters = cluster_near_dups(&hashes, 6);
        let scores = vec![0.3f32, 0.8, 0.5]; // a,b 유사 → b가 더 높음
        let keep = select_best_per_group(&clusters, &scores, 1);
        assert!(keep.contains(&1), "유사쌍에서 고점 b 채택");
        assert!(!keep.contains(&0), "저점 a 제외");
        assert!(keep.contains(&2), "단독 c는 유지");
    }

    fn meta_iso_time(iso: Option<u32>, time: Option<i64>) -> PhotoMeta {
        PhotoMeta { iso, datetime_secs: time, ..Default::default() }
    }

    #[test]
    fn group_culling_meta_then_burst() {
        let items = vec![
            CullItem { good: true, rank: 0.3, dhash: None, shot_time: Some(100), meta: meta_iso_time(Some(400), Some(100)) },
            CullItem { good: true, rank: 0.9, dhash: None, shot_time: Some(101), meta: meta_iso_time(Some(400), Some(101)) },
            CullItem { good: true, rank: 0.5, dhash: None, shot_time: Some(200), meta: meta_iso_time(Some(400), Some(200)) },
            CullItem { good: true, rank: 0.99, dhash: None, shot_time: Some(201), meta: meta_iso_time(Some(25600), Some(201)) },
        ];
        let p = GroupCullParams {
            use_meta: true,
            meta_filter: MetaFilter { iso_max: Some(6400), ..Default::default() },
            use_burst: true, burst_gap_secs: 2, burst_keep: 1,
            use_dedup: false, dedup_hamming: 6, dedup_keep: 1,
        };
        let out = apply_group_culling(&items, &p);
        assert_eq!(out[3], None, "고ISO 제외(손대지 않음)");
        assert_eq!(out[1], Some(true), "연사 [0,1] 고점 유지");
        assert_eq!(out[0], Some(false), "연사 저점 강등");
        assert_eq!(out[2], Some(true), "단독 유지");
    }

    #[test]
    fn group_culling_dedup_keeps_best() {
        let h = dhash(&gradient(120, 90, 0));
        let h2 = dhash(&gradient(120, 90, 0));
        let hd = dhash(&solid(120, 90, [200, 30, 30]));
        let items = vec![
            CullItem { good: true, rank: 0.4, dhash: Some(h), shot_time: None, meta: PhotoMeta::default() },
            CullItem { good: true, rank: 0.8, dhash: Some(h2), shot_time: None, meta: PhotoMeta::default() },
            CullItem { good: true, rank: 0.6, dhash: Some(hd), shot_time: None, meta: PhotoMeta::default() },
        ];
        let p = GroupCullParams {
            use_meta: false, meta_filter: MetaFilter::default(),
            use_burst: false, burst_gap_secs: 2, burst_keep: 1,
            use_dedup: true, dedup_hamming: 6, dedup_keep: 1,
        };
        let out = apply_group_culling(&items, &p);
        assert_eq!(out[1], Some(true), "유사쌍 고점 유지");
        assert_eq!(out[0], Some(false), "유사쌍 저점 강등");
        assert_eq!(out[2], Some(true), "단독 유지");
    }
}
