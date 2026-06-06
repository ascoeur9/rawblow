# BUILD — 빌드 / 릴리스 노트

RawBlow를 빌드하고 배포용 바이너리를 만드는 방법, 그리고 버전 변경 이력입니다.

## 요구 사항

- **Rust** 1.80+ (`rustup`)
- C 링커
  - Windows: **MSVC Build Tools** (VCTools 워크로드 + Windows SDK)
  - macOS: Xcode Command Line Tools (`xcode-select --install`)
  - Linux: gcc/clang + Vulkan 런타임
- 한글 폰트는 OS 폰트에서 런타임 자동 로드

## 개발 빌드 / 실행

```bash
cargo run   --release -p rawblow-app     # 실행 (디스플레이 필요)
cargo build --release -p rawblow-app     # 산출물: target/release/rawblow(.exe)
cargo test  -p rawblow-core              # 코어 로직 단위 테스트
```

> 코어 테스트 중 일부(`decode_real_jpg` 등)는 저장소에 포함하지 않는 `sample/` 실파일을
> 요구하므로, sample이 없으면 실패하는 게 정상입니다(로직 테스트는 통과).

## 배포용 클린 릴리스 빌드 (중요)

배포 바이너리는 아래 두 가지를 반드시 적용합니다:

1. **정적 CRT** — Windows에서 VC++ 재배포 패키지 없이 단독 실행되도록.
   `.cargo/config.toml`의 `[target.x86_64-pc-windows-msvc] rustflags = ["-C","target-feature=+crt-static"]`로 자동 적용됨.

2. **빌드 경로 익명화 (`--remap-path-prefix`)** — 그냥 빌드하면 바이너리에
   `C:\Users\<이름>\.cargo\registry\…` 처럼 **빌드한 사람의 사용자명·홈 경로**가 박힙니다
   (의존성 패닉 위치 문자열). 공개 배포 시 정보 노출이므로 홈 경로를 `~`로 리맵합니다.
   *(Cargo의 `profile.trim-paths`가 stable이 되면 그걸로 대체 가능. 현재는 RUSTFLAGS 사용.)*

> ⚠️ **중요**: 환경변수 `RUSTFLAGS`를 설정하면 `.cargo/config.toml`의 `rustflags`가
> **완전히 덮어써집니다**(append 아님). 따라서 배포용 PowerShell/bash 명령에서는
> `+crt-static`을 직접 같이 넣어야 합니다 — 빠뜨리면 VCRUNTIME140.dll 의존이 부활해
> 테스터 PC(VC++ 재배포 미설치)에서 `LoadLibrary failed with error 126`로 실행 불가
> (이슈 #10, v0.2.9에서 발생).

### Windows (PowerShell) — 권장: 빌드 스크립트
```powershell
.\scripts\build-release-windows.ps1
# 산출물: target\release\rawblow.exe  (단독 실행, 사용자명 없음)
```

스크립트는 매 호출마다 `RUSTFLAGS`를 명시적으로 다시 설정하고, 빌드 직후
`dumpbin /dependents`로 **VCRUNTIME140.dll / api-ms-win-crt-*.dll 의존이
남았는지 자동 검증**합니다. 의존이 잡히면 종료 코드 1로 실패해서 그 바이너리는
배포되지 못합니다 — 이슈 #10이 v0.2.10/v0.2.11에서 재발한 적이 있어(셸 세션에
다른 `RUSTFLAGS`가 살아있던 케이스) 릴리스 빌드는 반드시 이 스크립트로 돌리는
걸 권장합니다.

### Windows (PowerShell) — 수동 빌드 (스크립트를 못 쓸 때)
```powershell
$env:RUSTFLAGS = "-C target-feature=+crt-static --remap-path-prefix=$env:USERPROFILE=~"
cargo build --release -p rawblow-app
# 산출물: target\release\rawblow.exe  (단독 실행, 사용자명 없음)
```

### macOS / Linux (bash/zsh)
```bash
# macOS/Linux는 +crt-static 무관 — remap만 적용
RUSTFLAGS="--remap-path-prefix=$HOME=~" cargo build --release -p rawblow-app
# 산출물: target/release/rawblow
```

### 검증 (Windows 배포 바이너리에 VCRUNTIME 의존이 없어야 함)
```powershell
# 결과에 VCRUNTIME140.dll / api-ms-win-crt-*.dll 이 나오면 정적 CRT가 빠진 것
dumpbin /dependents target\release\rawblow.exe | Select-String -Pattern "vcruntime|api-ms-win-crt"
# 정상 출력: (없음)
```

### 확인 (사용자명이 안 박혔는지)
```bash
# 결과가 0이어야 함 (경로는 ~\.cargo\… 형태로만 남음)
strings target/release/rawblow* | grep -c "$USER"     # macOS/Linux
```

## 릴리스 업로드 (gh)

```bash
gh release create vX.Y.Z "RawBlow-vX.Y.Z-<os>.exe" \
  --title "RawBlow vX.Y.Z" --notes-file notes.md --target main
```

- `.exe` 파일 아이콘은 `crates/rawblow-app/build.rs`가 로고를 Windows 리소스로 임베드(빌드 시 자동).
- 크래시 로그는 실행 중 패닉 시 **바탕화면 `rawblow_crash.log`**에 기록됩니다.

---

## 변경 이력 (Changelog)

### v0.5.0
- **전송 진행률 표시** — 용량이 크거나 느린 드라이브로 복사/이동할 때 멈춘 것처럼 보이던 문제 해결.
  전송을 **백그라운드 스레드**에서 돌리고 진행 상황(프로그레스바·n/총 파일·MB·현재 파일명)을 모달로
  표시. 진행 중 **취소** 가능(이미 옮긴 파일은 유지) (이슈 #35).
- **폴더 자동 분류·이동** — 셀렉 전송과 **별개**의 새 기능. 툴바 **정리** 버튼 → 폴더 안 사진을
  **촬영일 / 카메라 / 렌즈 / 확장자** 기준으로 하위폴더에 나눠 담음(이동 또는 복사). 촬영일·카메라·렌즈는
  EXIF로 분류(없으면 미상 폴더), RAW+JPG 페어는 같은 폴더로 유지. 확장자 기준은 파일 단위로 분리.
  충돌 시 자동 일련번호, 진행률·취소 지원. 분류 후 결과(하위폴더)를 바로 볼 수 있게 폴더를 다시 엶 (이슈 #34).
- **새 릴리즈 안내(#33)** — 기획 단계. 방식·의존성·UX는 `docs/plan-release-notify.md`에 정리, 구현은 보류.

### v0.4.1
- **`⌘` 표기 세로 어긋남 수정** — 폴백 폰트로 그려지던 `⌘` 글리프가 위로 떠 보이던 문제. 플랫폼별
  표기로 교체(Windows/Linux는 `Ctrl+`, macOS는 `⌘`) — 실제 키와도 일치 (이슈 #32).
- **일본어 글자 세로 어긋남 수정** — 여러 CJK 폰트를 폴백으로 섞어 쓰면서 일본어 한자가 다른 폰트
  (다른 세로 메트릭)로 그려져 항목의 첫/마지막 글자가 어긋나던 문제. **활성 UI 언어의 폰트를
  primary로 로드**(일본어 UI=일본어 폰트, 한국어 UI=한국어 폰트)해 한 언어의 텍스트가 한 폰트에서
  일관 렌더되도록 변경. 언어 변경 시 폰트도 즉시 교체.

### v0.4.0
- **다국어 지원** — 한국어 / English / 日本語. OS 언어를 기본으로 따르되 설정에서 변경·저장. 일본어
  신자체 글리프가 깨지지 않도록 CJK 폰트를 폴백 체인(한국어+일본어)으로 로드 (이슈 #30).
- **컬러 태그(분류 추가)** — 라벨·별점과 **독립**된 3번째 분류축. `⇧1~5`로 5색(주황·분홍·청록·파랑·보라,
  라벨 색과 겹치지 않게 선정), `⇧0` 해제. 설정에서 색별 커스텀 이름. 좌측 레일 부여·필터, 썸네일 점,
  사이드카 저장, 전송 선택·태그별 폴더 분기에 반영 (이슈 #27).
- **전송 시 파일명 변경** — 복사/이동하면서 순번(1,2,3)·별점등급(A1,B1…)·직접 템플릿
  (`{seq}` `{gradeseq}` `{stars}` `{label}` `{tag}` `{orig}` 등)으로 리네임. 라이브 미리보기, RAW+JPG
  페어 동일 이름, 충돌 시 일련번호. 원본 무손상(전송 시에만) (이슈 #26).
- **전체화면** — F11 또는 툴바 버튼으로 **OS 창 전체화면**(이전엔 앱 내부 표시만). 단일/그리드 어디서든,
  전체화면에서도 넘김·ORIG·확대 단축키 동작 (이슈 #31).
- **EXIF 촬영일시 표기** — `YYYY:MM:DD` → `YYYY.MM.DD HH:MM:SS` (이슈 #29).
- **툴바 정리** — 모든 버튼에 단축키 괄호 표기 통일, 중복 Filter 버튼 제거(좌측 레일 전담),
  점프/전송/일괄 버튼 다국어화.
- **README 영어·일본어 추가** — `README.en.md` / `README.ja.md`(한국어 최상단 유지).
- 테스트 카메라에 Fujifilm X-T5 추가, Thanks To에 아그네스 디지털 (이슈 #28).

### v0.3.0
- **썸네일 로딩 속도 대폭 개선** — 그리드/필름스트립을 빠르게 넘겨도 썸네일이 **밀리지 않고
  바로** 뜨도록 워커를 **뷰포트 기준 LIFO+상한 스케줄러**로 재설계. 현재 화면을 항상 먼저
  디코딩하고 쌓인 옛 요청은 버려, 수천 장 RW2 폴더에서 "썸네일이 몇 분씩 걸리던" 문제를 해결
  (원인은 디코딩 속도가 아니라 FIFO 무제한 큐의 백로그였음).
- **단일 파일 고화질(ORIG) 즉시 표시** — RAW에서 IFD가 가리키는 **풀해상도 임베디드 JPEG
  구간만** 읽어 디코딩(파일 전체 수십 MB 읽기와 rawloader 중복 디코딩 제거). 단일뷰 5~15초 →
  1초 이내.
- **느린 드라이브(외장 HDD/USB/네트워크) 대응** — 점진적 prefix(128KB→512KB)로 필요한 만큼만
  읽어 느린 저장소에서도 빠르게. 썸네일/프리뷰 전 경로가 파일 전체 대신 임베디드 구간만 읽도록 정비.
- **타 제조사 RAW 미리보기 최적화** — Nikon(NEF)·Sony(ARW)·Fujifilm(RAF)·Olympus(ORF)·
  Pentax(PEF·DNG)·Samsung(SRW)의 임베디드 프리뷰를 **SubIFD·IFD 체인**에서 직접 찾아 읽어
  미리보기 디스크 I/O를 최대 16배 감소. Canon(CR2)은 풀해상도 프리뷰를 **해상도 기준**으로
  정확히 선택(바이트 크기로 고르던 회귀 수정).
- **영구 썸네일 디스크 캐시·별점 등 v0.2.12 기능 포함** (이전 항목 참조).

### v0.2.12
- **별점(★1~5)** — 숫자 1~5로 부여, `(백틱)으로 해제. 라벨(QWER)과 독립으로 동시 부여.
  사이드카 저장/복원, 좌측레일·HUD·썸네일(★N) 표시, 그리드 다중선택 일괄 적용.
  전송은 라벨 OR 별점(합집합), 별점 칩은 각각 독립 다중 선택 (이슈 #23).
- **보기 별점 필터** — 좌측레일 `Filter Stars`(전체/★1~★5, **정확히 N점**)를 라벨 필터와
  **독립 AND**로 결합. 해당 점수 항목이 없으면 흐리게, 같은 칩 재클릭 시 전체로 토글.
- **썸네일 디스크 캐시** — OS 캐시폴더에 JPEG로 보관해 폴더를 다시 열어도 재디코딩 없이
  즉시 표시. 설정에서 용량 표시·비우기 + 사용자 지정 자동 상한(기본 1GB, 0=무제한, 초과 시
  오래된 것부터 백그라운드 정리). 캐시 키는 FNV-1a(툴체인 무관) (이슈 #22).
- **이동(Move) 후 폴더 재스캔**으로 옮겨진 항목을 목록에서 자동 정리. 폴더 전환·이동 전
  디바운스 대기 중인 분류/별점을 강제 저장해 유실 방지 (이슈 #24).
- **설정 화면** — 버전·GitHub Releases/Issues 링크·제작자·cosly.link 링크(클릭 시 브라우저) (이슈 #18).
- **고속 스크롤·폴더 전환 텍스처 크래시 수정** — `Texture ... has been destroyed`(wgpu 제출
  중 텍스처 파괴) 재발 방지. 은퇴 텍스처 유예를 **개수 기준 → 프레임 기준(TTL)**으로 바꿔
  churn 양과 무관하게 in-flight 참조가 끝난 뒤에만 GPU 텍스처를 파괴하고, 폴더 전환 시
  캐시 통째 교체(즉시 드롭) 대신 유예 경로로 비우도록 수정.

### v0.2.11
- **macOS 폴더 선택창 한국어화** — `.app` 번들 없는 단독 바이너리에서도 시스템 언어를
  따르도록 시작 시 `NSUserDefaults["AppleLanguages"]`를 시스템 선호 언어로 설정
  (이슈 #12). Windows 동작은 변화 없음(cfg 가드).
- README 테스트 카메라 목록을 제조사별로 재정리, Sony α7R III 추가.

### v0.2.10
- **Windows 11 실행 불가 수정** — `RUSTFLAGS` 환경변수가 `.cargo/config.toml`을 덮어써
  v0.2.9 바이너리가 `VCRUNTIME140.dll`에 동적 의존(LoadLibrary error 126). 정적 CRT 복귀
  (이슈 #10).
- **ORIG 세로 사진 가로로 표시 수정** — `imagepipe`가 회전한 결과를 한 번 더 회전해 90°
  잘못 표시되던 문제. 소니 A7R3 ARW에서 확인, 다른 RAW도 동일 경로 (이슈 #11).
- **파일명으로 일괄 분류** — 그리드에서 파일명 패턴으로 한 번에 Q/W/E/R 분류 (이슈 #3).
- **전송 후 폴더 열기 — macOS 강제종료 수정** — OS 탐색기로 위임 (이슈 #5).
- **니콘 렌즈명 EXIF 꼬리 빈값 제거** (이슈 #6).

### v0.2.9
- 폴더 열기 버튼(툴바), 전송 다이얼로그 모달 재구현.

### v0.2.8
- 배포 바이너리에서 **빌드 머신 사용자명·홈 경로 제거**(`--remap-path-prefix`로 익명화).

### v0.2.7
- **Canon CR2(5D / 5D Mark II) 단일뷰 표시 수정** — 무손실 JPEG(SOF3) 프리뷰 오선택으로
  일부 CR2가 단일뷰에 안 뜨던 문제 해결. (제보: Party!!)
- README에 감사(Thanks To) 섹션 추가.

### v0.2.6
- 로고 적용: 앱 화면·툴바·창 아이콘 + **.exe 파일 아이콘**(Windows 리소스 임베드).
- 라이선스 **All rights reserved**(독점)로 전환, 정적 CRT 단독 실행 exe.
- 크래시 로그를 바탕화면에 기록.

### v0.2.3 ~ v0.2.5
- **단일/전체화면 줌·이동**: 클릭 맞춤↔1:1, Ctrl+휠·핀치 줌, 드래그 이동, 배율 표시.
- **ORIG(원본 보기)**: RAW를 원본 해상도로 디코딩(기존 RAW 버튼 → ORIG).
- **그리드 다중 선택**(Ctrl/Shift+클릭) + Q/W/E/R 일괄 분류, ↑/↓ 행 이동 + 자동 스크롤.
- **EXIF 표시 수정**(RW2 임베디드 프리뷰에서 추출), 단일뷰 하단 도움말 제거.
- 그리드 고속 스크롤·키보드 내비 크래시 수정(텍스처 수명/프리뷰 churn).

### v0.2.0 ~ v0.2.2
- 회색 썸네일 문제 해결(임베디드 JPEG의 false EOI 파싱 수정), 그리드 멈춤·고착 해소.
- RW2 방향(세로/가로) 정확 표시, 빠른 로딩(주문형 디코딩·LRU 텍스처 캐시).
