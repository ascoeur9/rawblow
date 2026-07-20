# 개발 기획/핸드오프 — Windows 실행 불가 (Smart App Control) (#79)

> 작성 2026-07-20 · 대상 버전 v0.5.8(준비 중) · 이 문서는 **Windows 머신에서 이어서 작업**하기 위한 핸드오프.
> macOS 쪽에서는 진단·해결안·검증 절차까지만 정리했고(여기선 Windows 빌드/실행 검증 불가), 실제 수정·검증은 Windows에서 진행한다.

## TL;DR
- 증상: 설치 후 실행 시 `RawBlow.exe - 잘못된 이미지` 대화상자, `webgpu_dawn.dll` 오류, **오류 상태 `0xc0e90002`**. 앱이 시작조차 못 함.
- 진단(확정): `0xc0e90002` = **`STATUS_SYSTEM_INTEGRITY_POLICY_VIOLATION`**. Windows **Smart App Control(SAC)**이 **서명 안 된** `webgpu_dawn.dll`(ONNX Runtime WebGPU/Dawn, pyke `download-binaries` 커뮤니티 빌드)을 코드 무결성 정책으로 **로드 차단**한 것.
- `onnxruntime.dll`이 `webgpu_dawn.dll`을 **로드타임 임포트**하고, `RawBlow.exe`가 `onnxruntime.dll`을 로드타임 링크하므로, 차단이 `main()` 실행 **전**에 발생 → OS "잘못된 이미지" 대화상자가 먼저 뜨고 앱은 자체 안내조차 못 함.
- **VC++ 재배포 설치·DLL 추가로는 안 고쳐진다** (누락/손상이 아니라 정책 차단).
- 권장 해결: **`ort`를 `load-dynamic`으로 전환** → 앱은 정상 실행되고 AI 컬링만 안내와 함께 비활성(인증서 불필요). 완전 정상화는 **코드 서명**.

## 증상 (재현 조건)
- 클린 설치된 Windows 11 소비자 PC(한국어)에서 발생. 개발/업그레이드 머신에서는 재현 안 됨.
- 대화상자 원문: `C:\Program Files\RawBlow\webgpu_dawn.dll에 오류가 있거나 Windows에서 실행할 수 없는 이미지입니다. … 오류 상태 0xc0e90002.`
- 이슈 첨부 스크린샷: https://github.com/user-attachments/assets/0ca9a7d3-4096-4017-a8b3-8d0ae887768d (418×207, "잘못된 이미지" OS 대화상자)

## 진단 (근거)
`0xC0E90002` 디코드: severity=ERROR, **facility `0x0E9` = FACILITY_SYSTEM_INTEGRITY**, code `0x0002` → `STATUS_SYSTEM_INTEGRITY_POLICY_VIOLATION`.
- ReactOS/x64dbg NTSTATUS 헤더: `0xC0E90002 STATUS_SYSTEM_INTEGRITY_POLICY_VIOLATION`.
- Smart App Control 내부: 서명 안 됐거나 **평판 없는(unknown reputation)** 네이티브 이미지를 이 상태로 차단. `.dll`/`.exe`가 강제 대상. SAC는 **클린 Win11 설치본에서 기본 ON**.
- 대조되는 다른 실패(코드가 다름 → 혼동 금지):
  - 의존 DLL 누락 = `0xC0000135 STATUS_DLL_NOT_FOUND`
  - 32/64비트 불일치 = `0xC000007B`
  - 서명 해시 손상 = `0xC0000428 STATUS_INVALID_IMAGE_HASH`
  - Dawn D3D12 런타임에 `dxil.dll`/`dxcompiler.dll` 누락 = 로드 성공 후 런타임 "No supported adapters"(CPU 폴백) — 시작 시 Bad Image 아님

**왜 앱이 아예 안 뜨나:** `ort` 크레이트가 기본(비-load-dynamic)이라 `onnxruntime.dll`이 `RawBlow.exe`의 로드타임 임포트다. 로더가 프로세스 시작 시 `onnxruntime.dll → webgpu_dawn.dll` 체인을 해석하다 SAC에 막혀 프로세스가 뜨기 전에 죽는다.

## 안 되는 "해결책" (하지 말 것)
- VC++ 2015–2022 재배포 설치/동봉: 이 오류는 CRT 누락이 아니라 **정책 차단**이라 무관.
- `msvcp140_2.dll` 등 런타임 DLL 추가 동봉: 위와 동일한 이유로 무효.
- 그냥 재설치: 파일 손상이 아니므로 무효.
- (참고) `dxil.dll`/`dxcompiler.dll`은 별개 이슈용이며 이 차단과 무관.

## 해결 옵션 (트레이드오프)
| 방법 | 사용자 경험 | 비용/리스크 |
| --- | --- | --- |
| **① load-dynamic (권장)** | 앱은 정상 실행, AI 컬링만 안내 후 비활성 | 인증서 불필요 / 3플랫폼 링크 방식 변경 → **Windows·macOS 실행 검증 필수** |
| **② 코드 서명** | 모든 기능 완전 정상 (SAC가 서명·평판 신뢰) | 인증서 발급 필요(Microsoft Trusted Signing 또는 EV) |
| **③ 문서 안내만** | "잘못된 이미지" 뜨면 사용자가 SAC 끄기 | 코드 0 / SAC OFF는 **되돌리기 불가**·보안 저하·UX 최악 |

> ③의 사용자 워크어라운드가 나쁜 이유: SAC는 **앱별 예외 허용이 없고**, 한 번 끄면 Windows 재설치 전엔 다시 못 켠다(Microsoft 사양). 게다가 앱보다 OS 대화상자가 먼저 떠서 인앱 안내가 불가능하다.

---

## 권장 구현 — `ort` load-dynamic (Windows에서 작업)

목표: `onnxruntime.dll`/`webgpu_dawn.dll`을 **프로세스 시작이 아니라 AI 컬링을 실제로 쓸 때** 로드한다. 그러면 SAC 차단이 `main()` 이후 **캐치 가능한 런타임 오류**가 되어, 핵심 앱은 정상 실행되고 AI 컬링만 안내 후 비활성된다.

### 1) Cargo 피처
`crates/rawblow-core/Cargo.toml`의 `ort`에 `load-dynamic` 추가. (공통 `download-binaries` 라인 + OS별 라인 모두 동일 버전이라, 공통 라인에 features로 더하면 됨)
```toml
# 공통(현재): features = ["download-binaries"]  →  ["download-binaries", "load-dynamic"]
ort = { version = "=2.0.0-rc.12", features = ["download-binaries", "load-dynamic"], optional = true }
```
- `download-binaries`는 prebuilt를 계속 내려받아 `target/<profile>/`에 `onnxruntime.dll`(+webgpu 변형은 `webgpu_dawn.dll`)을 배치한다. `load-dynamic`은 **링크 방식만** 바꾼다(로드타임 → 런타임 dlopen).
- 참고: load-dynamic이면 `+crt-static` 충돌 이슈(BUILD.md의 "왜 crt-static을 못 쓰는가")가 완화될 수 있으나, EP DLL(webgpu_dawn 등) 동봉은 여전히 필요하니 단독 exe로 오해 말 것.

### 2) dylib 경로 지정 (`crates/rawblow-app/src/main.rs`)
`ort`는 첫 `Session::builder()`에서 지연 로드하며, load-dynamic이면 **`ORT_DYLIB_PATH`** 로 라이브러리를 찾는다(상대경로는 exe 기준). `RawBlowApp::new` 이전, `main()` 초반에 exe 옆 라이브러리를 가리키게 1회 설정:
```rust
// ORT(AI 컬링) 동적 로드: onnxruntime 라이브러리를 exe 옆에서 찾게 한다(#79 load-dynamic).
#[cfg(feature = "ai")]
if std::env::var_os("ORT_DYLIB_PATH").is_none() {
    if let Some(dir) = std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.to_path_buf())) {
        let lib = if cfg!(target_os = "windows") { "onnxruntime.dll" }
            else if cfg!(target_os = "macos") { "libonnxruntime.dylib" }
            else { "libonnxruntime.so" };
        std::env::set_var("ORT_DYLIB_PATH", dir.join(lib));
    }
}
```
- rawblow-app은 `ai` 피처(기본 포함, `crates/rawblow-app/Cargo.toml:18,23`)로 `rawblow-core/ai`를 켠다 → 위 `#[cfg(feature = "ai")]`가 맞다.
- **macOS 주의:** 릴리즈(.app) 배치 시 `libonnxruntime.dylib`이 실제로 실행 바이너리 옆에 있어야 한다. 현재 macOS 패키징은 `ort` 기본 링크의 rpath에 의존하므로, load-dynamic 전환 후 **dev(`cargo run`)·릴리즈 둘 다에서 AI 컬링이 여전히 되는지 반드시 확인**(경로 어긋나면 정상 머신에서도 AI가 깨진다 — 이게 이번 변경의 최대 리스크).
- dev 빌드에선 라이브러리가 `target/debug/`에 있으므로 exe(`target/debug/rawblow`) 옆이라 위 로직으로 잡힌다.

### 3) 세션 생성 실패의 우아한 처리 (확인)
`crates/rawblow-core/src/quality.rs:512~` `QualityModel::load*`은 이미 `Result<Self, String>`을 반환하고 `Session::builder()` 실패를 `map_err`로 전파한다(quality.rs:526). load-dynamic에서 SAC가 dylib 로드를 막으면 이 지점에서 Err가 나므로 **크래시 없이** 컬링 파이프라인 상단으로 올라온다. 컬링 시작부(`crates/rawblow-app/src/app/culling.rs`)에서 이 Err를 받아 사용자에게 표시하는 경로가 있는지 확인하고, **없으면 안내 토스트/결과 메시지로 표면화**한다.
- axes.rs:36 / face_detect.rs:32 / object_detect.rs:56 도 같은 `Session::builder()` 경로 — 각 `new`가 실패를 삼키지 말고 상위로 올려 컬링이 "AI 사용 불가"로 정직하게 끝나게 한다.

### 4) 인앱 안내 메시지 (i18n)
SAC 차단으로 세션 생성이 실패했을 때 원문 ort 에러 대신 사람이 읽는 안내를 보여준다. `crates/rawblow-app/src/i18n.rs`에 키 추가 예:
```
"AI 컬링을 시작할 수 없습니다 — 이 PC의 Windows Smart App Control이 AI 구성요소를 차단했습니다"
   => ("Can't start AI culling — Windows Smart App Control on this PC blocked an AI component",
       "AIカリングを開始できません — このPCのWindows Smart App ControlがAIコンポーネントをブロックしました")
```
- 오류 문자열에 `webgpu_dawn`/SAC/`0xc0e90002` 신호가 있으면 이 안내로 매핑, 아니면 일반 "AI 사용 불가" 문구로 폴백.

### 5) 패키징 (이미 되어 있음 — 확인만)
`scripts/build-release-windows.ps1`이 `target/release/*.dll`(= `onnxruntime.dll`, `webgpu_dawn.dll`, `dxcompiler.dll`, `dxil.dll`)를 exe 옆으로 복사한다. load-dynamic이어도 이 동봉은 그대로 필요(런타임에 찾음). `scripts/rawblow.nsi`도 `*.dll`을 설치하므로 변경 불필요.

## 검증 체크리스트 (Windows에서)
- [ ] **SAC ON 클린 Win11**(또는 SAC 평가/강제 모드)에서: 앱이 "잘못된 이미지" 없이 **정상 실행**되는가.
- [ ] 그 PC에서 AI 컬링 실행 시: 크래시 없이 **안내 메시지와 함께 비활성**되는가(핵심 뷰잉·컬링·전송은 정상).
- [ ] **SAC 없는 일반 Windows PC**에서: AI 컬링이 **예전처럼 정상 동작**하는가(회귀 없음 — dylib 경로가 맞는지).
- [ ] `RB_GPU_EP=webgpu`/GPU 고속 모드에서도 정상/우아한 폴백인지.
- [ ] `cargo clippy -p rawblow-core -p rawblow-app --no-default-features --features rawblow-app/update-check,rawblow-app/model-download --all-targets -- -D warnings` 통과(CI 게이트).
- [ ] (가능하면) 코드 서명 없이 배포한 setup.exe로 실제 차단→우아한 비활성 흐름 재현.

## macOS 검증 (여기서/또는 Windows 수정 반영 후)
- [ ] `cargo run -p rawblow-app`(기본 피처 = ai)로 AI 컬링이 여전히 되는가(ORT_DYLIB_PATH 경로가 dev에서 유효한지).
- [ ] macOS 릴리즈 번들에 `libonnxruntime.dylib`이 바이너리 옆에 있고 AI 컬링이 되는가.

## 대안 — 코드 서명(완전 정상화)
load-dynamic은 "앱은 뜨되 SAC PC에서 AI는 비활성"까지다. **모든 사용자에서 AI까지 정상**이 목표면 서명이 유일한 근본책:
1. **Microsoft Trusted Signing**(구 Azure Code Signing) 또는 **EV 코드서명 인증서** 취득.
2. `RawBlow.exe` + `onnxruntime.dll` + `webgpu_dawn.dll`(그 밖 동봉 DLL 포함) 전부 `signtool`로 서명.
3. `scripts/build-release-windows.ps1` 마지막에 서명 단계 추가(스테이징 후, NSIS 전). setup.exe도 서명.
- EV/Trusted Signing은 Microsoft Intelligent Security Graph 평판을 빠르게 쌓아 SAC가 신뢰한다. OV 인증서는 평판 누적 전까지 여전히 막힐 수 있다.

## 현재 상태(2026-07-20)
- v0.5.8 준비 커밋까지 푸시됨(main). 이슈 후속 5건 완료: #80 AF, #78 Undo, #75 디코드 회복, #76 토스트, #74 설정 제목.
- **남은 릴리즈 게이트: 이 #79.** 이 문서대로 Windows에서 수정·검증 후 돌아오면, macOS 쪽에서 새로 빌드해 **macOS 릴리즈**를 진행한다(사용자 계획).

## 출처
- NTSTATUS `0xC0E90002`: ReactOS `sdk/include/psdk/ntstatus.h`, x64dbg `ntstatusdb.txt`, MS-ERREF(Microsoft Learn).
- Smart App Control 차단 메커니즘: n4r1b "Smart App Control Internals" (part 2), Microsoft Learn "Smart App Control overview", geode-sdk `ModImpl.cpp`(동일 상태를 SAC로 매핑).
- ort load-dynamic / `ORT_DYLIB_PATH`: ort.pyke.io "Linking" 문서.
- ORT 런타임 의존(dxil/dxcompiler/dxcore, VC++ set): microsoft/onnxruntime issues #25495, #24771, discussion #27405; onnxruntime.ai 의존성 문서.
