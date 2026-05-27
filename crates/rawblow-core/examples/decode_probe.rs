//! 진단용: 폴더의 지원 파일을 디코더로 돌려 성공/실패율을 출력.
//! 동시(멀티스레드) 디코딩 데드락도 감지한다(진행이 멈추면 hang 의심).
//! 사용: cargo run --release -p rawblow-core --example decode_probe -- "<folder>" [threads]

use rawblow_core::decode::{decode_file, DecodeOptions};
use rawblow_core::model::is_supported;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

fn main() {
    let dir = std::env::args().nth(1).expect("dir arg required");
    let threads: usize = std::env::args().nth(2).and_then(|s| s.parse().ok()).unwrap_or(6);

    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .expect("read_dir")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_file() && is_supported(p))
        .collect();
    files.sort();
    let files = Arc::new(files);
    let total = files.len();
    println!("files={total} threads={threads}");

    let next = Arc::new(AtomicUsize::new(0)); // 다음에 처리할 인덱스
    let done = Arc::new(AtomicUsize::new(0)); // 완료 수
    let ok = Arc::new(AtomicUsize::new(0));

    let mut handles = Vec::new();
    for _ in 0..threads {
        let files = files.clone();
        let next = next.clone();
        let done = done.clone();
        let ok = ok.clone();
        handles.push(std::thread::spawn(move || loop {
            let i = next.fetch_add(1, Ordering::Relaxed);
            if i >= files.len() {
                break;
            }
            let p = &files[i];
            // GUI와 동일하게: 썸네일(320) 디코딩
            let r = catch_unwind(AssertUnwindSafe(|| {
                decode_file(
                    p,
                    DecodeOptions {
                        full_raw: false,
                        max_edge: Some(320),
                    },
                )
            }));
            if matches!(r, Ok(Ok(_))) {
                ok.fetch_add(1, Ordering::Relaxed);
            }
            done.fetch_add(1, Ordering::Relaxed);
        }));
    }

    // 감시: 진행이 멈추면 hang 보고.
    let mut last = 0usize;
    let mut stalls = 0;
    loop {
        std::thread::sleep(std::time::Duration::from_secs(1));
        let d = done.load(Ordering::Relaxed);
        let n = next.load(Ordering::Relaxed);
        println!("done={d}/{total} ok={} dispatched={n}", ok.load(Ordering::Relaxed));
        if d >= total {
            break;
        }
        if d == last {
            stalls += 1;
            if stalls >= 5 {
                println!("*** STALL: no progress for 5s at done={d}/{total} (dispatched={n}) — likely DEADLOCK in concurrent decode ***");
                break;
            }
        } else {
            stalls = 0;
        }
        last = d;
    }
    println!("=== final: done={} ok={} of {} ===", done.load(Ordering::Relaxed), ok.load(Ordering::Relaxed), total);
}
