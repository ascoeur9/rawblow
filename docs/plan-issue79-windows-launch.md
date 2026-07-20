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

## 옵션 ④ — MS 스토어(MSIX) 배포로 SAC 우회 (권장 대안, 무료 서명)

스토어는 **MSIX/APPX 패키지를 무료로 재서명**한다(MSI/EXE는 안 해줌 — Learn app-package-requirements, 2026-07-17). MSIX는 **패키지 전체가 하나의 공통 카탈로그 서명**으로 신뢰되므로, 안에 든 **미서명 `webgpu_dawn.dll`·`onnxruntime.dll`도 포함으로 신뢰**되어 SAC의 `0xc0e90002` 차단이 사라진다. **DLL 손대지 않고, load-dynamic 없이도** 해결.

RawBlow 적합성(코드 확인 완료): config→`%APPDATA%\RawBlow`(config.rs:619), 캐시→`%LOCALAPPDATA%`(config.rs:651), **설치 폴더 쓰기 없음** → MSIX(풀트러스트)에서 유일한 파괴 지점(설치폴더 쓰기)이 없음. 임의 사진 폴더 사이드카 쓰기도 풀트러스트라 통과(`broadFileSystemAccess` 불필요). → **코드 변경 없이 재패키징만으로 가능성 높음.**

> ⚠ "패키지 서명이 내부 미서명 DLL까지 신뢰시킨다"는 결론은 근거는 탄탄하나 일부 추론 → **SAC 켜진 클린 Win11 1대에서 사이드로드로 실검증**(아래 5단계) 후 확정.

### 사전 준비
- **Windows 10/11 SDK** 설치(→ `makeappx.exe`·`signtool.exe`·WACK 제공). **Developer Command Prompt**에서 작업.

### 1) 스토어에서 이름 예약 + Identity 값 확보
1. Partner Center → **Apps and games** → **New product** → **MSIX or PWA app** → 이름 입력 → **Check availability** → **Reserve product name**("RawBlow").
2. 좌측 **Product management → Product identity**에서 **3개 값 그대로 복사**(대소문자·구두점 민감):
   - **Package/Identity/Name** → 매니페스트 `<Identity Name="…">`
   - **Package/Identity/Publisher** → `<Identity Publisher="CN=…">` (스토어가 배정한 문자열, 내 이름 아님)
   - **Package/Properties/PublisherDisplayName** → `<Properties><PublisherDisplayName>`
   - (PFN·SID는 매니페스트에 안 넣음)

### 2) `AppxManifest.xml` 작성 (위 3개 값 + RawBlow 값으로)
```xml
<?xml version="1.0" encoding="utf-8"?>
<Package
  xmlns="http://schemas.microsoft.com/appx/manifest/foundation/windows10"
  xmlns:uap="http://schemas.microsoft.com/appx/manifest/uap/windows10"
  xmlns:uap10="http://schemas.microsoft.com/appx/manifest/uap/windows10/10"
  xmlns:rescap="http://schemas.microsoft.com/appx/manifest/foundation/windows10/restrictedcapabilities">
  <Identity Name="[Partner Center Name]" Publisher="CN=[Partner Center Publisher]"
            Version="0.5.8.0" ProcessorArchitecture="x64" />
  <Properties>
    <DisplayName>RawBlow</DisplayName>
    <PublisherDisplayName>[Partner Center PublisherDisplayName]</PublisherDisplayName>
    <Logo>Assets\StoreLogo.png</Logo>
  </Properties>
  <Resources><Resource Language="en-us" /></Resources>
  <Dependencies>
    <TargetDeviceFamily Name="Windows.Desktop" MinVersion="10.0.19041.0" MaxVersionTested="10.0.22631.0" />
  </Dependencies>
  <Capabilities><rescap:Capability Name="runFullTrust" /></Capabilities>
  <Applications>
    <Application Id="RawBlow" Executable="RawBlow.exe"
        uap10:RuntimeBehavior="packagedClassicApp" uap10:TrustLevel="mediumIL">
      <uap:VisualElements DisplayName="RawBlow" Description="빠른 RAW 사진 컬링 뷰어"
        Square150x150Logo="Assets\Square150x150Logo.png"
        Square44x44Logo="Assets\Square44x44Logo.png" BackgroundColor="#464646" />
    </Application>
  </Applications>
</Package>
```
- `Version`은 **4번째 자리 반드시 0**(스토어 예약), 첫 자리 0 불가 → 릴리즈 버전이 x.y.z면 `x.y.z.0`.
- `ProcessorArchitecture="x64"`, `MinVersion="10.0.19041.0"`(packagedClassicApp+mediumIL 요구), `runFullTrust`(풀트러스트 Win32 필수).
- **번들 DLL은 매니페스트 선언 불필요** — 그냥 패키지 파일로 포함. 개별 서명도 불필요(BlockMap이 전 파일 해시, 패키지 서명이 커버).
- `broadFileSystemAccess`는 **넣지 말 것**(mediumIL은 이미 사용자 파일 전권 → 불필요·심사 가중).

### 3) 아이콘 에셋 3종 (`Assets\`)
`StoreLogo.png`(50×50), `Square150x150Logo.png`, `Square44x44Logo.png`. 기존 로고(`crates/rawblow-app/src/logo.rs`)에서 PNG로 뽑아 만들면 됨.

### 4) 스테이징 + 패키징 (기존 빌드 스크립트 재활용)
`scripts/build-release-windows.ps1`이 이미 `RawBlow.exe + 전체 DLL`을 스테이지 폴더에 모은다. 그 폴더 **루트에 `AppxManifest.xml` + `Assets\`** 를 얹고:
```
makeappx pack /d <스테이지폴더> /p RawBlow.msix /o
```
(→ 빌드 스크립트에 makeappx 단계를 추가하면 NSIS와 병행 산출 가능)

### 5) 로컬 사이드로드 실검증 (제출 전 필수)
자체서명 인증서의 `-Subject`를 **매니페스트 Publisher와 동일**하게 → 같은 .msix로 테스트·제출.
```powershell
New-SelfSignedCertificate -Type Custom -KeyUsage DigitalSignature `
  -Subject "CN=[Partner Center Publisher]" -CertStoreLocation "Cert:\CurrentUser\My" `
  -TextExtension @("2.5.29.37={text}1.3.6.1.5.5.7.3.3","2.5.29.19={text}")
$pw = ConvertTo-SecureString "Test123!" -Force -AsPlainText
Export-PfxCertificate -cert "Cert:\CurrentUser\My\<Thumbprint>" -FilePath .\dev.pfx -Password $pw
Import-PfxCertificate -CertStoreLocation "Cert:\LocalMachine\TrustedPeople" -FilePath .\dev.pfx -Password $pw
SignTool sign /fd SHA256 /a /f .\dev.pfx /p "Test123!" RawBlow.msix
Add-AppxPackage .\RawBlow.msix
```
→ 실행해서 **AI 컬링(DLL 로드)·`%APPDATA%`/`%LOCALAPPDATA%` 정상**인지 확인. **가능하면 SAC 켜진 클린 Win11에서** 사이드로드해 차단 안 됨을 확인. 이어 **WACK(Windows App Certification Kit)** 로 패키지 검사.

### 6) Partner Center 제출 (섹션별)
앱 overview → **Start submission**:
- **Pricing and availability**: Base price **Free**, Markets 전체.
- **Properties**: **Category**(Photo & video). 개인정보 전송이 있으면(업데이트 확인·AI 등) **Privacy policy URL 필수**(§공통 반려 사유).
- **Age ratings**: IARC 설문 **필수**(사진 유틸이라 2분).
- **Packages**: `RawBlow.msix` 업로드 → 매니페스트 Identity가 예약 제품과 **일치해야 통과**(불일치 시 여기서 반려).
- **Store listings**: **Description(≤10,000자) + 스크린샷 최소 1장**(권장 4장, PNG, Desktop ≥1366×768). 300×300 앱 타일 아이콘 권장.
- **Submission options**: `runFullTrust` **정당화 한 줄**("Packaged classic Win32 desktop app; runs as mediumIL full-trust process.").
- → **Submit for certification** → 보통 **1~3 영업일**(멀웨어 스캔·크래시·정책·restricted-cap 검토). 통과 시 스토어가 재서명·게시.

### 자주 겪는 반려 + 예방
- **개인정보 처리방침 누락**(가장 흔함) → 전송하는 게 있으면 URL 제공, 전부 오프라인이면 그 취지 명시.
- **스크린샷 부족/저품질** → 실제 앱 화면 1366×768+ PNG ≥1장.
- **인증 머신에서 크래시** → 서명한 .msix를 **클린 VM에서 먼저 테스트**(§5), 필요 시 Notes for certification에 설명.
- **runFullTrust 정당화** → 위 한 줄. `broadFileSystemAccess` 추가 금지.

### 배포 채널 주의
스토어(MSIX)를 **Windows 주 배포로** 삼으면 #79 해결. **NSIS setup.exe 병행 시** 그 사용자는 여전히 SAC에 막히므로 → 그 채널엔 load-dynamic 또는 서명 필요.

### 출처(추가)
- Store가 MSIX/AppX만 재서명: Learn `publish/publish-your-app/msix/app-package-requirements`(2026-07-17)
- 수동 매니페스트/makeappx: Learn `msix/desktop/desktop-to-uwp-manual-conversion`, `msix/package/create-app-package-with-makeappx-tool`
- 서명·사이드로드: Learn `msix/package/sign-msix-package-guide`(2026-04)
- 이름 예약·제출·리스팅: Learn `apps/publish/publish-your-app/msix/{reserve-your-apps-name,create-app-submission,add-and-edit-store-listing-info}`
- Product identity: Learn `apps/publish/view-app-identity-details`
- capabilities(runFullTrust): Learn `uwp/packaging/app-capability-declarations`
- 인증 FAQ/기간·Store Policies: Learn `apps/publish/faq/get-your-app-certified`, `apps/publish/store-policies`

## 현재 상태(2026-07-20)
- v0.5.8 준비 커밋까지 푸시됨(main). 이슈 후속 5건 완료: #80 AF, #78 Undo, #75 디코드 회복, #76 토스트, #74 설정 제목.
- **남은 릴리즈 게이트: 이 #79.** 이 문서대로 Windows에서 수정·검증 후 돌아오면, macOS 쪽에서 새로 빌드해 **macOS 릴리즈**를 진행한다(사용자 계획).

## 출처
- NTSTATUS `0xC0E90002`: ReactOS `sdk/include/psdk/ntstatus.h`, x64dbg `ntstatusdb.txt`, MS-ERREF(Microsoft Learn).
- Smart App Control 차단 메커니즘: n4r1b "Smart App Control Internals" (part 2), Microsoft Learn "Smart App Control overview", geode-sdk `ModImpl.cpp`(동일 상태를 SAC로 매핑).
- ort load-dynamic / `ORT_DYLIB_PATH`: ort.pyke.io "Linking" 문서.
- ORT 런타임 의존(dxil/dxcompiler/dxcore, VC++ set): microsoft/onnxruntime issues #25495, #24771, discussion #27405; onnxruntime.ai 의존성 문서.
