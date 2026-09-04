//! 객체 검출(#51 후속, 컬링 "객체 포함"용). YOLOv10n ONNX(NMS-free, onnx-community)를 ort로 실행.
//! 출력 (1,300,6)=[x1,y1,x2,y2,conf,cls]. 특정 COCO 클래스가 conf 임계 이상으로 검출되면 포함으로 본다.
//! 입력 고정 640×640 RGB(/255). `ai` 미빌드면 모듈 전체가 빠진다.

/// 검출 신뢰도 임계. 실측(샘플): person 0.77–0.92, 가구 0.75–0.84로 충분히 분리.
pub const OBJ_CONF_THRESH: f32 = 0.30;

/// COCO 80 클래스 이름(YOLO 표준 순서). object_class 텍스트 → 인덱스 매핑용.
pub const COCO_NAMES: [&str; 80] = [
    "person", "bicycle", "car", "motorcycle", "airplane", "bus", "train", "truck", "boat",
    "traffic light", "fire hydrant", "stop sign", "parking meter", "bench", "bird", "cat", "dog",
    "horse", "sheep", "cow", "elephant", "bear", "zebra", "giraffe", "backpack", "umbrella",
    "handbag", "tie", "suitcase", "frisbee", "skis", "snowboard", "sports ball", "kite",
    "baseball bat", "baseball glove", "skateboard", "surfboard", "tennis racket", "bottle",
    "wine glass", "cup", "fork", "knife", "spoon", "bowl", "banana", "apple", "sandwich",
    "orange", "broccoli", "carrot", "hot dog", "pizza", "donut", "cake", "chair", "couch",
    "potted plant", "bed", "dining table", "toilet", "tv", "laptop", "mouse", "remote",
    "keyboard", "cell phone", "microwave", "oven", "toaster", "sink", "refrigerator", "book",
    "clock", "vase", "scissors", "teddy bear", "hair drier", "toothbrush",
];

/// object_class 텍스트를 COCO 인덱스로 해석한다. 정확 일치 우선, 없으면 부분일치(대소문자 무시).
/// 예: "person"·"PERSON"·"sports" 등. 매칭 실패 시 None(필터가 동작하지 않음 → 호출부가 안내).
pub fn coco_index(name: &str) -> Option<u8> {
    let q = name.trim().to_lowercase();
    if q.is_empty() {
        return None;
    }
    if let Some(i) = COCO_NAMES.iter().position(|n| *n == q) {
        return Some(i as u8);
    }
    COCO_NAMES.iter().position(|n| n.contains(&q) || q.contains(*n)).map(|i| i as u8)
}

#[cfg(feature = "ai")]
pub use detail::ObjectModel;

#[cfg(feature = "ai")]
mod detail {
    use super::OBJ_CONF_THRESH;
    use crate::decode::DecodedImage;
    use ort::session::Session;
    use ort::value::Tensor;
    use std::path::Path;
    use std::sync::Mutex;

    const SIDE: usize = 640;

    /// YOLOv10 객체 검출 모델. 내부 Mutex로 스레드 안전(워커 공유 가능).
    pub struct ObjectModel {
        session: Mutex<Session>,
    }

    impl ObjectModel {
        pub fn load(path: &Path) -> Result<Self, String> {
            let session = Session::builder()
                .map_err(|e| format!("ort builder: {e}"))?
                .commit_from_file(path)
                .map_err(|e| format!("ort load yolo {}: {e}", path.display()))?;
            Ok(Self { session: Mutex::new(session) })
        }

        /// 이미지에 `class_idx` 객체가 conf 임계 이상으로 있으면 true. 추론 실패는 false.
        pub fn contains(&self, img: &DecodedImage, class_idx: u8) -> bool {
            self.detected(img).map(|set| set.contains(&class_idx)).unwrap_or(false)
        }

        /// conf 임계 이상으로 검출된 COCO 클래스 인덱스 집합. 실패 시 None.
        pub fn detected(&self, img: &DecodedImage) -> Option<Vec<u8>> {
            let chw = preprocess(img)?;
            let tensor = Tensor::from_array(([1usize, 3, SIDE, SIDE], chw)).ok()?;
            let mut sess = self.session.lock().ok()?;
            let iname = sess.inputs().first()?.name().to_string();
            let outputs = sess.run(ort::inputs![iname => tensor]).ok()?;
            let (_, d) = outputs[0].try_extract_tensor::<f32>().ok()?;
            // (1,300,6) = [x1,y1,x2,y2,conf,cls]. NMS-free라 그대로 임계만.
            let mut set: Vec<u8> = Vec::new();
            for det in d.as_chunks::<6>().0 {
                if det[4] >= OBJ_CONF_THRESH {
                    let cls = det[5].round();
                    if (0.0..80.0).contains(&cls) {
                        let c = cls as u8;
                        if !set.contains(&c) {
                            set.push(c);
                        }
                    }
                }
            }
            Some(set)
        }
    }

    /// RGBA → 640×640 RGB CHW(/255, 정규화 없음 = YOLO 표준).
    fn preprocess(img: &DecodedImage) -> Option<Vec<f32>> {
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
            for c in 0..3 {
                out[c * plane + i] = p[c] as f32 / 255.0;
            }
        }
        Some(out)
    }
}
