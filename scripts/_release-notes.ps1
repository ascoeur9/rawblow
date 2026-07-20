# RawBlow — 릴리즈 노트 파서 (공용, dot-source 전용)
#
# 한 버전의 노트를 파일 하나로 관리한다:  scripts\release-notes\v<버전>.md
#
# 파일 형식 (HTML 주석 마커로 구획 — 마크다운 미리보기엔 안 보임):
#
#   <!-- @github -->            GitHub 릴리즈 본문 전체를 그대로 사용(있으면 최우선)
#   ...본문...
#
#   <!-- @changelog -->         @github 가 없을 때, 이 변경목록을 보일러플레이트로 감싸 GitHub 본문 생성
#   - 항목1
#   - 항목2
#
#   <!-- @store ko-KR -->       MS 스토어 "새로운 기능"(언어별, 각 ≤1500자, 마크다운/링크 없이 평문)
#   - 항목...
#   <!-- @store ja-JP -->
#   - ...
#   <!-- @store en-US -->
#   - ...
#
# Get-ReleaseNotes → [ordered]@{ GitHub=<string>; Changelog=<string>; Store=@{ 'ko-KR'=..; 'ja-JP'=..; 'en-US'=.. } }

function Get-ReleaseNotes {
    [CmdletBinding()]
    param([Parameter(Mandatory)][string]$Path)

    if (-not (Test-Path -LiteralPath $Path)) {
        throw "릴리즈 노트 파일 없음: $Path`n  → scripts\release-notes\v<버전>.md 를 만들어 주세요 (템플릿: scripts\release-notes\v0.5.9.md 참고)."
    }

    $result = [ordered]@{ GitHub = ''; Changelog = ''; Store = @{} }
    $cur = $null                         # 'github' | 'changelog' | 'ko-KR' ...
    $buf = [System.Collections.Generic.List[string]]::new()

    $commit = {
        param($section, $lines)
        if (-not $section) { return }
        $text = ($lines -join "`n").Trim()
        switch -Regex ($section) {
            '^github$'    { $result.GitHub = $text }
            '^changelog$' { $result.Changelog = $text }
            default       { $result.Store[$section] = $text }
        }
    }

    foreach ($line in (Get-Content -LiteralPath $Path -Encoding UTF8)) {
        if ($line -match '^\s*<!--\s*@github\s*-->\s*$') {
            & $commit $cur $buf; $buf.Clear(); $cur = 'github'; continue
        }
        if ($line -match '^\s*<!--\s*@changelog\s*-->\s*$') {
            & $commit $cur $buf; $buf.Clear(); $cur = 'changelog'; continue
        }
        if ($line -match '^\s*<!--\s*@store\s+(\S+)\s*-->\s*$') {
            & $commit $cur $buf; $buf.Clear(); $cur = $Matches[1]; continue
        }
        if ($cur) { $buf.Add($line) }
    }
    & $commit $cur $buf
    return $result
}

# GitHub 릴리즈 본문 생성: @github 가 있으면 그대로, 없으면 @changelog 를 보일러플레이트로 감싼다.
function Build-GitHubBody {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][string]$Version,
        [Parameter(Mandatory)]$Notes    # Get-ReleaseNotes 결과
    )
    if ($Notes.GitHub) { return $Notes.GitHub }
    if (-not $Notes.Changelog) {
        throw "GitHub 본문을 만들 수 없음 — 노트 파일에 <!-- @github --> 또는 <!-- @changelog --> 중 하나가 필요합니다."
    }
    $tpl = @'
## RawBlow v@VER@

> **다운로드** — Windows(x64) `RawBlow-Setup-v@VER@.exe` · macOS(arm64) `RawBlow-v@VER@-macos-arm64.zip`

### ⚠️ AI 컬링은 테스트 중인 기능입니다
얼굴 검출 · AI 선명도 · 객체 포함 등 ONNX 기반 판정은 **아직 테스트 중이라 오류가 있을 수 있습니다.** 결과를 맹신하지 마세요. 원본 파일은 삭제되지 않으며, 걸러진 컷은 숨김/표시로만 처리됩니다.

### 변경 사항
@CHANGELOG@

### Windows 설치 — 처음 실행 시
`RawBlow-Setup-v@VER@.exe` 를 실행하면 설치됩니다. 서명 미적용이라 첫 실행 시 SmartScreen 경고가 뜨면 **추가 정보 → 실행**을 누르세요. GPU 추론(webgpu)·VC++ 런타임 DLL이 모두 동봉돼 별도 재배포 설치 없이 동작합니다.

### macOS 설치 — 처음 실행 시 (중요)
이 앱은 ad-hoc 서명만 되어 있고 공증(notarize)되지 않았습니다. macOS 15+ 에서는 우클릭→열기가 통하지 않으니, **터미널**에서 압축 해제 후 아래를 실행하세요:
`xattr -dr com.apple.quarantine RawBlow.app`
'@
    return $tpl.Replace('@VER@', $Version).Replace('@CHANGELOG@', $Notes.Changelog)
}
