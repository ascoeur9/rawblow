# RawBlow — 원커맨드 릴리즈: GitHub(빌드+릴리즈) + MS 스토어(제출) 한 번에
#
# 전제:
#   - Cargo.toml 의 version 이 이번 릴리즈 버전 (예: 0.5.9). 미리 버전 범프해 두세요.
#   - scripts\release-notes\v<버전>.md 작성 (GitHub 본문 + 스토어 ko/ja/en). 템플릿: v0.5.9.md
#   - 스토어 제출을 하려면 store-credentials(env) 세팅 (submit-msix-store.ps1 헤더 참조).
#   - gh CLI 인증됨 (gh auth status).
#
# 하는 일 (순서):
#   1) [GitHub] build-release-windows.ps1 → dist\RawBlow-Setup-v<버전>.exe (NSIS, 무서명)
#   2) [GitHub] annotated 태그 v<버전> 생성·푸시(없으면) → gh release create + 에셋 업로드
#              (dist 에 macOS zip 이 있으면 함께 첨부)
#   3) [Store]  submit-msix-store.ps1 → MSIX 빌드 + 제출 API 커밋
#
# 사용:
#   .\scripts\release-all.ps1                 # 전체(깃헙 릴리즈 + 스토어 제출)
#   .\scripts\release-all.ps1 -DryRun         # 빌드만, 태그 푸시/릴리즈 생성/스토어 커밋은 전부 생략
#   .\scripts\release-all.ps1 -SkipStore      # 깃헙만
#   .\scripts\release-all.ps1 -SkipGitHub     # 스토어만
#   .\scripts\release-all.ps1 -Draft          # GitHub 릴리즈를 draft 로 생성(수동 게시)

[CmdletBinding()]
param(
    [switch]$SkipGitHub,
    [switch]$SkipStore,
    [switch]$Draft,
    [switch]$DryRun
)

$ErrorActionPreference = "Stop"
$RepoRoot = Split-Path -Parent $PSScriptRoot
$scratch  = $PSScriptRoot
. (Join-Path $PSScriptRoot "_release-notes.ps1")
Push-Location $RepoRoot
try {
    # ── 버전 + 노트 ─────────────────────────────────────────────────────
    $m = Select-String -Path (Join-Path $RepoRoot "Cargo.toml") -Pattern '^version\s*=\s*"([^"]+)"' | Select-Object -First 1
    if (-not $m) { throw "Cargo.toml 에서 version 을 찾지 못함" }
    $ver = $m.Matches.Groups[1].Value
    $tag = "v$ver"
    $notesPath = Join-Path $PSScriptRoot ("release-notes\v{0}.md" -f $ver)
    $notes = Get-ReleaseNotes -Path $notesPath      # 없으면 즉시 throw — 릴리즈 전에 노트부터 강제
    Write-Host "=== RawBlow 릴리즈 $tag ===" -ForegroundColor Cyan
    if ($DryRun) { Write-Host "(DryRun — 게시/푸시/커밋 없음)" -ForegroundColor Yellow }

    # 태그 중복 방지: 이미 릴리즈된 태그면 중단(버전 범프 잊음 방지).
    if (-not $SkipGitHub) {
        $exists = git tag --list $tag
        if ($exists -and -not $DryRun) {
            throw "태그 $tag 이 이미 존재합니다 — 버전을 올렸는지 확인하세요(Cargo.toml). 재릴리즈면 태그를 먼저 지우세요."
        }
    }

    # ── [1] GitHub: 빌드 + 릴리즈 ───────────────────────────────────────
    if (-not $SkipGitHub) {
        Write-Host "[GitHub 1/2] Windows 릴리즈 빌드(NSIS)" -ForegroundColor Cyan
        & (Join-Path $PSScriptRoot "build-release-windows.ps1")
        if ($LASTEXITCODE -ne 0) { throw "build-release-windows.ps1 실패" }
        $setup = Join-Path $RepoRoot ("dist\RawBlow-Setup-v{0}.exe" -f $ver)
        if (-not (Test-Path $setup)) { throw "인스톨러 산출물 없음: $setup" }

        # 첨부 에셋: Windows 인스톨러 + (있으면) macOS zip.
        $assets = @($setup)
        $mac = Join-Path $RepoRoot ("dist\RawBlow-v{0}-macos-arm64.zip" -f $ver)
        if (Test-Path $mac) { $assets += $mac } else { Write-Warning "macOS zip 없음(dist\RawBlow-v$ver-macos-arm64.zip) — Windows 에셋만 올립니다. Mac 빌드는 별도 첨부하세요." }

        $body = Build-GitHubBody -Version $ver -Notes $notes
        $bodyFile = Join-Path $scratch ("_ghbody-v{0}.md" -f $ver)
        [System.IO.File]::WriteAllText($bodyFile, $body, (New-Object System.Text.UTF8Encoding($false)))

        Write-Host "[GitHub 2/2] 태그·릴리즈 생성 + 에셋 업로드" -ForegroundColor Cyan
        if ($DryRun) {
            Write-Host "  [DryRun] 태그 $tag 푸시 생략 / gh release create 생략" -ForegroundColor Yellow
            Write-Host "  [DryRun] 본문 미리보기 → $bodyFile" -ForegroundColor Gray
            Write-Host "  [DryRun] 에셋: $($assets -join ', ')" -ForegroundColor Gray
        } else {
            # 기존 관례대로 annotated 태그 생성·푸시.
            git tag -a $tag -m "release: $tag"
            git push origin $tag
            $ghArgs = @('release','create',$tag,'--title',"RawBlow $tag",'--notes-file',$bodyFile)
            if ($Draft) { $ghArgs += '--draft' }
            $ghArgs += $assets
            & gh @ghArgs
            if ($LASTEXITCODE -ne 0) { throw "gh release create 실패" }
            Write-Host "✅ GitHub 릴리즈 게시: https://github.com/ascoeur9/rawblow/releases/tag/$tag" -ForegroundColor Green
        }
    }

    # ── [2] MS 스토어: MSIX 빌드 + 제출 ────────────────────────────────
    if (-not $SkipStore) {
        Write-Host "[Store] MSIX 빌드 + 제출" -ForegroundColor Cyan
        $storeArgs = @()
        if ($DryRun) { $storeArgs += '-DryRun' }
        & (Join-Path $PSScriptRoot "submit-msix-store.ps1") @storeArgs
        if ($LASTEXITCODE -ne 0) { throw "submit-msix-store.ps1 실패" }
    }

    Write-Host "=== 완료 ($tag) ===" -ForegroundColor Green
}
finally {
    Pop-Location
}
