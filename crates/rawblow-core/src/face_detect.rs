//! 얼굴 존재 검출(#51 후속, 컬링 "얼굴 있는 컷"·"장르 인물/풍경"용). YuNet(2023mar) ONNX를
//! ort로 실행한다. 정확한 박스는 필요 없고 **얼굴이 한 개라도 있는지**만 본다 — YuNet의
//! per-cell `cls`(클래스 확률)·`obj`(objectness, 둘 다 sigmoid 후 0..1) 곱의 최댓값으로 판정한다.
//! 실측(검증): 얼굴 사진 max(cls·obj)≈0.66–0.84, 단색(얼굴 없음)≈0.000–0.002 → 임계 0.5에서 분리.
//! 모델 입력은 고정 640×640 BGR(0–255, 정규화 없음). 미빌드(ai 미사용)면 모듈 전체가 빠진다.

#[cfg(feature = "ai")]
pub use detail::FaceModel;

/// 얼굴 존재로 판정하는 max(cls·obj) 임계. 실측 마진이 커서(0.66 vs 0.002) 0.5면 충분.
pub const FACE_SCORE_THRESH: f32 = 0.5;

#[cfg(feature = "ai")]
mod detail {
    use super::FACE_SCORE_THRESH;
    use crate::decode::DecodedImage;
    use ort::session::Session;
    use ort::value::Tensor;
    use std::path::Path;
    use std::sync::Mutex;

    const SIDE: usize = 640; // YuNet 2023mar 고정 입력.

    /// YuNet 얼굴 검출 모델. 내부 Mutex로 스레드 안전(워커 여러 개가 공유 가능).
    pub struct FaceModel {
        session: Mutex<Session>,
    }

    impl FaceModel {
        /// `.onnx`에서 로드(CPU — 작은 모델이라 충분히 빠름).
        pub fn load(path: &Path) -> Result<Self, String> {
            let session = Session::builder()
                .map_err(|e| format!("ort builder: {e}"))?
                .commit_from_file(path)
                .map_err(|e| format!("ort load yunet {}: {e}", path.display()))?;
            Ok(Self { session: Mutex::new(session) })
        }

        /// 이미지에 얼굴이 한 개라도 있으면 true. 추론 실패는 false(조용히 미검출).
        pub fn has_face(&self, img: &DecodedImage) -> bool {
            self.face_score(img).map(|s| s >= FACE_SCORE_THRESH).unwrap_or(false)
        }

        /// 최대 얼굴 점수 max(cls·obj). 디코드/추론 실패 시 None.
        pub fn face_score(&self, img: &DecodedImage) -> Option<f32> {
            let chw = preprocess_bgr(img)?;
            let tensor = Tensor::from_array(([1usize, 3, SIDE, SIDE], chw)).ok()?;
            let mut sess = self.session.lock().ok()?;
            let iname = sess.inputs().first()?.name().to_string();
            let outputs = sess.run(ort::inputs![iname => tensor]).ok()?;
            let mut best = 0f32;
            // 스트라이드 8/16/32의 cls·obj per-cell 곱 최댓값. 한 곳이라도 높으면 얼굴.
            for s in [8usize, 16, 32] {
                let (_, cls) = outputs[format!("cls_{s}").as_str()].try_extract_tensor::<f32>().ok()?;
                let (_, obj) = outputs[format!("obj_{s}").as_str()].try_extract_tensor::<f32>().ok()?;
                for (c, o) in cls.iter().zip(obj.iter()) {
                    best = best.max(c * o);
                }
            }
            Some(best)
        }
    }

    /// RGBA → 640×640 BGR CHW(0–255, 정규화 없음). 종횡비는 무시(존재 판정엔 무관).
    fn preprocess_bgr(img: &DecodedImage) -> Option<Vec<f32>> {
        let px = (img.width as usize).checked_mul(img.height as usize)?;
        if px == 0 || img.rgba.len() < px * 4 {
            return None;
        }
        let buf = image::RgbaImage::from_raw(img.width, img.height, img.rgba.clone())?;
        let rgb = image::DynamicImage::ImageRgba8(buf)
            .resize_exact(SIDE as u32, SIDE as u32, image::imageops::FilterType::Triangle)
            .to_rgb8();
        let plane = SIDE * SIDE;
        let mut out = vec![0f32; 3 * plane];
        for (i, p) in rgb.pixels().enumerate() {
            out[i] = p[2] as f32; // B
            out[plane + i] = p[1] as f32; // G
            out[2 * plane + i] = p[0] as f32; // R
        }
        Some(out)
    }
}
