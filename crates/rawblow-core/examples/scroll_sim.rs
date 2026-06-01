//! [임시] 스크롤 시 보이는 셀(prio) 응답성 시뮬레이션. 워커의 prio>bg 레인 + bg 스레드 제한을
//! 그대로 본떠, 백그라운드 윈도우 프리페치가 도는 동안 보이는 썸네일이 얼마나 빨리 오는지 측정.
//! 사용: cargo run --release -p rawblow-core --example scroll_sim -- "<folder>" [threads] [bg_threads] [offset]

use crossbeam_channel::{select, unbounded};
use rawblow_core::decode::{decode_file, DecodeOptions};
use rawblow_core::model::is_supported;
use std::path::PathBuf;
use std::time::Instant;
use walkdir::WalkDir;

fn main() {
    let dir = std::env::args().nth(1).expect("dir arg");
    let threads: usize = std::env::args().nth(2).and_then(|s| s.parse().ok()).unwrap_or(4);
    let bg_threads: usize = std::env::args().nth(3).and_then(|s| s.parse().ok()).unwrap_or(2);
    let offset: usize = std::env::args().nth(4).and_then(|s| s.parse().ok()).unwrap_or(0);
    let visible = 40usize; // 한 화면분 보이는 셀
    let window = 96usize; // 배경 프리페치 윈도우

    let mut files: Vec<PathBuf> = WalkDir::new(&dir)
        .max_depth(1)
        .into_iter()
        .filter_map(|e| e.ok().map(|e| e.into_path()))
        .filter(|p| p.is_file() && is_supported(p))
        .collect();
    files.sort();
    let files: Vec<PathBuf> = files.into_iter().skip(offset).take(window + visible + 8).collect();

    let (prio_tx, prio_rx) = unbounded::<PathBuf>();
    let (bg_tx, bg_rx) = unbounded::<PathBuf>();
    let (res_tx, res_rx) = unbounded::<(bool, f64)>(); // (is_prio, decode_ms)

    for idx in 0..threads {
        let (prio_rx, bg_rx, res_tx) = (prio_rx.clone(), bg_rx.clone(), res_tx.clone());
        let serves_bg = bg_threads == 0 || idx < bg_threads;
        std::thread::spawn(move || loop {
            let (p, is_prio) = match prio_rx.try_recv() {
                Ok(p) => (p, true),
                Err(_) => {
                    if serves_bg {
                        select! {
                            recv(prio_rx) -> m => match m { Ok(p) => (p, true), Err(_) => break },
                            recv(bg_rx) -> m => match m { Ok(p) => (p, false), Err(_) => break },
                        }
                    } else {
                        match prio_rx.recv() { Ok(p) => (p, true), Err(_) => break }
                    }
                }
            };
            let t = Instant::now();
            let _ = decode_file(&p, DecodeOptions { full_raw: false, max_edge: Some(320) });
            let _ = res_tx.send((is_prio, t.elapsed().as_secs_f64() * 1000.0));
        });
    }

    // 배경 윈도우 프리페치 적재.
    for p in files.iter().take(window) {
        let _ = bg_tx.send(p.clone());
    }
    // 살짝 뒤 보이는 셀들을 prio로 요청(스크롤 진입).
    std::thread::sleep(std::time::Duration::from_millis(20));
    let t0 = Instant::now();
    for p in files.iter().skip(window.min(files.len().saturating_sub(visible))).take(visible) {
        let _ = prio_tx.send(p.clone());
    }

    // 보이는 셀 결과가 모두 올 때까지 기다리며 prio 지연 수집.
    let mut prio_done = 0;
    let mut prio_lat = Vec::new();
    let first_req = t0;
    while prio_done < visible {
        if let Ok((is_prio, _ms)) = res_rx.recv() {
            if is_prio {
                prio_lat.push(first_req.elapsed().as_secs_f64() * 1000.0);
                prio_done += 1;
            }
        }
    }
    let total = first_req.elapsed().as_secs_f64() * 1000.0;
    prio_lat.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let p50 = prio_lat[prio_lat.len() / 2];
    let p95 = prio_lat[(prio_lat.len() * 95 / 100).min(prio_lat.len() - 1)];
    println!(
        "threads={threads} bg_threads={bg_threads} window={window} visible={visible} offset={offset}");
    println!(
        "  보이는 셀 {visible}개 전부 표시까지: {:.0}ms  (p50 {:.0}ms, p95 {:.0}ms, max {:.0}ms)",
        total, p50, p95, prio_lat.last().unwrap()
    );
}
