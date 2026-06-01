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

### Cycle 2 — 배경 프리필 플러드 제거 (썸네일 3분의 직접 원인)
- 가설: 폴더 열 때 3562개 전부를 bg 큐에 적재 → 느린 디스크 포화 → 보이는 셀(prio)이 굶음.
- 변경: `request_prefetch_thumbs`(전체) → `request_prefetch_window`(현재 index 주변 BEHIND=16/AHEAD=80만, update에서 매 프레임 슬라이드). open_folder의 전체 enqueue 제거.
- 측정: 폴더 열 때 enqueue 수 **3562 → ≤96**. 윈도우가 이동을 따라가므로 디스크 부하는 폴더 크기와 무관하게 일정. 보이는 셀의 prio 요청이 더 이상 수천 건 배경 읽기 뒤에 줄서지 않음.
- 판정: **채택**. (남은 디스크 경합은 Cycle 4에서 bg 동시성 제한으로 추가 완화.)

### Cycle 3 — ORIG 15초 해결: IFD 기반 풀해상도 임베디드 읽기
- 발견: `ifd_dump` 프로브로 RW2 IFD0 구조 확인. tag `0x002e`(JpgFromRaw)=offset6144/628KB(1920 프리뷰), tag **`0x0127`=offset649216/8.5MB(풀해상도 8144×5424 JPEG)**.
- 가설: ORIG는 rawloader가 56MB 읽고 패닉 + 폴백이 56MB 또 읽음(2×56MB). 풀해상도 JPEG가 이미 IFD에 있으니 그 구간만 읽으면 됨.
- 변경: `tiff_ifd0_jpeg_blobs`(헤더 64KB만 파싱 → type7 블롭 offset/len) + `read_range`(seek+구간 읽기) + `decode_largest_ifd_embedded`. decode_file ORIG가 **IFD 임베디드 우선**, 큰 임베디드(≥3000px) 없을 때만 rawloader.
- 측정: ORIG **57.3MB → 8.04MB read (7.1× 감소)**, 패닉 **수십 줄 → 0**, 시간 419→370ms(워밍). 콜드 디스크 기준 2×56MB→8MB ≈ **14× 적은 I/O** → 15초의 핵심 해소. 출력 8144×5424 동일(무회귀).
- 판정: **채택**.

### Cycle 4 — bg 동시성 제한 + 썸네일 병목 진단
- 변경: 워커 bg(프리페치) 레인을 최대 2스레드만 처리(`bg_threads=(threads/2).clamp(1,2)`), 나머지는 prio·일반만 → 느린 디스크에서 전경 대역폭 확보.
- 진단(중요): 썸네일 병목을 정밀 측정한 결과,
  - `thumb_scan` 300파일: 전체파일 폴백 **0%**, 장당 **0.52MB**(512KB prefix), 콜드에서도 **5.4ms/장**(D:는 빠른 SSD), 워밍 1.6ms.
  - `store_bench`: 디스크 캐시 store **1.46ms/장**(Defender 영향 없음), 3562개 ~5초.
  - 즉 **디코드/저장/읽기 파이프라인은 콜드에서도 빠르다(3562개 ≈ 19초 단일스레드, ~5초 4스레드)**. "썸네일 3분"은 디코드가 아니라 **폴더 전체(3562) 프리필 플러드**(직전 세션 Cycle C 회귀)가 앱 루프와 cold 디스크에서 일으킨 것 → Cycle 2(윈도우)로 구조적 제거.
- 판정: bg 제한 **채택**(워밍 throughput 손해는 4ms로 미미, 콜드 전경 응답성 이득). 다음: 실제 앱 계측으로 그라운드 트루스 확보.
