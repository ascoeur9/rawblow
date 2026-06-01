//! 단일파일 고화질/썸네일 로딩 병목 프로파일러. 단계별 시간 + 디스크 읽은 바이트.
//! 사용: cargo run --release -p rawblow-core --example rw2_profile -- "<folder>" [count]

use rawblow_core::decode::{self, decode_file, DecodeOptions};
use rawblow_core::model::is_supported;
use std::path::PathBuf;
use std::time::Instant;
use walkdir::WalkDir;

fn ms(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1000.0
}

/// 파일 안의 임베디드 JPEG 후보(오프셋, 길이, 긴변)를 거칠게 스캔.
fn map_embedded(bytes: &[u8]) -> Vec<(usize, usize, u32)> {
    let mut out = Vec::new();
    let mut i = 0usize;
    while i + 3 < bytes.len() {
        if bytes[i] == 0xFF && bytes[i + 1] == 0xD8 && bytes[i + 2] == 0xFF {
            // 다음 SOI 또는 끝까지를 상한으로 EOI 탐색(거친 추정).
            if let Some(rel) = bytes[i + 2..].windows(2).position(|w| w == [0xFF, 0xD9]) {
                let len = rel + 4;
                out.push((i, len, 0));
                i += len;
                continue;
            }
        }
        i += 1;
    }
    out
}

fn main() {
    let dir = std::env::args().nth(1).expect("dir arg required");
    let count: usize = std::env::args().nth(2).and_then(|s| s.parse().ok()).unwrap_or(6);

    let mut files: Vec<PathBuf> = WalkDir::new(&dir)
        .max_depth(1)
        .into_iter()
        .filter_map(|e| e.ok().map(|e| e.into_path()))
        .filter(|p| p.is_file() && is_supported(p))
        .collect();
    files.sort();
    files.truncate(count);
    println!("files={} (of folder)\n", files.len());

    let (mut sum_thumb, mut sum_prev, mut sum_orig) = (0.0, 0.0, 0.0);
    let (mut by_thumb, mut by_prev, mut by_orig) = (0u64, 0u64, 0u64);
    let mut disk_mbps = Vec::new();

    for p in &files {
        let name = p.file_name().unwrap().to_string_lossy();
        let fsize = std::fs::metadata(p).map(|m| m.len()).unwrap_or(0);

        // 0) 디스크 속도(전체 읽기). OS 캐시 영향 있음 — 콜드/웜 모두 참고로.
        let t = Instant::now();
        let whole = std::fs::read(p).unwrap_or_default();
        let dread = ms(t);
        let mbps = (whole.len() as f64 / 1e6) / (dread / 1000.0).max(1e-6);
        disk_mbps.push(mbps);

        // 임베디드 맵(앞 1MB vs 전체).
        let n1m = (1 << 20).min(whole.len());
        let emb_prefix = map_embedded(&whole[..n1m]);
        let emb_whole = map_embedded(&whole);
        let biggest = emb_whole.iter().map(|c| c.1).max().unwrap_or(0);

        // 1) orientation 비용(decode_file 내부에서 매번 호출됨).
        let _ = decode::take_bytes_read();
        let t = Instant::now();
        let _o = rawblow_core::meta::orientation(p);
        let t_or = ms(t);
        let b_or = decode::take_bytes_read();

        // 2) 썸네일(320).
        let _ = decode::take_bytes_read();
        let t = Instant::now();
        let th = decode_file(p, DecodeOptions { full_raw: false, max_edge: Some(320) });
        let t_th = ms(t);
        let b_th = decode::take_bytes_read();

        // 3) 프리뷰(1600).
        let _ = decode::take_bytes_read();
        let t = Instant::now();
        let pv = decode_file(p, DecodeOptions { full_raw: false, max_edge: Some(1600) });
        let t_pv = ms(t);
        let b_pv = decode::take_bytes_read();

        // 4) ORIG(full_raw, 8192).
        let _ = decode::take_bytes_read();
        let t = Instant::now();
        let og = decode_file(p, DecodeOptions { full_raw: true, max_edge: Some(8192) });
        let t_og = ms(t);
        let b_og = decode::take_bytes_read();

        let dim = |r: &Result<decode::DecodedImage, _>| r.as_ref().map(|i| format!("{}x{}", i.width, i.height)).unwrap_or_else(|_| "ERR".into());

        println!("{name}  {:.1}MB  disk={:.0}MB/s  embPrefix={} embWhole={} biggestEmb={:.1}MB",
            fsize as f64 / 1e6, mbps, emb_prefix.len(), emb_whole.len(), biggest as f64 / 1e6);
        println!("   orient  {:6.0}ms  {:7.2}MB", t_or, b_or as f64 / 1e6);
        println!("   thumb   {:6.0}ms  {:7.2}MB  {}", t_th, b_th as f64 / 1e6, dim(&th));
        println!("   preview {:6.0}ms  {:7.2}MB  {}", t_pv, b_pv as f64 / 1e6, dim(&pv));
        println!("   ORIG    {:6.0}ms  {:7.2}MB  {}", t_og, b_og as f64 / 1e6, dim(&og));

        sum_thumb += t_th; sum_prev += t_pv; sum_orig += t_og;
        by_thumb += b_th; by_prev += b_pv; by_orig += b_og;
    }

    let n = files.len().max(1) as f64;
    let avg_disk = disk_mbps.iter().sum::<f64>() / n;
    println!("\n=== AVG over {} files (disk ~{:.0} MB/s) ===", files.len(), avg_disk);
    println!("thumb   {:7.0}ms  {:6.2}MB read", sum_thumb / n, by_thumb as f64 / 1e6 / n);
    println!("preview {:7.0}ms  {:6.2}MB read", sum_prev / n, by_prev as f64 / 1e6 / n);
    println!("ORIG    {:7.0}ms  {:6.2}MB read", sum_orig / n, by_orig as f64 / 1e6 / n);
}
