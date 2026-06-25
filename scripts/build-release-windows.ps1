# RawBlow — Windows 릴리스 빌드 스크립트
#
# 사용법 (PowerShell, 저장소 루트에서):
#     .\scripts\build-release-windows.ps1
#
# 하는 일:
#   1) RUSTFLAGS 설정 (--remap-path-prefix). crt-static은 쓰지 않는다(아래 "왜" 참조).
#   2) cargo build --release -p rawblow-app  (기본 피처 = ai 포함, ONNX 미적 컬링)
#   3) dist\RawBlow-v<버전>-windows-x86_64\ 에 배포 묶음 구성:
#        RawBlow.exe
#        + ORT/Dawn GPU 런타임 DLL(dxcompiler/dxil/webgpu_dawn) — target\release에서
#        + VC++ 런타임 DLL(vcruntime140/_1, msvcp140/_1) — MSVC 재배포 폴더에서
#   4) dumpbin /dependents 로 묶음 자체완결성 검증 — exe·DLL이 참조하는 non-OS DLL이
#      전부 묶음 안에 있어야 통과. 하나라도 빠지면 exit 1 (이슈 #10 재발 방지).
#   5) NSIS(makensis)로 단일 setup.exe 인스톨러 빌드 → dist\RawBlow-Setup-v<버전>.exe.
#      (NSIS 필요: winget install NSIS.NSIS. NSIS는 zlib 라이선스로 상용 무료.)
#
# 왜 crt-static을 못 쓰는가 (중요):
#   ai 피처의 ORT(ONNX Runtime) download-binaries prebuilt는 /MD(동적 CRT)로 빌드돼 있어
#   +crt-static(/MT)과 링크 충돌한다(LNK2019: __imp__wfopen 등). 따라서 동적 CRT로 빌드하고
#   VC++ 런타임 DLL을 묶음에 동봉해 재배포 미설치 PC에서도 실행되게 한다. 또한 GPU EP(webgpu)는
#   Dawn DLL 3종을 exe 옆에 요구하므로 윈도우 ai 빌드는 애초에 단독 exe가 불가능하다.
#   (ai 없이 단독 exe가 필요하면: cargo build --release -p rawblow-app --no-default-features
#    + RUSTFLAGS에 +crt-static 추가. 단 미적 AI는 빠진다.)

[CmdletBinding()]
param(
    [string]$Package = "rawblow-app",
    [string]$BinName = "rawblow",
    [switch]$SkipVerify,
    [switch]$SkipInstaller
)

$ErrorActionPreference = "Stop"
$RepoRoot = Split-Path -Parent $PSScriptRoot
Push-Location $RepoRoot
try {
    Write-Host "[1/5] RUSTFLAGS 설정 (--remap-path-prefix, crt-static 미사용)" -ForegroundColor Cyan
    $home_dir = if ($env:USERPROFILE) { $env:USERPROFILE } else { $HOME }
    $env:RUSTFLAGS = "--remap-path-prefix=$home_dir=~"
    Write-Host "       RUSTFLAGS = $env:RUSTFLAGS"

    Write-Host "[2/5] cargo build --release -p $Package" -ForegroundColor Cyan
    & cargo build --release -p $Package
    if ($LASTEXITCODE -ne 0) { throw "cargo build 실패 (exit $LASTEXITCODE)" }

    $rel = Join-Path $RepoRoot "target\release"
    $exe = Join-Path $rel "$BinName.exe"
    if (-not (Test-Path $exe)) { throw "산출물을 찾을 수 없음: $exe" }

    # 버전(워크스페이스 Cargo.toml).
    $m = Select-String -Path (Join-Path $RepoRoot "Cargo.toml") -Pattern '^version\s*=\s*"([^"]+)"' | Select-Object -First 1
    $ver = if ($m) { $m.Matches.Groups[1].Value } else { (Get-Item $exe).VersionInfo.ProductVersion }

    Write-Host "[3/5] 배포 묶음 구성 (v$ver)" -ForegroundColor Cyan
    $stage = Join-Path $RepoRoot ("dist\RawBlow-v{0}-windows-x86_64" -f $ver)
    if (Test-Path $stage) { Remove-Item -Recurse -Force $stage }
    New-Item -ItemType Directory -Force $stage | Out-Null

    Copy-Item $exe (Join-Path $stage "RawBlow.exe")
    # target\release의 런타임 DLL(ORT/Dawn) 전부 — cargo가 빌드 시 배치한 것.
    Get-ChildItem (Join-Path $rel "*.dll") -ErrorAction SilentlyContinue | ForEach-Object { Copy-Item $_.FullName $stage }

    # VC++ 런타임 DLL — MSVC 재배포 폴더에서 최신 버전을 찾아 동봉.
    $crtRoot = Get-ChildItem -Path @("${env:ProgramFiles}\Microsoft Visual Studio","${env:ProgramFiles(x86)}\Microsoft Visual Studio") `
        -Recurse -Directory -Filter "Microsoft.VC*.CRT" -ErrorAction SilentlyContinue |
        Where-Object { $_.FullName -match "\\x64\\" -and $_.FullName -notmatch "onecore" } |
        Sort-Object FullName -Descending | Select-Object -First 1
    if (-not $crtRoot) { throw "VC++ 재배포(Microsoft.VC*.CRT\x64) 폴더를 찾지 못함 — MSVC Build Tools 필요" }
    foreach ($d in @("vcruntime140.dll","vcruntime140_1.dll","msvcp140.dll","msvcp140_1.dll")) {
        $src = Join-Path $crtRoot.FullName $d
        if (Test-Path $src) { Copy-Item $src $stage } else { Write-Warning "VC++ DLL 없음: $d" }
    }
    Get-ChildItem $stage | Select-Object Name, @{n='MB';e={[math]::Round($_.Length/1MB,2)}} | Format-Table -AutoSize | Out-Host

    if ($SkipVerify) {
        Write-Host "[4/5] 검증 생략 (-SkipVerify)" -ForegroundColor Yellow
    } else {
        Write-Host "[4/5] dumpbin 자체완결성 검증" -ForegroundColor Cyan
        $db = Get-ChildItem "${env:ProgramFiles(x86)}\Microsoft Visual Studio\*\*\VC\Tools\MSVC\*\bin\Hostx64\x64\dumpbin.exe","${env:ProgramFiles}\Microsoft Visual Studio\*\*\VC\Tools\MSVC\*\bin\Hostx64\x64\dumpbin.exe" -ErrorAction SilentlyContinue |
            Sort-Object FullName -Descending | Select-Object -First 1
        if (-not $db) {
            Write-Warning "dumpbin.exe를 찾지 못해 검증 생략. 묶음은 생성됨."
        } else {
            # OS 내장으로 간주: api-ms-win-* + 알려진 시스템 DLL.
            $osDlls = @("kernel32.dll","ntdll.dll","ole32.dll","oleaut32.dll","advapi32.dll","user32.dll",
                "imm32.dll","shell32.dll","ws2_32.dll","shlwapi.dll","opengl32.dll","gdi32.dll","dwmapi.dll",
                "uxtheme.dll","dbghelp.dll","setupapi.dll","dxgi.dll","bcrypt.dll","bcryptprimitives.dll",
                "crypt32.dll","secur32.dll","winmm.dll","version.dll")
            $present = (Get-ChildItem "$stage\*.dll").Name | ForEach-Object { $_.ToLower() }
            $missing = @()
            foreach ($f in (Get-ChildItem "$stage\*.exe","$stage\*.dll")) {
                $deps = & $db.FullName /dependents $f.FullName 2>$null |
                    Select-String -Pattern "^\s+\S+\.dll\s*$" | ForEach-Object { $_.Line.Trim().ToLower() }
                foreach ($d in $deps) {
                    if ($d -like "api-ms-win-*") { continue }
                    if ($osDlls -contains $d) { continue }
                    if ($present -contains $d) { continue }
                    $missing += ("{0} -> {1}" -f $f.Name, $d)
                }
            }
            if ($missing) {
                Write-Host "❌ 누락된 non-OS DLL (재배포 미설치 PC에서 실행 불가):" -ForegroundColor Red
                $missing | Select-Object -Unique | ForEach-Object { Write-Host "   $_" -ForegroundColor Red }
                exit 1
            }
            Write-Host "✅ 모든 non-OS 의존 DLL이 묶음에 포함됨 — 배포 OK" -ForegroundColor Green
        }
    }

    if ($SkipInstaller) {
        Write-Host "[5/5] 인스톨러 생략 (-SkipInstaller). 스테이징: $stage" -ForegroundColor Yellow
        return
    }
    Write-Host "[5/5] NSIS 인스톨러 빌드" -ForegroundColor Cyan
    $makensis = @(
        "${env:ProgramFiles(x86)}\NSIS\makensis.exe",
        "${env:ProgramFiles}\NSIS\makensis.exe",
        "$env:LOCALAPPDATA\Programs\NSIS\makensis.exe"
    ) | Where-Object { Test-Path $_ } | Select-Object -First 1
    if (-not $makensis) { throw "makensis.exe 없음 — NSIS 설치 필요: winget install NSIS.NSIS" }
    $nsi = Join-Path $PSScriptRoot "rawblow.nsi"
    $distDir = Join-Path $RepoRoot "dist"
    & $makensis "/DAPPVERSION=$ver" "/DSTAGEDIR=$stage" "/DOUTDIR=$distDir" $nsi
    if ($LASTEXITCODE -ne 0) { throw "makensis 실패 (exit $LASTEXITCODE)" }
    $setup = Join-Path $distDir ("RawBlow-Setup-v{0}.exe" -f $ver)
    Write-Host ("✅ {0} ({1} MB)" -f $setup, [math]::Round((Get-Item $setup).Length/1MB,1)) -ForegroundColor Green
}
finally {
    Pop-Location
}
