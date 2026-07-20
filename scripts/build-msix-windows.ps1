# RawBlow — Windows MSIX(Microsoft Store) 패키지 빌드 스크립트
#
# 사용법 (PowerShell, 저장소 루트에서):
#   # 스토어 제출용 (Identity 를 Partner Center 배정값으로 — 개인 개발자 계정 기준):
#   .\scripts\build-msix-windows.ps1 -IdentityName "1234Hare.RawBlow" `
#       -Publisher "CN=ABCD1234-...-배정된GUID" -PublisherDisplay "Hare"
#
#   # 로컬 설치 테스트용 (자체 서명까지 한 번에):
#   .\scripts\build-msix-windows.ps1 -SignForLocalTest
#
# 하는 일:
#   1) cargo build --release -p rawblow-app  (기본 피처 = ai 포함)
#   2) 스테이징(dist\msix\RawBlow-v<버전>\)에 RawBlow.exe + 런타임 DLL(ORT/Dawn + VC++) 배치
#   3) gen_msix_assets 예제로 Assets\ 로고 PNG 5종 생성
#   4) scripts\AppxManifest.xml 토큰 치환 → 스테이징 루트에 AppxManifest.xml 작성
#   5) makeappx pack → dist\RawBlow-v<버전>.msix
#   6) (-SignForLocalTest) 자체 서명 인증서 생성·서명·.cer 내보내기 + 설치 안내 출력
#
# 왜 MSIX/스토어인가 (#79): 스토어가 배포 시 패키지 전체를 Microsoft 인증서로 서명하고,
#   MSIX 블록맵이 내부 모든 파일(무서명 webgpu_dawn.dll 포함)의 무결성을 그 서명으로 보증한다.
#   따라서 Smart App Control 이 개별 무서명 DLL 을 따로 검사·차단하지 않아 "잘못된 이미지"(0xc0e90002)
#   가 발생하지 않는다. (자체 서명 로컬 빌드는 패키징 정상 여부·실행만 검증하며, SAC 강제 PC 통과는
#   스토어 배포에서만 보장된다 — 자체 서명 루트는 SAC 가 신뢰하지 않기 때문.)

[CmdletBinding()]
param(
    [string]$Package = "rawblow-app",
    [string]$BinName = "rawblow",
    # 개인 프로젝트: 기존 NSIS 설치 프로그램과 동일한 개인 명의(Hare)를 기본값으로.
    # 스토어 제출 시엔 개인 Partner Center 계정이 배정하는 값으로 파라미터 오버라이드.
    [string]$IdentityName = "Hare.RawBlow",
    [string]$Publisher = "CN=Hare",
    [string]$PublisherDisplay = "Hare",
    [switch]$SignForLocalTest,
    [switch]$SkipBuild
)

$ErrorActionPreference = "Stop"
$RepoRoot = Split-Path -Parent $PSScriptRoot
Push-Location $RepoRoot
try {
    # ── SDK 도구 확인 (makeappx / signtool) ─────────────────────────────
    function Find-SdkTool([string]$name) {
        $roots = @("${env:ProgramFiles(x86)}\Windows Kits\10\bin", "${env:ProgramFiles}\Windows Kits\10\bin")
        foreach ($r in $roots) {
            if (Test-Path $r) {
                $hit = Get-ChildItem $r -Recurse -Filter $name -ErrorAction SilentlyContinue |
                    Where-Object { $_.FullName -match "\\x64\\" } |
                    Sort-Object FullName -Descending | Select-Object -First 1
                if ($hit) { return $hit.FullName }
            }
        }
        return $null
    }
    $makeappx = Find-SdkTool "makeappx.exe"
    if (-not $makeappx) { throw "makeappx.exe 없음 — Windows 10/11 SDK 설치 필요 (winget install Microsoft.WindowsSDK)" }

    # ── 버전 (워크스페이스 Cargo.toml) → 4-part (스토어는 리비전 0 요구) ──
    $m = Select-String -Path (Join-Path $RepoRoot "Cargo.toml") -Pattern '^version\s*=\s*"([^"]+)"' | Select-Object -First 1
    if (-not $m) { throw "Cargo.toml 에서 version 을 찾지 못함" }
    $ver = $m.Matches.Groups[1].Value
    $parts = $ver.Split('.')
    while ($parts.Count -lt 3) { $parts += "0" }
    $ver4 = "{0}.{1}.{2}.0" -f $parts[0], $parts[1], $parts[2]
    Write-Host "[MSIX] RawBlow v$ver  →  패키지 버전 $ver4" -ForegroundColor Cyan

    # ── 1) 빌드 ──────────────────────────────────────────────────────────
    if ($SkipBuild) {
        Write-Host "[1/6] cargo build 생략 (-SkipBuild)" -ForegroundColor Yellow
    } else {
        Write-Host "[1/6] cargo build --release -p $Package" -ForegroundColor Cyan
        $home_dir = if ($env:USERPROFILE) { $env:USERPROFILE } else { $HOME }
        $env:RUSTFLAGS = "--remap-path-prefix=$home_dir=~"
        & cargo build --release -p $Package
        if ($LASTEXITCODE -ne 0) { throw "cargo build 실패 (exit $LASTEXITCODE)" }
    }
    $rel = Join-Path $RepoRoot "target\release"
    $exe = Join-Path $rel "$BinName.exe"
    if (-not (Test-Path $exe)) { throw "산출물을 찾을 수 없음: $exe" }

    # ── 2) 스테이징 (exe + 런타임 DLL) ──────────────────────────────────
    Write-Host "[2/6] 스테이징 구성" -ForegroundColor Cyan
    $stage = Join-Path $RepoRoot ("dist\msix\RawBlow-v{0}" -f $ver)
    if (Test-Path $stage) { Remove-Item -Recurse -Force $stage }
    New-Item -ItemType Directory -Force $stage | Out-Null

    Copy-Item $exe (Join-Path $stage "RawBlow.exe")
    # target\release 의 런타임 DLL(ORT/Dawn) 전부 — Copy-Item 이 reparse point 를 역참조해 실제 내용 복사.
    Get-ChildItem (Join-Path $rel "*.dll") -ErrorAction SilentlyContinue | ForEach-Object { Copy-Item $_.FullName $stage }
    # VC++ 런타임 DLL — MSVC 재배포 폴더에서.
    $crtRoot = Get-ChildItem -Path @("${env:ProgramFiles}\Microsoft Visual Studio","${env:ProgramFiles(x86)}\Microsoft Visual Studio") `
        -Recurse -Directory -Filter "Microsoft.VC*.CRT" -ErrorAction SilentlyContinue |
        Where-Object { $_.FullName -match "\\x64\\" -and $_.FullName -notmatch "onecore" } |
        Sort-Object FullName -Descending | Select-Object -First 1
    if (-not $crtRoot) { throw "VC++ 재배포(Microsoft.VC*.CRT\x64) 폴더를 찾지 못함 — MSVC Build Tools 필요" }
    foreach ($d in @("vcruntime140.dll","vcruntime140_1.dll","msvcp140.dll","msvcp140_1.dll")) {
        $src = Join-Path $crtRoot.FullName $d
        if (Test-Path $src) { Copy-Item $src $stage } else { Write-Warning "VC++ DLL 없음: $d" }
    }

    # ── 3) 로고 자산 생성 ────────────────────────────────────────────────
    Write-Host "[3/6] Assets\ 로고 PNG 생성 (gen_msix_assets)" -ForegroundColor Cyan
    $assets = Join-Path $stage "Assets"
    & cargo run -q -p $Package --example gen_msix_assets -- $assets
    if ($LASTEXITCODE -ne 0) { throw "gen_msix_assets 실패 (exit $LASTEXITCODE)" }

    # ── 4) 매니페스트 토큰 치환 ─────────────────────────────────────────
    Write-Host "[4/6] AppxManifest.xml 작성 (Identity: $IdentityName / $Publisher)" -ForegroundColor Cyan
    $manifestTpl = Join-Path $PSScriptRoot "AppxManifest.xml"
    $xml = Get-Content $manifestTpl -Raw
    $xml = $xml.Replace("@IDENTITY_NAME@", $IdentityName)
    $xml = $xml.Replace("@PUBLISHER@", $Publisher)
    $xml = $xml.Replace("@PUBLISHER_DISPLAY@", $PublisherDisplay)
    $xml = $xml.Replace("@APPVERSION@", $ver4)
    # BOM 없는 UTF-8 로 저장(makeappx 안전).
    $utf8NoBom = New-Object System.Text.UTF8Encoding($false)
    [System.IO.File]::WriteAllText((Join-Path $stage "AppxManifest.xml"), $xml, $utf8NoBom)

    Get-ChildItem $stage | Select-Object Name, @{n='MB';e={[math]::Round($_.Length/1MB,2)}} | Format-Table -AutoSize | Out-Host

    # ── 5) 패키징 ────────────────────────────────────────────────────────
    Write-Host "[5/6] makeappx pack" -ForegroundColor Cyan
    $distDir = Join-Path $RepoRoot "dist"
    # 서명(로컬 테스트) 빌드는 -test 접미사로, 스토어 제출용(무서명)은 깔끔한 이름으로 분리한다.
    $suffix = if ($SignForLocalTest) { "-test" } else { "" }
    $msix = Join-Path $distDir ("RawBlow-v{0}{1}.msix" -f $ver, $suffix)
    & $makeappx pack /o /d $stage /p $msix
    if ($LASTEXITCODE -ne 0) { throw "makeappx pack 실패 (exit $LASTEXITCODE)" }
    Write-Host ("✅ {0} ({1} MB)" -f $msix, [math]::Round((Get-Item $msix).Length/1MB,1)) -ForegroundColor Green

    # ── 6) 로컬 테스트 서명 (선택) ──────────────────────────────────────
    if ($SignForLocalTest) {
        Write-Host "[6/6] 자체 서명 (로컬 설치 테스트용)" -ForegroundColor Cyan
        $signtool = Find-SdkTool "signtool.exe"
        if (-not $signtool) { throw "signtool.exe 없음 — Windows SDK 필요" }
        # 인증서 Subject 는 매니페스트 Publisher 와 정확히 일치해야 설치된다.
        $cert = New-SelfSignedCertificate -Type Custom -Subject $Publisher `
            -KeyUsage DigitalSignature -FriendlyName "RawBlow MSIX test cert" `
            -CertStoreLocation "Cert:\CurrentUser\My" `
            -TextExtension @("2.5.29.37={text}1.3.6.1.5.5.7.3.3", "2.5.29.19={text}")
        & $signtool sign /fd SHA256 /sha1 $cert.Thumbprint $msix
        if ($LASTEXITCODE -ne 0) { throw "signtool sign 실패 (exit $LASTEXITCODE)" }
        $cer = Join-Path $distDir "RawBlow-test.cer"
        Export-Certificate -Cert $cert -FilePath $cer | Out-Null
        Write-Host "✅ 서명 완료. 이 PC에서 설치 테스트하려면 (관리자 PowerShell):" -ForegroundColor Green
        Write-Host "     Import-Certificate -FilePath `"$cer`" -CertStoreLocation Cert:\LocalMachine\TrustedPeople" -ForegroundColor Gray
        Write-Host "     Add-AppxPackage `"$msix`"" -ForegroundColor Gray
        Write-Host "   (스토어 제출 시엔 서명 불필요 — Partner Center Identity 로 다시 빌드해 .msix 를 그대로 업로드)" -ForegroundColor Gray
    } else {
        Write-Host "[6/6] 서명 생략. 스토어 제출: Partner Center Identity 로 빌드한 이 .msix 를 업로드하면 MS 가 서명." -ForegroundColor Yellow
        Write-Host "   로컬 설치 테스트가 필요하면 -SignForLocalTest 로 다시 실행." -ForegroundColor Gray
    }
}
finally {
    Pop-Location
}
