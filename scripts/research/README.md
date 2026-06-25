# 컬링 추가 기준 — 가능 여부 검증 (#50 후속)

"연사 중 베스트만", "인물/풍경 위주 픽" 같은 **추가 컬링 인수**가 가능한지 코드로 검증한 결과.
2026-06-26 실측. 검증용 스크립트와 결과를 남긴다(프로덕션 통합은 별도).

## 결론 요약

| 기능 | 검증 방법 | 결과 | 상태 |
|---|---|---|---|
| **방향(세로/가로)** | Rust 단위테스트 | width/height로 즉시 | ✅ 구현됨 |
| **ISO·셔터·조리개·초점거리 임계** | Rust 단위테스트 | EXIF 파싱→임계 필터 | ✅ 구현됨 |
| **카메라/렌즈 한정** | Rust 단위테스트 | 부분일치 필터 | ✅ 구현됨 |
| **연사 그룹별 베스트-N** | Rust 단위테스트 | EXIF시각 그룹핑→그룹별 top-N | ✅ 구현됨 |
| **시각적 근접중복 클러스터링** | Rust 단위테스트 | dHash+해밍 union-find | ✅ 구현됨 |
| **장르(인물/풍경) 픽** | CLIP, 실이미지 7장 | 포트레이트 P=1.00 vs 풍경 0.00–0.10 | ✅ 검증됨 |
| **장면 다중분류** | CLIP, 실이미지 | 인물·풍경·동물·스포츠 7/7 정답 | ✅ 검증됨 |
| **선명도·구도·품질·커스텀 프롬프트 축** | CLIP, 실이미지 | 임의 안토님쌍으로 점수화 | ✅ 검증됨 |
| **의미적 임베딩 유사도** | CLIP, 실이미지 | 이미지 임베딩 코사인 | ✅ 검증됨 |
| **얼굴검출(인물 컬링)** | YuNet ONNX, 실이미지 | 포트레이트 1얼굴 conf0.95, 풍경 0 | ✅ 검증됨 |
| **눈 위치(→눈감음 토대)** | YuNet ONNX | 양 눈 좌표 출력 | ✅ 검증됨(분류기 후속) |
| **일반 객체검출(특정 피사체)** | YOLOv8n, 실이미지 | person:3·skateboard·dog 정확 | ✅ 검증됨 |

## Tier 1·2·3a — 순수 Rust (모델 불필요, 구현 완료)

`crates/rawblow-core/src/cull_ext.rs` + 단위테스트 7종(`cargo test -p rawblow-core --lib cull_ext`).
- EXIF 수치 파서, `MetaFilter`(방향·ISO·셔터·조리개·초점·카메라/렌즈)
- `group_bursts`(촬영시각 간격) + `select_best_per_group`(그룹별 top-N) — 기존 글로벌 `top_n`의 일반화
- `dhash`/`hamming`/`cluster_near_dups`(union-find) — 연사 아니어도 그림이 비슷하면 묶음
- 한계: SubSec 미파싱(초 해상도) → 초당 다수 연사 정밀화 시 `SubSecTimeOriginal` 추가 권장.

## Tier 3b — CLIP 프롬프트 축 (`clip_axes.py`)

`python clip_axes.py <image_dir>` (open_clip ViT-B/32). 실이미지 7장 실측:

```
image          quality sharp portrait compose  top-scene
pic10(코요테)   0.76  0.92   0.65    0.64   an animal (0.99)
pic27(해변)     0.86  0.92   0.10    0.59   a landscape (0.36)
pic40(스케이터) 0.72  0.72   0.04    0.50   street/sports (0.78)
pic55(피오르드) 0.79  0.84   0.00    0.40   a landscape (0.98)
portrait1(남)   0.57  0.70   1.00    0.71   portrait (0.96)
portrait2(여)   0.68  0.48   1.00    0.76   portrait (0.99)
```

핵심: **인물 P=1.00, 풍경 P=0.00–0.10**으로 완전 분리. 사람이 있어도 포트레이트가 아니면(스케이터 0.04)
정확. 프롬프트만 바꾸면 임의 축 측정 → **재export 없이** 장르·선명도·구도·커스텀 가능.
- 프로덕션: 모델을 "이미지 임베딩 출력"형으로 export + 프롬프트 임베딩 테이블 동봉(현 Good/Bad 1축 모델 대체).

## Tier 4 — 얼굴/객체 검출

- `face_detect.py <yunet.onnx> <dir>` — YuNet(232KB ONNX, ort로 실행): 포트레이트만 얼굴 1 + 눈 좌표(conf 0.95),
  풍경·동물 0. 눈 좌표 확보 → **눈감음/깜빡임은 눈영역 크롭 + 소형 분류기**로 이어붙이는 단계만 남음.
- `yolo_detect.py <dir>` — YOLOv8n(COCO 80클래스): 코요테→dog, 스케이터→person:3·skateboard:1, 풍경→none.
  카운트까지 정확("사람 2명 이상" 등 조건 가능). `.pt`→ONNX export로 ort 탑재 가능.

## 의존성(검증 전용, 앱 빌드와 무관)

`pip install torch open_clip_torch onnxruntime opencv-python ultralytics` (Python 3.14에서 휠 확인).
앱 런타임은 기존대로 Rust `ort`. 위 스크립트는 가능성 증명·후속 구현의 출발점.
