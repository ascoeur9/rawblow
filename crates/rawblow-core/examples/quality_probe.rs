//! 품질 채점 확인용 예제(#50). 한 파일을 디코딩해 노출·초점·기울기 점수를 출력한다.
//! `ai` 기능 빌드 시 둘째 인자로 CLIP-IQA `.onnx` 경로를 주면 미적 점수도 낸다.
//!
//!   cargo run -p rawblow-core --example quality_probe -- photo.cr2
//!   cargo run -p rawblow-core --example quality_probe --features ai -- photo.cr2 clip-iqa.onnx

use rawblow_core::decode::{decode_file, DecodeOptions};
use rawblow_core::quality;
use std::path::Path;

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args.next().expect("usage: quality_probe <image> [nima.onnx]");
    let _model_arg = args.next();

    let img = decode_file(
        Path::new(&path),
        DecodeOptions {
            full_raw: false,
            max_edge: Some(1600),
        },
    )
    .expect("decode failed");

    #[allow(unused_mut)]
    let mut report = quality::analyze(&img);

    #[cfg(feature = "ai")]
    if let Some(model) = _model_arg {
        match quality::AestheticModel::load(Path::new(&model)) {
            Ok(m) => report.aesthetic = m.score(&img).ok(),
            Err(e) => eprintln!("model load: {e}"),
        }
    }

    println!("{} ({}×{})", path, img.width, img.height);
    let e = &report.exposure;
    println!(
        "  노출  : score {:.2}  mean {:.2}  하이라이트클립 {:.1}%  섀도우클립 {:.1}%  DR {:.2}",
        e.score,
        e.mean,
        e.highlight_clip * 100.0,
        e.shadow_clip * 100.0,
        e.dynamic_range
    );
    let f = &report.focus;
    println!(
        "  초점  : {}  (선명도 {:.2}, acutance {:.1})",
        if f.in_focus { "합초 추정" } else { "흐림 후보" },
        f.sharpness,
        f.acutance
    );
    let t = &report.tilt;
    println!("  기울기: {:+.2}°  (신뢰 {:.2})", t.degrees, t.confidence);
    if let Some(a) = report.aesthetic {
        println!("  미적  : {:.2} / 10", a);
    }
}
