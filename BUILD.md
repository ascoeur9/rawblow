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

### Windows (PowerShell)
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
