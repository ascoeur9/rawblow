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

### Cycle 5 — 【핵심】 뷰포트 기반 LIFO+상한 스케줄러 (썸네일 3분의 진짜 원인 해결)
- 그라운드 트루스: 앱에 env `RB_BENCH` 자동스크롤 계측을 심어 실제 그리드를 20행/s로 훑으며 측정.
  - **이전(FIFO 무한 큐)**: `pend_thumb` 0→**3302**, `pend_pref` 0→**6561** 무한 증가. 워커가 prio/bg 큐를 **FIFO(오래된 것 먼저)**로 처리 → 빠른 스크롤로 쌓인 수천 개 backlog 뒤에 **현재 화면**이 줄서 디코딩이 수 분 지연. **이것이 "썸네일 3분"의 진짜 원인.** (디코드 자체는 5ms로 빠름 — Cycle 4 진단대로 큐잉 문제였음.)
- 변경: `worker.rs`를 채널 FIFO → **Mutex+Condvar 레인 스케줄러**로 재설계. 레인 우선순위 Preview>Thumb>Normal>Bg, 각 레인 **LIFO(최신=현재 화면 우선)** + **상한**(Thumb 256/Bg 192/Preview 4/Normal 32). 상한 초과 시 가장 오래된(지나간) 요청을 버리고 `dropped` 결과로 UI에 통지 → pending 해제 → 아직 보이면 재요청. app.rs는 썸네일→Thumb 레인, 현재 프리뷰→Preview 레인, 이웃→Normal, 프리페치→Bg로 라우팅 + drain에서 dropped 처리.
- 측정(앱 벤치, 동일 20행/s 풀스피드 44초): **가시 셀 캐시 적중 vis_cached = 72~80/80 (대부분 100%)**, 끝에서만 60/72. pend_thumb/pref **상한 내 안정**(≤260/≤200). frame ~8ms.
- 판정: **채택(핵심 수정)**. 스크롤 시 현재 화면 썸네일이 즉시 표시됨 = 3분 → 즉시.

### Cycle 6 — 워커 스레드 수 스윕 + env override
- 가설: 16코어인데 워커 6스레드(clamp 2..6)로 과소활용 → 더 많은 전경 스레드가 처리량↑.
- 변경: clamp 2..6 → 2..8, `RB_THREADS` env override 추가.
- 측정: 앱 벤치로 threads 6/8/12 스윕 → **워밍 캐시에선 셋 다 vis_cached 100%**(차이 없음). LIFO 스케줄러가 이미 전경을 보장하므로 스레드가 병목이 아님. 콜드 프리필엔 약간 도움.
- 판정: (2,8) 소폭 상향 **유지**(고코어 콜드 프리필 이득, 무해). RB_THREADS는 튜닝용으로 둠.

### Cycle 7 — JPG 썸네일도 prefix 임베디드 사용 (전체파일 읽기 제거)
- 발견: 117_PANA 하위폴더 JPG(6~9MB) 썸네일이 `decode_file`에서 **전체파일을 읽어 104ms**(RW2는 512KB/5ms). 벤치 후반 dip의 원인. 그런데 JPG 앞부분에 EXIF 임베디드 썸네일 존재(embPrefix=2).
- 변경: JPEG도 **썸네일 크기 요청이면 512KB prefix에서 임베디드 썸네일 디코딩**(RW2와 동일 경로, find_eoi 마커워킹이 본 이미지 가짜 EOI 방지), 없으면 전체 폴백.
- 측정: JPG 썸네일 **8.19MB/104ms → 0.52MB/2ms** (16× I/O, 50× 빠름). 출력 160×120(EXIF 썸네일, 그리드엔 충분). DCT 회귀 테스트 통과.
- 판정: **채택**. 이제 RW2·JPG 모두 썸네일이 512KB/≤5ms.
