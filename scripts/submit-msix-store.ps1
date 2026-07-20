# RawBlow — MS 스토어 제출 (MSIX 빌드 → 제출 API 업로드 → 커밋)
#
# 하는 일:
#   1) build-msix-windows.ps1 로 스토어 제출용 무서명 MSIX 빌드(Partner Center Identity 적용)
#   2) release-notes\v<버전>.md 에서 언어별 "새로운 기능" 읽기
#   3) Microsoft Store 제출 API(v1.0):
#        토큰 → 앱 조회 → (대기중 제출 삭제) → 새 제출 생성(직전본 복제)
#        → 패키지 교체 + 언어별 releaseNotes + 인증 메모 → zip 업로드 → PUT → commit
#   4) 인증(certification) 상태 잠깐 폴링 후 Partner Center URL 안내
#      (커밋 후엔 MS 인증 통과 시 자동 게시 — 직전 제출의 게시 모드를 그대로 승계)
#
# 필요한 값 (환경변수 권장 — 리포에 커밋 금지):
#   STORE_APP_ID        스토어 앱 ID (Partner Center, 12자리 예: 9NXXXXXXXXXX)
#   STORE_TENANT_ID     Azure AD 테넌트 ID
#   STORE_CLIENT_ID     Azure AD 앱(클라이언트) ID
#   STORE_CLIENT_SECRET Azure AD 앱 시크릿
#   STORE_IDENTITY_NAME 매니페스트 Identity Name  (Partner Center 앱 ID 페이지)
#   STORE_PUBLISHER     매니페스트 Publisher       (예: CN=ABCD1234-...-배정GUID)
#   STORE_PUBLISHER_DISPLAY  게시자 표시명 (예: Hare)
#
#   설정 예 (PowerShell):  scripts\store-credentials.example.ps1 복사→실값 채우고 dot-source
#
# 사용:
#   .\scripts\submit-msix-store.ps1                 # 빌드+제출+커밋
#   .\scripts\submit-msix-store.ps1 -DryRun         # 빌드+노트/자격증명 검증만(제출 생성·업로드·커밋 안 함)
#   .\scripts\submit-msix-store.ps1 -NoCommit       # 제출 생성·업로드까지 하되 커밋은 보류(Partner Center에서 검토 후 수동 커밋)
#   .\scripts\submit-msix-store.ps1 -SkipBuild      # 기존 dist\RawBlow-v<버전>.msix 재사용

[CmdletBinding()]
param(
    [string]$AppId               = $env:STORE_APP_ID,
    [string]$TenantId            = $env:STORE_TENANT_ID,
    [string]$ClientId            = $env:STORE_CLIENT_ID,
    [string]$ClientSecret        = $env:STORE_CLIENT_SECRET,
    [string]$IdentityName        = $env:STORE_IDENTITY_NAME,
    [string]$Publisher           = $env:STORE_PUBLISHER,
    [string]$PublisherDisplay    = $(if ($env:STORE_PUBLISHER_DISPLAY) { $env:STORE_PUBLISHER_DISPLAY } else { 'Hare' }),
    [string]$NotesPath,                                   # 기본: scripts\release-notes\v<버전>.md
    [string]$NotesForCertification = '이 앱은 로컬 RAW 사진 선별 도구입니다. 로그인/계정이 없으며 모든 처리는 오프라인에서 이뤄집니다. 원본 파일은 절대 삭제되지 않습니다.',
    [switch]$SkipBuild,
    [switch]$NoCommit,
    [switch]$DryRun
)

$ErrorActionPreference = "Stop"
$RepoRoot = Split-Path -Parent $PSScriptRoot
. (Join-Path $PSScriptRoot "_release-notes.ps1")
Push-Location $RepoRoot
try {
    # ── 자격증명 확인 ────────────────────────────────────────────────────
    $missing = @()
    foreach ($p in @(
        @{n='STORE_APP_ID';v=$AppId}, @{n='STORE_TENANT_ID';v=$TenantId},
        @{n='STORE_CLIENT_ID';v=$ClientId}, @{n='STORE_CLIENT_SECRET';v=$ClientSecret},
        @{n='STORE_IDENTITY_NAME';v=$IdentityName}, @{n='STORE_PUBLISHER';v=$Publisher})) {
        if (-not $p.v) { $missing += $p.n }
    }
    if ($missing) {
        throw "필수 값 누락: $($missing -join ', ')`n  → scripts\store-credentials.example.ps1 를 복사해 실값을 채우고 dot-source 하세요."
    }

    # ── 버전 ─────────────────────────────────────────────────────────────
    $m = Select-String -Path (Join-Path $RepoRoot "Cargo.toml") -Pattern '^version\s*=\s*"([^"]+)"' | Select-Object -First 1
    if (-not $m) { throw "Cargo.toml 에서 version 을 찾지 못함" }
    $ver = $m.Matches.Groups[1].Value
    if (-not $NotesPath) { $NotesPath = Join-Path $PSScriptRoot ("release-notes\v{0}.md" -f $ver) }
    Write-Host "[스토어] RawBlow v$ver 제출 준비" -ForegroundColor Cyan

    # ── 릴리즈 노트(언어별) ──────────────────────────────────────────────
    $notes = Get-ReleaseNotes -Path $NotesPath
    if ($notes.Store.Count -eq 0) { throw "노트 파일에 <!-- @store <lang> --> 섹션이 없습니다: $NotesPath" }
    foreach ($lang in $notes.Store.Keys) {
        $len = $notes.Store[$lang].Length
        Write-Host ("       새로운 기능 [{0}] {1}자" -f $lang, $len) -ForegroundColor Gray
        if ($len -gt 1500) { Write-Warning "  [$lang] 릴리즈 노트가 1500자를 초과합니다(스토어 한도). 잘릴 수 있습니다." }
    }

    # ── MSIX 빌드 ────────────────────────────────────────────────────────
    if ($SkipBuild) {
        Write-Host "[1] MSIX 빌드 생략 (-SkipBuild)" -ForegroundColor Yellow
    } else {
        Write-Host "[1] MSIX 빌드 (Identity: $IdentityName / $Publisher)" -ForegroundColor Cyan
        & (Join-Path $PSScriptRoot "build-msix-windows.ps1") `
            -IdentityName $IdentityName -Publisher $Publisher -PublisherDisplay $PublisherDisplay
        if ($LASTEXITCODE -ne 0) { throw "build-msix-windows.ps1 실패 (exit $LASTEXITCODE)" }
    }
    $msix = Join-Path $RepoRoot ("dist\RawBlow-v{0}.msix" -f $ver)
    if (-not (Test-Path $msix)) { throw "MSIX 산출물 없음: $msix (무서명 스토어용). -SkipBuild 로 재사용하려면 먼저 빌드하세요." }
    $msixLeaf = Split-Path $msix -Leaf

    # ── 제출 API: 토큰 ──────────────────────────────────────────────────
    Write-Host "[2] 인증 토큰 취득" -ForegroundColor Cyan
    $tokenResp = Invoke-RestMethod -Method Post -Uri "https://login.microsoftonline.com/$TenantId/oauth2/token" `
        -Body @{
            grant_type    = "client_credentials"
            client_id     = $ClientId
            client_secret = $ClientSecret
            resource      = "https://manage.devcenter.microsoft.com"
        }
    $auth = @{ Authorization = "Bearer $($tokenResp.access_token)" }
    $base = "https://manage.devcenter.microsoft.com/v1.0/my/applications/$AppId"

    # ── 앱 조회 + 대기중 제출 정리 ─────────────────────────────────────
    Write-Host "[3] 앱 조회" -ForegroundColor Cyan
    $app = Invoke-RestMethod -Method Get -Uri $base -Headers $auth
    if ($app.pendingApplicationSubmission) {
        Write-Warning "  대기중 제출(pending)이 이미 있습니다: $($app.pendingApplicationSubmission.id)"
        if ($DryRun) {
            Write-Host "  [DryRun] 대기중 제출을 삭제하지 않습니다." -ForegroundColor Yellow
        } else {
            Write-Host "  대기중 제출 삭제 후 새로 생성합니다." -ForegroundColor Yellow
            Invoke-RestMethod -Method Delete -Headers $auth `
                -Uri "$base/submissions/$($app.pendingApplicationSubmission.id)" | Out-Null
        }
    }

    if ($DryRun) {
        Write-Host "✅ [DryRun] 자격증명·앱 조회·노트 검증 통과. 실제 제출은 생성하지 않았습니다." -ForegroundColor Green
        Write-Host "   제출 대상 언어: $($notes.Store.Keys -join ', ')" -ForegroundColor Gray
        Write-Host "   업로드 예정 패키지: $msixLeaf" -ForegroundColor Gray
        return
    }

    # ── 새 제출 생성 (직전 게시본 복제) ─────────────────────────────────
    Write-Host "[4] 새 제출 생성 (직전본 복제)" -ForegroundColor Cyan
    $sub = Invoke-RestMethod -Method Post -Headers $auth -Uri "$base/submissions"
    $subId = $sub.id

    # 패키지: 복제된 기존 패키지는 삭제 표시, 새 MSIX 는 업로드 대기로 추가.
    $keepOld = @($sub.applicationPackages | ForEach-Object { $_.fileStatus = 'PendingDelete'; $_ })
    $newPkg  = [pscustomobject]@{ fileName = $msixLeaf; fileStatus = 'PendingUpload' }
    $sub.applicationPackages = @($keepOld + $newPkg)

    # 언어별 "새로운 기능" 주입 (리스팅에 이미 존재하는 언어만 — 없으면 경고).
    foreach ($lang in $notes.Store.Keys) {
        $listing = $sub.listings.PSObject.Properties |
            Where-Object { $_.Name -ieq $lang } | Select-Object -First 1
        if (-not $listing) {
            Write-Warning "  리스팅에 언어 '$lang' 이 없습니다(Partner Center에서 먼저 추가 필요). 이 언어는 건너뜁니다."
            continue
        }
        $listing.Value.baseListing.releaseNotes = $notes.Store[$lang]
    }
    if ($NotesForCertification) { $sub.notesForCertification = $NotesForCertification }

    # ── zip 만들고 업로드 ───────────────────────────────────────────────
    Write-Host "[5] 패키지 zip 업로드" -ForegroundColor Cyan
    $zip = Join-Path $RepoRoot ("dist\RawBlow-v{0}-store-upload.zip" -f $ver)
    if (Test-Path $zip) { Remove-Item -Force $zip }
    Compress-Archive -Path $msix -DestinationPath $zip -Force   # zip 내 엔트리명 = $msixLeaf 와 일치
    $uploadUrl = $sub.fileUploadUrl.Replace('+', '%2B')          # SAS 의 '+' 보존(알려진 처리)
    $bytes = [System.IO.File]::ReadAllBytes($zip)
    Invoke-RestMethod -Method Put -Uri $uploadUrl `
        -Headers @{ 'x-ms-blob-type' = 'BlockBlob' } -Body $bytes -ContentType 'application/octet-stream' | Out-Null
    Write-Host ("       업로드 완료: {0} ({1} MB)" -f (Split-Path $zip -Leaf), [math]::Round((Get-Item $zip).Length/1MB,1)) -ForegroundColor Gray

    # ── 수정된 제출 PUT ─────────────────────────────────────────────────
    Write-Host "[6] 제출 갱신(PUT)" -ForegroundColor Cyan
    $json = $sub | ConvertTo-Json -Depth 25
    Invoke-RestMethod -Method Put -Headers $auth -Uri "$base/submissions/$subId" `
        -Body $json -ContentType 'application/json' | Out-Null

    if ($NoCommit) {
        Write-Host "✅ 제출 생성·업로드 완료(커밋 보류, -NoCommit)." -ForegroundColor Green
        Write-Host "   Partner Center에서 검토 후 커밋하거나, 이 스크립트를 -NoCommit 없이 다시 실행하세요." -ForegroundColor Gray
        Write-Host "   submissionId = $subId" -ForegroundColor Gray
        return
    }

    # ── 커밋 ─────────────────────────────────────────────────────────────
    Write-Host "[7] 커밋(commit) — 인증 큐 진입" -ForegroundColor Cyan
    Invoke-RestMethod -Method Post -Headers $auth -Uri "$base/submissions/$subId/commit" | Out-Null

    # 상태 잠깐 폴링 (실패면 즉시 표면화).
    for ($i = 0; $i -lt 6; $i++) {
        Start-Sleep -Seconds 10
        $st = Invoke-RestMethod -Method Get -Headers $auth -Uri "$base/submissions/$subId/status"
        Write-Host ("       status: {0}" -f $st.status) -ForegroundColor Gray
        if ($st.status -eq 'CommitFailed') {
            $st.statusDetails.errors | ForEach-Object { Write-Host ("   ❌ {0}" -f $_.details) -ForegroundColor Red }
            throw "커밋 실패 — 위 오류 참조."
        }
        if ($st.status -notin @('CommitStarted')) { break }   # PreProcessing 이후로 넘어가면 큐 진입 성공
    }
    Write-Host "✅ 제출 커밋됨 — MS 인증 진행 중. 통과하면 자동 게시됩니다(직전 게시 모드 승계)." -ForegroundColor Green
    Write-Host "   진행 확인: https://partner.microsoft.com/dashboard  → RawBlow → 제출" -ForegroundColor Gray
}
finally {
    Pop-Location
}
