# 개발 기획 — AF 포인트 표시 가능여부 검토 (#37)

> 상태: **구현 완료 (오너 육안 QA 대기)** — 2026-06-11, `rawblow-core/src/af.rs` + `A` 토글 오버레이.
> Canon AFInfo·AFInfo2 / Panasonic AFPointPosition+AFAreaSize / Sony FocusLocation 지원,
> 실파일 좌표는 ExifTool 13.59와 교차검증(통합 테스트 3종). 남은 것: §4-3 "실제 초점과 일치?" 육안 대조.
> 작성: 2026-06-08. 조사 환경: Linux 개발기(cc 링커 없음, 샘플은 파나소닉 RW2 위주 + Sony ARW 1장).

## 0. 이슈 요약

- 출처: 사용자 커뮤니티 요청(#37, 제보자 "그랜드캐논" = 캐논 사용자).
- 요청: **촬영 시 카메라가 잡은 초점 측거점(AF point)을 사진 위에 오버레이로 표시**해달라.
  ("혹시 AF포인트 보이게는 못함? 해줄 수만 있다면 너무 행복할 것 같음…")
- 오너 답변: "저도 모르는 영역이라 한 번 체크해보겠다" → 본 검토.
- 참고: FastRawViewer / Photo Mechanic / 제조사 순정 소프트(Canon DPP, Nikon NX Studio)가 제공하는 그 기능.

## 1. 결론 (먼저)

- **기술적으로 가능은 하나, "표준 기능 하나 붙이기"가 아니라 제조사별 리버스 엔지니어링 작업임.**
- 일부 바디(특히 소니·신형 니콘)는 사실상 어렵거나 불가. **한 번에 전 제조사 지원은 비권장.**
- **권장: 캐논·파나소닉 한정 best-effort로 시작**, "지원 바디에서만 표시·나머지는 조용히 미표시".
- **선행 조건: Windows PC의 다양한 실파일로 ExifTool 덤프 검증**(아래 4장 체크리스트). 이게 끝나야 실제 구현 착수 가능.

## 2. 왜 어려운가 (핵심 제약)

1. **AF 포인트는 표준 EXIF가 아니라 MakerNote(제조사 독자 영역)에 들어있음.**
   - 현재 사용 라이브러리 `kamadak-exif`(0.6.1)는 **표준 태그만** 읽고 MakerNote 내부는 풀어주지 않음.
   - 즉 라이브러리 호출 한 줄로 끝나는 일이 아니라, MakerNote IFD를 **직접 파싱**해야 함.
2. **제조사마다 포맷이 완전히 다름.** 이를 통합 지원하는 Rust crate는 없음.
   - 사실상 **ExifTool**(Perl, 20년 누적 커뮤니티 리버스 엔지니어링)이 유일한 레퍼런스.
   - 제조사별로 ExifTool 로직을 Rust로 **포팅**하는 셈. 신형 바디는 ExifTool에서도 매핑이 늦거나 빠질 수 있음.
3. **좌표 → 화면 매핑이 모델별로 제각각.** AF 좌표를 뽑아도 실제 표시까지 추가 작업 필요:
   - 기준 해상도가 실제 이미지 크기와 다를 수 있음(`AFImageWidth/Height` 별도 존재).
   - 원점(중심 기준 vs 좌상단 기준)·부호·세로/가로 orientation 보정이 바디마다 다름.
   - 앱의 **ORIG / 그리드 / 회전 / 확대(zoom·pan)** 좌표계와도 맞춰야 함.
4. **신뢰도 한계.** ExifTool조차 제조사·모델별로 AF 지원 편차가 큼 → 결과물은 본질적으로 "best-effort"(되는 바디만).

## 3. 실측 결과 (Linux 개발기, 저장소 `sample/` 실파일)

> 주의: 아래는 **바이트 단위 직접 조사** 결과임. 손수 짠 임시 파서라 일부는 "데이터 존재"까지만 확인했고
> "좌표 디코딩 성공"까지는 못 간 항목이 있음(정직하게 구분 표기).

| 제조사 | 샘플 | 확인된 사실 | 판정 |
|---|---|---|---|
| **Panasonic RW2** (DC-S1RM2 / DC-S1M2) | 187장 | Panasonic MakerNote 시그니처(`Panasonic\0\0\0`)가 파일 앞 **~7.6KB 지점**(앱이 이미 읽는 임베디드 JPEG EXIF 범위 내)에 **물리적으로 존재**. 단, 표준 IFD 네비게이션으로는 곧장 못 닿음(RW2 특유 비표준 배치) → **AFPointPosition 좌표 실디코딩은 실패**, 전용 파서 필요. | **데이터 있음 / 추출은 전용 파서 필요** |
| **Sony ARW** (1장) | `_DSC0504.ARW` | 표준 위치(ExifIFD의 0x927C MakerNote)에 MakerNote가 **아예 없음**. 소니는 별도 위치 + 난독화(enciphered). | **가장 어려움** |
| **Canon CR2/CR3** | **샘플 없음** | 미검증. (도메인 지식) Canon `AFInfo2`는 ExifTool 기준 측거점 좌표가 명시적이라 5대 제조사 중 가장 구현 친화적. 테스트 바디(5D/5D2)도 구형이라 호환 가능성 높음. | **미검증 / 가장 유망** |
| Nikon NEF | 샘플 없음 | 미검증. (도메인 지식) 일부 MakerNote 암호화, Z 시리즈 신형은 포맷 상이 → 난이도 중상. | **미검증 / 중상** |
| Fujifilm RAF | 샘플 없음 | 미검증. ExifTool 부분 지원. | **미검증 / 중** |

- 핵심 시사점 1: **파나소닉은 데이터가 앱이 이미 읽는 앞부분에 있다** → 새 I/O 경로는 불필요, MakerNote 파서만 추가하면 됨.
- 핵심 시사점 2: **요청자(캐논) 샘플이 하나도 없다** → 캐논 우선 진행하려면 제보자에게 CR2/CR3 샘플 확보가 선행.
- 핵심 시사점 3: **오너 바디(S1R II/S1 II)는 2025년 최신** → ExifTool 매핑이 최신이 아닐 수 있어 좌표 검증 스파이크 필수.

## 4. Windows 빌드 PC 검증 체크리스트 (★ 이 문서의 핵심)

> 개발기에는 파나소닉 위주 샘플뿐이고 cc 링커가 없어 깊은 검증 불가.
> **Windows PC에는 제조사별 샘플이 다양하므로, 거기서 ExifTool로 먼저 "무엇이 나오는지" 확정**한 뒤 구현 범위를 정한다.

### 4-1. 사전 준비
- ExifTool 설치: https://exiftool.org → `exiftool(-k).exe`를 `exiftool.exe`로 리네임 후 PATH 등록.
- 확인: `exiftool -ver`

### 4-2. 제조사별 AF 태그 덤프 (바디마다 1~2장씩)
```
exiftool -a -G1 -s -AF* -Focus* "사진파일"
exiftool -a -G1 -s -makernotes "사진파일"     # MakerNote 전체(원시 확인용)
```
- 보고 싶은 핵심 태그(제조사별):
  - **Canon**: `AFAreaXPositions` `AFAreaYPositions` `AFAreaWidths` `AFAreaHeights`
    `AFImageWidth` `AFImageHeight` `NumAFPoints` `AFPointsInFocus` `AFPointsSelected` `AFAreaMode`
  - **Nikon**: `AFInfo2Version` `AFAreaMode` `PrimaryAFPoint` `AFPointsUsed`
    `ContrastDetectAF` `AFAreaXPosition` `AFAreaYPosition` `AFAreaWidth` `AFAreaHeight`
  - **Sony**: `AFAreaMode` `AFPointSelected` `AFPointsUsed` `FocalPlaneAFPointsUsed` `AFPoint`
    (값이 안 나오거나 enciphered면 → 지원 제외 후보)
  - **Fujifilm**: `AFAreaMode` `AFAreaPointSize` `AFAreaZoneSize` `FocusPixel`
  - **Panasonic**: `AFPointPosition` `AFAreaMode` `FacesDetected` `Face1Position` …

### 4-3. 검증 매트릭스 (Windows에서 채워서 회신)
바디별로 아래 표를 채우면 구현 우선순위·범위가 확정됨.

> **✅ 2026-06-10 Windows 빌드 PC에서 검증 완료** (ExifTool 13.59, 저장소 sample/ 실파일).
> 결론: **보유 3개 제조사 모두 AF 좌표가 실제 기록됨** — B안(네이티브 파서) 착수 가능.

| 제조사/바디 | 포맷 | AF 좌표 태그 나옴? | 좌표계(원점/기준해상도) | 실제 초점과 일치? | 비고 |
|---|---|:---:|---|:---:|---|
| Canon EOS 5D | CR2 | **Y** (15점) | `AFAreaX/YPositions` 중심 원점, `AFImageWidth/Height`(4992×3328) 기준 | 육안 대조 필요 | `AFAreaWidths/Heights` 미기록 → 고정 크기 박스 폴백 |
| Canon EOS 5D Mark II | CR2 | **Y** (9점) | 좌표 + **폭/높이 완비**, 중심 원점, `AFImageWidth/Height`(5616×3744) 기준 | 육안 대조 필요 | `AFPointsInFocus`로 합초점 구분까지 가능 — 가장 완전 |
| Panasonic S1R II (DC-S1RM2) | RW2 | **Y** | `AFPointPosition`(0~1 정규화 중심) + `AFAreaSize`(0~1 폭/높이) | 육안 대조 필요 | `AFSubjectDetection`(Human Eye/Face/Body)도 기록 |
| Panasonic S1 II (DC-S1M2) | RW2 | **Y** | 상동 | 육안 대조 필요 | 세로 사진도 동일 기록 — orientation 보정 검증 필요 |
| Sony A7R III (ILCE-7RM3) | ARW | **부분** | `FocusLocation` = (이미지W, H, X, Y) 픽셀 좌표 1점 | 육안 대조 필요 | §3의 "가장 어려움" 판정 완화 — 점 1개 표시는 가능, 영역 크기 없음 |

- 시사점: 캐논(요청자)·파나소닉(오너) 모두 데이터 완비 → §6 권장안 그대로 진행 가능.
  파나소닉 신형도 ExifTool 13.59가 완전 디코딩(정규화 좌표라 변환 가장 단순).
- 남은 선행 과제는 "실제 초점과 일치?" 열의 **육안 대조**(오너 확인 필요)뿐.

- "AF 좌표 태그 나옴?" = `AFAreaXPositions`류가 **숫자로** 출력되는지(빈값·Unknown이면 N).
- "실제 초점과 일치?" = 사진을 보면서 초점 맞은 위치와 좌표가 대략 맞는지 육안 대조.

### 4-4. 좌표 매핑 확인 포인트(구현 직결)
- 기준 해상도: `AFImageWidth/Height`가 실제 이미지 px와 같은지/다른지 → 비율 환산 필요 여부.
- 원점·부호: 좌상단(0,0) 기준인지, 중심 기준인지, Y축 방향.
- orientation: 세로 사진에서 좌표가 회전 보정 전/후 값인지.
- RAW+JPG/임베디드: RW2처럼 **임베디드 JPEG EXIF에서 읽어야 하는** 케이스 구분(파나소닉 해당).

## 5. 옵션 비교

| 안 | 내용 | 평가 |
|---|---|---|
| **A. ExifTool 위임** | 앱에서 ExifTool 호출해 AF 좌표 수신 | 정확도·범위 최고. 단 단독 바이너리 철학(README "설치·런타임 불필요") 위배, 번들 5~50MB. "설치돼 있으면만" 옵션화는 가능하나 일반 사용자엔 부적합 → **비권장** |
| **B. 네이티브 Rust 파서, 제조사 점진 추가** | MakerNote 직접 파싱, 캐논·파나소닉부터 best-effort | **권장.** "지원 바디만 표시·나머지 미표시". 모델별 좌표 검증 필수, 제조사 추가마다 별도 공수 |
| **C. 보류** | 검증 후 공수/불확실성 보고 재판단 | 합리적 선택지. 4장 검증 결과가 부정적이면 자연스럽게 여기로 |

## 6. 권장안 (요약)

1. **(선행) Windows PC에서 4장 검증 매트릭스 작성** — 어떤 바디가 실제로 AF 좌표를 내놓는지 확정.
2. **구현은 B안 + 범위 축소** — 캐논(요청자·가장 쉬움) → 파나소닉(오너·데이터 확인됨) 순.
   - 캐논 진행 시 **제보자에게 CR2/CR3 샘플 요청**(현재 0장).
   - 파나소닉은 **S1R II/S1 II 좌표 검증 스파이크** 후 착수(신형이라 매핑 불확실).
3. **표시 UX(초안)**: ORIG/단일뷰에서 토글(예: 기존 EXIF=I, 히스토그램=H 옆에 새 단축키). 측거점은 사각형/점 오버레이.
   강제 아님·없으면 조용히 미표시. (확정은 구현 단계에서.)
4. **에러/없음 처리**: 태그 없음·파싱 실패·미지원 바디는 **조용히 미표시**(앱 동작 영향 0).

## 7. 공수 감각

- "며칠짜리 기능 하나"가 아니라 **제조사별로 붙여가는 누적형** 작업.
- 1제조사 = MakerNote 전용 파서 + AF 태그 디코딩 + 좌표 매핑(원점/해상도/회전) + egui 오버레이(ORIG/그리드/zoom 대응) + 실파일 검증.
- 중간 규모이며 제조사가 늘수록 누적. 신형 바디는 매핑 부재 리스크 상존.

## 8. 미결정 사항 (오너 확정 필요)

1. **진행 여부 / 범위** — 캐논+파나소닉 한정 best-effort로 갈지, 보류할지.
2. **우선순위** — 요청자 배려(캐논 1순위) vs 데이터 확인된 파나소닉(오너 본인) 1순위.
3. **캐논 샘플 확보** — 제보자에게 CR2/CR3 요청할지.
4. **표시 방식/단축키** — 오버레이 모양·토글 키(구현 단계 결정 가능).

## 9. 참고 — 조사에 쓴 방법(재현용)
- 시그니처 탐색: 파일에서 `Panasonic\0\0\0`(파나소닉 MakerNote 헤더) 바이트 검색 → 위치/임베디드 JPEG SOI(`FF D8 FF`) 위치 대조.
- IFD 파싱: TIFF 헤더(II/MM) → IFD0 → ExifIFD(0x8769) → MakerNote(0x927C) → 제조사 IFD 순 직접 워킹.
- 레퍼런스: ExifTool 소스의 제조사 모듈(Canon.pm / Panasonic.pm / Nikon.pm / Sony.pm / FujiFilm.pm)의 AF 관련 태그 정의.
