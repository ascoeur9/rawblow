# RawBlow — Windows 릴리스 빌드 스크립트
#
# 사용법 (PowerShell, 저장소 루트에서):
#     .\scripts\build-release-windows.ps1
#
# 하는 일:
#   1) RUSTFLAGS를 정확히 한 번에 설정 (+crt-static + remap-path-prefix)
#      → 빌드 세션의 잔존 환경변수에 영향받지 않음
#   2) cargo build --release -p rawblow-app
#   3) dumpbin /dependents 자동 검증 — VCRUNTIME140.dll / api-ms-win-crt-*.dll
#      의존이 잡히면 종료 코드 1로 실패 (정적 CRT 누락 = 이슈 #10 재발 방지)
#
# 왜 필요한가:
#   .cargo/config.toml의 `[target.x86_64-pc-windows-msvc] rustflags`는 환경변수
#   `RUSTFLAGS`가 설정된 순간 통째로 덮어써진다(append 아님). 이전 PowerShell
#   세션에서 다른 RUSTFLAGS를 한 번이라도 설정해 두면 거기서 정적 CRT가 빠진 채
#   빌드되어 VC++ 재배포 미설치 PC에서 LoadLibrary error 126이 발생한다.
#   이 스크립트가 그 변동성을 차단한다.

[CmdletBinding()]
param(
    [string]$Package = "rawblow-app",
    [string]$BinName = "rawblow",
    [switch]$SkipVerify
)

$ErrorActionPreference = "Stop"

# 저장소 루트 = 이 스크립트의 상위 디렉토리.
$RepoRoot = Split-Path -Parent $PSScriptRoot
Push-Location $RepoRoot
try {
    Write-Host "[1/3] RUSTFLAGS 설정 (+crt-static, --remap-path-prefix)" -ForegroundColor Cyan
    # USERPROFILE 누락 환경(예: SYSTEM 컨텍스트) 대비 fallback.
    $home_dir = if ($env:USERPROFILE) { $env:USERPROFILE } else { $HOME }
    $env:RUSTFLAGS = "-C target-feature=+crt-static --remap-path-prefix=$home_dir=~"
    Write-Host "       RUSTFLAGS = $env:RUSTFLAGS"

    Write-Host "[2/3] cargo build --release -p $Package" -ForegroundColor Cyan
    & cargo build --release -p $Package
    if ($LASTEXITCODE -ne 0) {
        throw "cargo build 실패 (exit $LASTEXITCODE)"
    }

    $exe = Join-Path $RepoRoot "target\release\$BinName.exe"
    if (-not (Test-Path $exe)) {
        throw "산출물을 찾을 수 없음: $exe"
    }
    Write-Host "       산출물: $exe"

    if ($SkipVerify) {
        Write-Host "[3/3] 검증 생략 (-SkipVerify)" -ForegroundColor Yellow
        return
    }

    Write-Host "[3/3] dumpbin /dependents 검증" -ForegroundColor Cyan
    $dumpbin = Get-Command dumpbin.exe -ErrorAction SilentlyContinue
    if (-not $dumpbin) {
        # MSVC Build Tools 설치 시 자동 추가되는 위치까지는 PATH가 없을 수 있음.
        # vswhere로 위치를 찾는다.
        $vswhere = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe"
        if (Test-Path $vswhere) {
            $vsroot = & $vswhere -latest -products * -find "VC\Tools\MSVC\*\bin\Hostx64\x64\dumpbin.exe" | Select-Object -First 1
            if ($vsroot) { $dumpbin = Get-Command $vsroot }
        }
    }
    if (-not $dumpbin) {
        Write-Warning "dumpbin.exe를 찾지 못해 검증을 건너뜁니다 (MSVC Build Tools 필요). 결과 exe는 만들었습니다."
        return
    }

    $out = & $dumpbin.Source /dependents $exe
    $bad = $out | Select-String -Pattern "vcruntime|api-ms-win-crt" -CaseSensitive:$false
    if ($bad) {
        Write-Host ""
        Write-Host "❌ 정적 CRT 누락 — 다음 DLL 의존이 발견되어 배포 부적합:" -ForegroundColor Red
        $bad | ForEach-Object { Write-Host "   $($_.Line.Trim())" -ForegroundColor Red }
        Write-Host ""
        Write-Host "원인 후보: 셸 세션의 RUSTFLAGS가 다른 값으로 캐시되어 있거나" -ForegroundColor Yellow
        Write-Host "cargo가 다른 .cargo/config로 빌드됐을 가능성. PowerShell을 새로" -ForegroundColor Yellow
        Write-Host "열고 다시 시도하거나 ``Remove-Item Env:RUSTFLAGS`` 후 재실행 권장." -ForegroundColor Yellow
        exit 1
    }
    Write-Host "✅ VCRUNTIME / api-ms-win-crt 의존 없음 — 배포 OK" -ForegroundColor Green
}
finally {
    Pop-Location
}
