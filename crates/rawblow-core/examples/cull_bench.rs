//! AI 컬링 추론 처리량 벤치(#50). 합성 이미지로 `score()` 처리량을 측정한다.
//! 추론 시간은 입력 내용과 무관(텐서 모양 고정)하므로 실제 사진 없이도 신뢰할 수 있다.
//!
//! 사용:
//!   cargo run --release --features ai --example cull_bench -- <model.onnx> [cpu|gpu|auto] [iters] [threads]
//!
//! 출력: 워밍업 후 iters×threads 장을 채점하고 ms/img(스레드당 지연)·img/s(총 처리량)를 보고.

use rawblow_core::decode::DecodedImage;
use rawblow_core::quality::{analyze_selective, Accel, AestheticModel};
use std::sync::Arc;
use std::time::Instant;

/// 결정적 합성 RGBA 이미지(그래디언트+노이즈). 추론 비용은 내용 무관이라 OK.
fn synth(w: u32, h: u32) -> DecodedImage {
    let mut rgba = vec![0u8; (w as usize) * (h as usize) * 4];
    for (i, px) in rgba.as_chunks_mut::<4>().0.iter_mut().enumerate() {
        let v = ((i * 97) % 251) as u8;
        px[0] = v;
        px[1] = v.wrapping_add(40);
        px[2] = v.wrapping_add(80);
        px[3] = 255;
    }
    DecodedImage { width: w, height: h, rgba, color_managed: false, full_raw: false }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let model_path = args.get(1).cloned().expect("usage: cull_bench <model.onnx> [cpu|gpu|auto] [iters] [threads]");
    let ep = args.get(2).map(|s| s.as_str()).unwrap_or("auto");
    let iters: usize = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(40);
    let threads: usize = args.get(4).and_then(|s| s.parse().ok()).unwrap_or(1);
    let accel = match ep {
        "cpu" => Accel::Cpu,
        "gpu" => Accel::Gpu,
        _ => Accel::Auto,
    };

    let warmup = 5usize;
    let img = Arc::new(synth(1024, 1024));

    // 모델 로드 시간(EP 컴파일 포함 — CoreML/WebGPU는 첫 로드가 느릴 수 있음).
    let load_t0 = Instant::now();
    let probe = AestheticModel::load_with(std::path::Path::new(&model_path), accel)
        .unwrap_or_else(|e| panic!("model load failed: {e}"));
    let load_ms = load_t0.elapsed().as_secs_f64() * 1000.0;
    let first = probe.score(&img).unwrap_or_else(|e| panic!("score failed: {e}"));
    drop(probe);

    let model_name = std::path::Path::new(&model_path).file_name().unwrap().to_string_lossy().to_string();
    eprintln!(
        "model={model_name} ep={ep} threads={threads} iters/thread={iters} | load={load_ms:.0}ms first_score={first:.3}"
    );

    // RB_SHARE=1: 모델 세션을 한 번만 로드해 모든 스레드가 Arc로 공유(WebGPU 동시성 크래시 회피 검증).
    let shared: Option<Arc<AestheticModel>> = if std::env::var("RB_SHARE").is_ok() {
        Some(Arc::new(AestheticModel::load_with(std::path::Path::new(&model_path), accel).expect("load")))
    } else {
        None
    };

    let t0 = Instant::now();
    let mut handles = Vec::new();
    for _ in 0..threads {
        let mp = model_path.clone();
        let img = img.clone();
        let shared = shared.clone();
        // RB_PIPE: ""=score만, "cv"=analyze만, "full"=analyze+score. RB_BATCH=N으로 배치 추론.
        let pipe = std::env::var("RB_PIPE").unwrap_or_default();
        let batch: usize = std::env::var("RB_BATCH").ok().and_then(|s| s.parse().ok()).unwrap_or(0);
        handles.push(std::thread::spawn(move || {
            let m: Arc<AestheticModel> = shared
                .unwrap_or_else(|| Arc::new(AestheticModel::load_with(std::path::Path::new(&mp), accel).expect("load")));
            if batch > 0 {
                let imgs: Vec<&DecodedImage> = (0..batch).map(|_| img.as_ref()).collect();
                for _ in 0..warmup {
                    let _ = m.score_batch(&imgs);
                }
                let s = Instant::now();
                for _ in 0..iters {
                    let _ = m.score_batch(&imgs).expect("score_batch");
                }
                return s.elapsed().as_secs_f64();
            }
            let run = |img: &DecodedImage| match pipe.as_str() {
                "cv" => {
                    let _ = analyze_selective(img, true, true, true);
                }
                "full" => {
                    let _ = analyze_selective(img, true, true, true);
                    let _ = m.score(img).expect("score");
                }
                _ => {
                    let _ = m.score(img).expect("score");
                }
            };
            for _ in 0..warmup {
                run(&img);
            }
            let s = Instant::now();
            for _ in 0..iters {
                run(&img);
            }
            s.elapsed().as_secs_f64()
        }));
    }
    let mut max_secs = 0f64;
    for h in handles {
        max_secs = max_secs.max(h.join().unwrap());
    }
    let total_secs = t0.elapsed().as_secs_f64();
    let batch_env: usize = std::env::var("RB_BATCH").ok().and_then(|s| s.parse().ok()).unwrap_or(0);
    let per_thread = iters * batch_env.max(1);
    let total_imgs = (per_thread * threads) as f64;
    let ms_per_img = max_secs / per_thread as f64 * 1000.0; // 장당 평균(배치 환산)
    let imgs_per_sec = total_imgs / max_secs; // 총 처리량(추론 구간)

    println!(
        "RESULT model={model_name} ep={ep} threads={threads} batch={batch_env} | {ms_per_img:.1} ms/img | {imgs_per_sec:.1} img/s | wall={total_secs:.1}s"
    );
}
