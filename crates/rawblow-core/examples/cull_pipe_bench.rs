//! 실파이프라인 컬링 벤치(#50): **디코드 포함** decode→analyze→score 처리량을 측정한다.
//! cull_bench는 추론만(합성 텐서) 쟀지만, 실제 컬링은 파일 디코드가 큰 비중이라 이걸로 확인.
//! (JPEG 디코드는 RAW보다 빠르므로 RAW 폴더는 이보다 느림 — 상대 비교·구조 검증용.)
//!
//! 사용: cargo run --release -p rawblow-core --features ai --example cull_pipe_bench -- <model.onnx> [cpu|gpu] [n_images] [threads]

use rawblow_core::decode::{decode_file, DecodeOptions};
use rawblow_core::quality::{analyze_selective, Accel, AestheticModel};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Instant;

// 디코드 최대 변(RB_EDGE로 조정). 작을수록 디코드·CV 빠르나 초점 판별력 감소.
fn edge() -> u32 {
    std::env::var("RB_EDGE").ok().and_then(|s| s.parse().ok()).unwrap_or(1024)
}

/// 생산자-소비자: 디코드 스레드 풀(대부분 코어) → 채널 → 추론 소비자(GPU=배치). 디코드가
/// 병목인 실파이프라인에서 GPU를 연속으로 먹여 추론 우위를 실현한다. 로드 시간은 측정 제외.
fn run_decoupled(files: &Arc<Vec<PathBuf>>, model_path: &str, accel: Accel, intra: Option<usize>, ep: &str, threads: usize) {
    let n_infer = if ep == "gpu" { 2usize } else { threads }; // CoreML 동시 2소비자, CPU는 워커가 곧 추론
    let n_decode = threads.max(1);
    let batch = if ep == "gpu" { 8usize } else { 1usize };
    let next = Arc::new(AtomicUsize::new(0));
    let done = Arc::new(AtomicUsize::new(0));
    // 디코드된 이미지를 흘려보내는 바운드 채널(백프레셔). crossbeam로 다중 소비자.
    let (tx, rx) = crossbeam_channel::bounded::<rawblow_core::decode::DecodedImage>(64);

    // 추론 소비자(모델 로드는 타이머 밖 — 미리 로드해 ready 후 시작).
    let infer_ready = Arc::new(std::sync::Barrier::new(n_infer + 1));
    let mut infer_handles = Vec::new();
    for _ in 0..n_infer {
        let rx = rx.clone();
        let done = done.clone();
        let mp = model_path.to_string();
        let infer_ready = infer_ready.clone();
        infer_handles.push(std::thread::spawn(move || {
            let model = if mp == "none" { None } else { AestheticModel::load_tuned(std::path::Path::new(&mp), accel, intra).ok() };
            infer_ready.wait();
            let mut buf: Vec<rawblow_core::decode::DecodedImage> = Vec::with_capacity(batch);
            // 채널이 닫힐 때까지 수신.
            while let Ok(img) = rx.recv() {
                buf.push(img);
                // 배치가 차거나 채널이 잠시 빌 때까지 모아 score_batch.
                while buf.len() < batch {
                    match rx.try_recv() { Ok(i) => buf.push(i), Err(_) => break }
                }
                if let Some(m) = model.as_ref() {
                    let refs: Vec<&_> = buf.iter().collect();
                    let _ = m.score_batch(&refs);
                }
                done.fetch_add(buf.len(), Ordering::Relaxed);
                buf.clear();
            }
        }));
    }
    drop(rx);
    infer_ready.wait();
    let t0 = Instant::now();
    // 디코드 생산자.
    let mut dec_handles = Vec::new();
    for _ in 0..n_decode {
        let files = files.clone();
        let next = next.clone();
        let tx = tx.clone();
        dec_handles.push(std::thread::spawn(move || loop {
            let idx = next.fetch_add(1, Ordering::Relaxed);
            if idx >= files.len() { break; }
            if let Ok(im) = decode_file(&files[idx], DecodeOptions { full_raw: false, max_edge: Some(edge()) }) {
                if std::env::var("RB_NOCV").is_err() {
                    let _ = analyze_selective(&im, true, true, true);
                }
                if tx.send(im).is_err() { break; }
            }
        }));
    }
    drop(tx);
    for h in dec_handles { let _ = h.join(); }
    for h in infer_handles { let _ = h.join(); }
    let secs = t0.elapsed().as_secs_f64();
    let d = done.load(Ordering::Relaxed) as f64;
    println!(
        "PIPE-DECOUPLE model={} ep={ep} decode={n_decode} infer={n_infer} | {} imgs in {:.2}s = {:.1} img/s",
        std::path::Path::new(model_path).file_name().unwrap().to_string_lossy(), d as usize, secs, d / secs
    );
}

/// 결정적 콘텐츠의 큰 JPEG들을 임시 폴더에 1회 생성(이미 있으면 재사용).
fn ensure_jpegs(dir: &PathBuf, n: usize) -> Vec<PathBuf> {
    std::fs::create_dir_all(dir).unwrap();
    let (w, h) = (3000u32, 2000u32);
    (0..n)
        .map(|i| {
            let p = dir.join(format!("bench_{i:04}.jpg"));
            if !p.exists() {
                let mut img = image::RgbImage::new(w, h);
                for (x, y, px) in img.enumerate_pixels_mut() {
                    // 디테일 있는 패턴(JPEG 압축이 일하도록) + 이미지별 변형.
                    let v = (((x ^ y) as usize + i * 37) % 256) as u8;
                    *px = image::Rgb([v, v.wrapping_add(70), v.wrapping_add(140)]);
                }
                img.save(&p).unwrap();
            }
            p
        })
        .collect()
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let model_path = args.get(1).cloned().expect("usage: cull_pipe_bench <model.onnx> [cpu|gpu] [n] [threads]");
    let ep = args.get(2).map(|s| s.as_str()).unwrap_or("cpu");
    let n: usize = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(100);
    let threads: usize = args.get(4).and_then(|s| s.parse().ok()).unwrap_or(8);
    let accel = if ep == "gpu" { Accel::Gpu } else { Accel::Cpu };
    let intra = if ep == "gpu" { None } else { Some(1usize) };

    let dir = std::env::temp_dir().join("rawblow_pipebench");
    let files = Arc::new(ensure_jpegs(&dir, n));
    eprintln!("pipe-bench: {} JPEGs @ {}, ep={ep} threads={threads}", n, dir.display());

    // RB_DECOUPLE=1: 디코드 스레드 풀과 추론 소비자를 분리(생산자-소비자). 디코드가 GPU를
    // 굶기지 않게 채널로 흘려보내, GPU 추론(빠름)을 디코드 공급률까지 끌어올린다.
    if std::env::var("RB_DECOUPLE").is_ok() {
        run_decoupled(&files, &model_path, accel, intra, ep, threads);
        return;
    }

    // GPU는 chunk를 모아 score_batch(프로덕션과 동일), CPU는 1장씩.
    let batch_size = if ep == "gpu" { 8usize } else { 1usize };
    let next = Arc::new(AtomicUsize::new(0));
    let done = Arc::new(AtomicUsize::new(0));
    let ready = Arc::new(std::sync::Barrier::new(threads + 1));
    let go = Arc::new(std::sync::Barrier::new(threads + 1));
    let mut handles = Vec::new();
    for _ in 0..threads {
        let files = files.clone();
        let next = next.clone();
        let done = done.clone();
        let mp = model_path.clone();
        let ready = ready.clone();
        let go = go.clone();
        handles.push(std::thread::spawn(move || {
            // 모델 로드(타이머 밖). model="none"이면 CV 전용(미적 모델 없음). 워밍업 후 일제히 시작.
            let model = if mp == "none" {
                None
            } else {
                AestheticModel::load_tuned(std::path::Path::new(&mp), accel, intra).ok()
            };
            if let (Some(m), Ok(im)) = (model.as_ref(), decode_file(&files[0], DecodeOptions { full_raw: false, max_edge: Some(edge()) })) {
                let _ = m.score(&im); // 워밍업(CoreML 컴파일 등) — 타이머 제외.
            }
            ready.wait();
            go.wait();
            loop {
                let start = next.fetch_add(batch_size, Ordering::Relaxed);
                if start >= files.len() {
                    break;
                }
                let end = (start + batch_size).min(files.len());
                let mut imgs = Vec::with_capacity(end - start);
                for f in &files[start..end] {
                    if let Ok(im) = decode_file(f, DecodeOptions { full_raw: false, max_edge: Some(edge()) }) {
                        let _ = analyze_selective(&im, true, true, true);
                        imgs.push(im);
                    }
                }
                if let Some(m) = model.as_ref() {
                    if batch_size > 1 {
                        let refs: Vec<&_> = imgs.iter().collect();
                        let _ = m.score_batch(&refs);
                    } else {
                        for im in &imgs {
                            let _ = m.score(im);
                        }
                    }
                }
                done.fetch_add(imgs.len(), Ordering::Relaxed);
            }
        }));
    }
    ready.wait(); // 모든 워커가 로드+워밍업 완료
    let t0 = Instant::now();
    go.wait(); // 일제히 시작(로드 시간 제외)
    for h in handles {
        let _ = h.join();
    }
    let secs = t0.elapsed().as_secs_f64();
    let d = done.load(Ordering::Relaxed) as f64;
    println!(
        "PIPE model={} ep={ep} threads={threads} | {} imgs in {:.2}s = {:.1} img/s (decode+CV+score)",
        std::path::Path::new(&model_path).file_name().unwrap().to_string_lossy(),
        d as usize,
        secs,
        d / secs
    );
}
