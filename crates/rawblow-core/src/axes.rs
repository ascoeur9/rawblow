//! CLIP 다축 점수(#51 후속, AI 선명도 등). `clip-axes-*` ONNX는 프롬프트쌍 4개를 그래프에
//! 구워넣어 입력=이미지(1,3,224,224) → 출력=(N,4) 각 축 P(positive)를 낸다.
//! 축 순서: 0=quality, 1=genre_portrait(인물↔풍경), 2=sharp, 3=compose (export_clip_axes.py).
//! 전처리는 CLIP 정규화(아래 MEAN/STD), RGB, 224². `ai` 미빌드면 모듈 전체가 빠진다.

/// 축 인덱스(export_clip_axes.py PAIRS 순서와 일치).
pub const AXIS_QUALITY: usize = 0;
pub const AXIS_GENRE_PORTRAIT: usize = 1;
pub const AXIS_SHARP: usize = 2;
pub const AXIS_COMPOSE: usize = 3;

#[cfg(feature = "ai")]
pub use detail::AxesModel;

#[cfg(feature = "ai")]
mod detail {
    use crate::decode::DecodedImage;
    use ort::session::Session;
    use ort::value::Tensor;
    use std::path::Path;
    use std::sync::Mutex;

    const SIDE: usize = 224;
    // CLIP 전처리 통계(open_clip 표준, export_clip_axes.py와 동일).
    const MEAN: [f32; 3] = [0.481_454_66, 0.457_827_5, 0.408_210_73];
    const STD: [f32; 3] = [0.268_629_54, 0.261_302_58, 0.275_777_11];

    /// CLIP 다축 모델. 내부 Mutex로 스레드 안전(워커 공유 가능).
    pub struct AxesModel {
        session: Mutex<Session>,
    }

    impl AxesModel {
        pub fn load(path: &Path) -> Result<Self, String> {
            let session = Session::builder()
                .map_err(|e| format!("ort builder: {e}"))?
                .commit_from_file(path)
                .map_err(|e| format!("ort load axes {}: {e}", path.display()))?;
            Ok(Self { session: Mutex::new(session) })
        }

        /// 4축 P(positive) [quality, genre_portrait, sharp, compose]. 실패 시 None.
        pub fn scores(&self, img: &DecodedImage) -> Option<[f32; 4]> {
            let chw = preprocess(img)?;
            let tensor = Tensor::from_array(([1usize, 3, SIDE, SIDE], chw)).ok()?;
            let mut sess = self.session.lock().ok()?;
            let iname = sess.inputs().first()?.name().to_string();
            let outputs = sess.run(ort::inputs![iname => tensor]).ok()?;
            let (_, data) = outputs[0].try_extract_tensor::<f32>().ok()?;
            if data.len() < 4 {
                return None;
            }
            Some([data[0], data[1], data[2], data[3]])
        }
    }

    fn preprocess(img: &DecodedImage) -> Option<Vec<f32>> {
        let px = (img.width as usize).checked_mul(img.height as usize)?;
        if px == 0 || img.rgba.len() < px * 4 {
            return None;
        }
        let buf = image::RgbaImage::from_raw(img.width, img.height, img.rgba.clone())?;
        let rgb = image::DynamicImage::ImageRgba8(buf)
            .resize_exact(SIDE as u32, SIDE as u32, image::imageops::FilterType::CatmullRom)
            .to_rgb8();
        let plane = SIDE * SIDE;
        let mut out = vec![0f32; 3 * plane];
        for (i, p) in rgb.pixels().enumerate() {
            for c in 0..3 {
                out[c * plane + i] = (p[c] as f32 / 255.0 - MEAN[c]) / STD[c];
            }
        }
        Some(out)
    }
}
