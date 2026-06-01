# RawBlow 로딩 속도 개선 사이클 로그

대상 폴더: `D:\260320 한일교류회` — Panasonic RW2 3562장 × ~56MB.
증상(개선 전): 단일 고화질 5~15초, 그리드/스트립 화살표 이동 시 썸네일 표시까지 3분 이상.

## 측정 방법

- 프로파일러: `cargo run --release -p rawblow-core --example rw2_profile -- "<folder>" [count]`
- 핵심 지표는 **읽은 디스크 바이트**(`decode::BYTES_READ` 카운터). 느린/콜드 디스크에서 로딩 시간 ∝ 읽은 바이트 + file-open seek 수이므로, OS 캐시에 영향받는 벽시계 시간보다 바이트가 더 신뢰성 있는 지표다.
- 보조 지표: 벽시계 시간(워밍 캐시 기준 CPU 비용), file-open 수.

## 베이스라인 (워밍 캐시, 4파일 평균)

| 작업 | 시간 | 읽은 바이트 | 비고 |
|---|---|---|---|
| thumb(320) | 7ms | 0.52MB | 512KB prefix, OK |
| preview(1600) | 23ms | 1.05MB | 1MB prefix → 1920×1280, OK |
| **ORIG(8192)** | 419ms | **57.3MB** | rawloader 패닉 → 전체파일 폴백 |

### 근본 원인
1. **ORIG**: `rawloader-0.37.1`이 이 RW2의 타일 오프셋을 잘못 읽어 패닉(`range start … out of range`). `catch_unwind`가 잡고 `decode_largest_embedded`가 **전체 56MB를 읽어** 8144px 임베디드 디코딩. 콜드 디스크에선 imagepipe가 56MB 읽고 패닉 + 폴백이 56MB 또 읽음 = **2×56MB** → 5~15초. 또한 imagepipe(rayon)가 스레드마다 패닉 메시지를 stderr로 폭주.
2. **썸네일 플러드**: 배경 프리필이 폴더 열 때 3562개 전부를 큐에 적재 → 콜드 디스크 포화 → 보이는 셀이 진행 중 배경 읽기 뒤에서 굶음.

---

## 사이클

<!-- 각 사이클: 가설 → 변경 → 측정(바이트/시간) → 판정(채택/되돌림) → 커밋 -->

### Cycle 1 — 베이스라인 측정 + 계측 추가
- 변경: `decode::BYTES_READ` 바이트 카운터 추가(`read_prefix`/`read_whole`에 집계), 프로파일러 `rw2_profile` 작성.
- 결과: 위 베이스라인 확보. ORIG=57MB/419ms가 최대 병목, 배경 프리필 3562 플러드가 썸네일 플러드 원인으로 특정.
- 판정: 계측 채택. 다음 사이클부터 개선.
