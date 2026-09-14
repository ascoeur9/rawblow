# 설치

Windows는 Microsoft Store에서 받는 것이 기본입니다. Store를 쓸 수 없을 때만 GitHub Releases의 설치 파일을 씁니다. macOS는 Apple Silicon용 zip만 사전 빌드로 올라갑니다.

## Windows (권장: Microsoft Store)

[Microsoft Store에서 RawBlow 받기](https://apps.microsoft.com/store/detail/9PC2FKGPQPD1). Store에서 설치하면 업데이트가 따라오고, SmartScreen 경고 없이 실행됩니다.

### Store를 쓰지 못할 때

[GitHub Releases](https://github.com/ascoeur9/rawblow/releases/latest)에서 `RawBlow-Setup-vX.Y.Z.exe`를 받습니다. 필요한 런타임이 들어 있어 따로 설치할 것은 없습니다.

이 설치 파일은 코드 서명이 없습니다. 첫 실행 때 Windows SmartScreen이 **Windows의 PC 보호**를 띄우면 **추가 정보**를 누른 뒤 **실행**을 누릅니다.

<!-- SCREENSHOT-PLACEHOLDER: windows-smartscreen -->

> **캡처 지시 (Windows)**
>
> - **창:** Windows SmartScreen 대화상자. 제목은 보통 `Windows의 PC 보호`입니다. macOS에서는 이 화면이 없으므로 Windows에서만 찍습니다.
> - **준비:** Microsoft Store가 아닌 GitHub의 `RawBlow-Setup-v*.exe`를 받아 실행한 직후. SmartScreen이 막은 상태.
> - **찍을 컨트롤:** 대화상자 전체. **추가 정보**가 보이는 첫 화면과, 펼친 뒤 **실행** 버튼이 보이는 화면을 각각 한 장씩 찍습니다.
> - **크롭:** 대화상자 테두리만 남깁니다. 바탕화면·탐색기·브라우저 탭은 잘라 냅니다. 사용자 계정 이름·다운로드 전체 경로는 보이지 않게 잘라도 됩니다.
> - **넣기:** 이 블록을 PNG로 바꿉니다. 두 장이면 첫 화면 / 펼친 화면 순으로 붙입니다.

## macOS (Apple Silicon)

[GitHub Releases](https://github.com/ascoeur9/rawblow/releases/latest)에서 `RawBlow-vX.Y.Z-macos-arm64.zip`을 받아 압축을 풀면 `RawBlow.app`이 나옵니다. Applications 폴더로 옮긴 뒤 실행합니다.

공증(notarize)을 받지 않아 처음 실행할 때 **확인되지 않은 개발자** 경고가 뜹니다.

1. 경고창에서 **완료**를 누릅니다.
2. **시스템 설정 → 개인정보 보호 및 보안 → 보안**으로 갑니다.
3. 맨 아래의 **그래도 열기**를 누릅니다.

macOS 15 Sequoia부터는 예전처럼 앱을 우클릭한 뒤 **열기**로 우회하는 방법이 동작하지 않습니다. 터미널을 쓴다면 아래 한 줄로 격리 속성을 지울 수 있습니다.

```bash
xattr -dr com.apple.quarantine /Applications/RawBlow.app
```

<!-- SCREENSHOT-PLACEHOLDER: macos-gatekeeper -->

> **캡처 지시 (macOS)**
>
> - **창 1:** Gatekeeper 경고. `"RawBlow.app"은(는) Apple에서 확인하지 않은 개발자가 배포했기 때문에 열 수 없습니다` 류 문구와 **완료** 버튼이 있는 대화상자.
> - **창 2:** **시스템 설정 → 개인정보 보호 및 보안**. 보안 섹션 맨 아래에 RawBlow에 대한 **그래도 열기**가 보이는 상태.
> - **준비:** zip에서 꺼낸 `RawBlow.app`을 처음 실행한 직후. Windows에서는 이 화면이 없으므로 macOS에서만 찍습니다.
> - **찍을 컨트롤:** 창 1은 경고 대화상자 전체. 창 2는 시스템 설정 창에서 보안 블록(앱 이름 + **그래도 열기**)이 보이도록.
> - **크롭:** 창 1은 대화상자만. 창 2는 왼쪽 사이드바의 개인정보 보호 및 보안과 오른쪽 보안 문단이 들어오게 자르되, 계정 사진·Apple ID 메일 주소는 넣지 않습니다.
> - **넣기:** 이 블록을 PNG로 바꿉니다. 경고창 / 시스템 설정 순으로 붙입니다.

## Linux

Linux는 코드상 동작하지만 사전 빌드본은 없습니다. Rust 1.80 이상, C 링커, Vulkan 런타임이 있으면 소스에서 빌드합니다.

```bash
cargo build --release -p rawblow-app
```

실행물은 `target/release/rawblow`입니다. 한글 폰트는 OS 폰트에서 불러옵니다.

## 지원 OS

Windows 11과 macOS(Apple Silicon)로 정식 배포합니다. Intel Mac용 사전 빌드본은 없습니다.

## 다음

설치가 끝나면 [폴더를 열고 고르기](select.md)로 갑니다.
