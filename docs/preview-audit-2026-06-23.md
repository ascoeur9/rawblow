# 프리뷰 성능/해상도 개선 가설 검토 — 2026-06-23

작업 브랜치: `perf/preview-optimization-20260623`

## 목표

현재 디코딩 구조에서 같은 파일을 더 빠르게 보거나, 같은 파일을 더 높은 해상도의 미리보기로 볼 수 있는지 가설을 세우고 검토했다. 유의미한 구현안으로 **기본 단일/전체화면 프리뷰 해상도 상향**을 선택했다.

## 관찰한 현재 구조

- 앱 기본 단일/전체화면 프리뷰 요청: `PREVIEW_EDGE = 1600`
- RAW 빠른 프리뷰 경로: `full_raw=false`에서 RAW 전체 현상 대신 IFD가 가리키는 임베디드 JPEG를 사용
- ORIG(`D`) 경로: 풀해상도 임베디드 JPEG 또는 full RAW 현상 사용
- 썸네일 요청: `THUMB_EDGE = 320`

## 샘플 바이트 분석

저장소 샘플 루트의 Panasonic RW2 3장을 직접 파싱했다.

| 파일 | 작은 프리뷰 | 풀해상도 임베디드 |
|---|---:|---:|
| `PANA2740.RW2` | 1920×1280, 0.60MB, offset 6144 | 8144×5424, 7.91MB, offset 610816 |
| `PANA2799.RW2` | 1920×1280, 0.78MB, offset 6144 | 8144×5424, 8.51MB, offset 782848 |
| `PANA2878.RW2` | 1920×1280, 0.88MB, offset 6144 | 8144×5424, 10.83MB, offset 886784 |

또한 앞 128KB/512KB에는 160px EXIF 썸네일만 완전히 들어 있고, 1920px 프리뷰는 1MB prefix에서야 완전히 들어온다.

## 가설과 판단

### 가설 A — 기본 프리뷰를 더 빠르게

- 기존 1600급 프리뷰는 이미 IFD offset으로 작은 1920px JPEG 구간만 읽는다.
- 샘플 기준 읽는 바이트는 1MB 미만이며, 전체 RAW(55~63MB)를 읽지 않는다.
- 더 빠르게 만들 여지는 있었지만, 큰 구조 변경 없이 얻는 이득은 제한적이었다.
- 예: 후보 dimension probe를 줄이는 안은 작은 프리뷰의 SOF가 약 52KB 뒤에 있어 64KB probe를 유지해야 안전했다.

판단: 이번 브랜치의 주 개선축으로 채택하지 않음.

### 가설 B — 기본 프리뷰를 더 높은 해상도로

- 같은 RAW 파일 안에 이미 8144px JPEG가 들어 있다.
- full RAW 현상보다 훨씬 가볍게, 이 JPEG를 DCT 축소해 2560px 프리뷰를 만들 수 있다.
- 기존 디코더는 `max_edge=2560`을 요청하더라도 RAW 빠른 프리뷰 후보 선택 기준이 1600급으로 고정되어 1920px 후보를 고르는 구조였다.

판단: 유의미함. 4K급 화면에서 FIT 기본 미리보기가 덜 물러 보이고, ORIG 전환 없이 더 선명한 확인이 가능하다.

## 구현 내용

1. `rawblow-core/src/decode.rs`
   - RAW 빠른 프리뷰(`full_raw=false`)에서 `max_edge`를 후보 선택 기준으로 반영하도록 변경했다.
   - `max_edge=None`인 하위호환 호출은 기존처럼 2048px 상한/1600px 이상 후보 선택을 유지한다.
   - IFD가 없는 fallback 스캔에서도 요청 긴변 이상 후보를 우선하도록 `decode_best_embedded`에 `preview_min_long_edge` 기준을 추가했다.

2. `rawblow-app/src/app.rs`
   - 기본 단일/전체화면 프리뷰 요청을 `1600px`에서 `2560px`로 상향했다.
   - RAW에서는 작은 1920px 후보 대신 풀해상도 임베디드 JPEG를 DCT 축소해 사용한다.
   - ORIG(`D`)는 그대로 8192px이며, 기본 프리뷰와 ORIG의 역할은 유지한다.

3. `rawblow-core/tests/core_tests.rs`
   - 합성 RW2에 1920px/3200px 임베디드 JPEG를 넣고 다음을 검증하는 테스트를 추가했다.
     - `max_edge=1600` → 작은 프리뷰에서 1600px 결과
     - `max_edge=2560` → 큰 임베디드에서 2560px 결과

## 기대 효과

- 기본 단일/전체화면 미리보기의 긴변이 1600/1920급에서 2560px급으로 올라간다.
- RAW full develop 없이 카메라 내장 JPEG를 사용하므로 ORIG보다 가볍다.
- 고해상도 모니터에서 FIT 상태 확인 품질이 좋아진다.

## 리스크/트레이드오프

- 기본 프리뷰가 작은 0.6~0.9MB JPEG 대신 8~11MB급 임베디드 JPEG를 읽을 수 있어, 느린 외장/네트워크 드라이브에서는 첫 프리뷰 시간이 늘 수 있다.
- 썸네일은 기존 속도 우선 경로를 유지했다. 앞 128KB의 160px EXIF 썸네일을 쓰는 현상은 별도 품질 옵션으로 다루는 편이 안전하다.
- 이번 변경은 “더 빠름”이 아니라 “더 높은 기본 미리보기 해상도”에 초점을 둔 안이다.

## 실행한 검증

### 성공

- 샘플 RW2 바이트 직접 분석으로 1920px/8144px 후보 존재 확인
- `git diff --check` 통과
- `rustfmt --check`를 변경 파일에 대해 실행해 Rust 파서가 통과함을 확인했다. 단, 기존 테스트 파일의 광범위한 포맷 diff 때문에 exit code는 1이었다.
- 합성 회귀 테스트 코드 추가

### 환경 제약

현재 Linux 환경에는 시스템 C 링커/개발 파일이 없다.

- `cc`, `gcc`, `clang` 없음
- `sudo`는 비밀번호 필요
- `cargo check -p rawblow-core`는 dependency build script 링크 단계에서 `linker cc not found`로 실패

빌드 가능한 환경에서 다음 명령으로 최종 확인이 필요하다.

```bash
cargo fmt --all -- --check
cargo check --workspace
cargo test -p rawblow-core raw_preview_uses_hires_if_requested_edge_exceeds_small_preview
cargo test --workspace
```
