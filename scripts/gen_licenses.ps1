# 서드파티 라이센스 수집(#39): 배포 타겟(Win/macOS/Linux) 의존성 합집합의 라이센스
# 전문을 cargo 레지스트리 소스에서 모아 JSON으로 출력한다. 릴리즈 전 의존성이 바뀌면 재실행.
#   .\scripts\gen_licenses.ps1
# 출력: crates/rawblow-app/assets/third_party_licenses.json (include_str!로 임베드)
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
$outDir = Join-Path $root 'crates\rawblow-app\assets'
if (-not (Test-Path $outDir)) { New-Item -ItemType Directory -Force $outDir | Out-Null }
$out = Join-Path $outDir 'third_party_licenses.json'

# 1) 배포 가능 타겟의 normal 의존성 합집합(dev/build 제외). --target은 그래프 필터일 뿐
#    툴체인 설치가 필요 없다.
$targets = @('x86_64-pc-windows-msvc', 'x86_64-apple-darwin', 'aarch64-apple-darwin', 'x86_64-unknown-linux-gnu')
$set = New-Object 'System.Collections.Generic.HashSet[string]'
foreach ($t in $targets) {
    cargo tree --target $t -e normal --prefix none -p rawblow-app | ForEach-Object {
        $l = $_.Trim()
        if ($l -match '^(\S+) v([0-9][^\s]*)') { [void]$set.Add($Matches[1] + '|' + $Matches[2]) }
    }
}

$meta = cargo metadata --format-version 1 | ConvertFrom-Json
$pkgs = $meta.packages |
    Where-Object { $set.Contains($_.name + '|' + $_.version) -and $_.name -notmatch '^rawblow-' } |
    Sort-Object name, version

# 2) 라이센스 텍스트 풀(내용 기준 중복 제거).
$texts = New-Object System.Collections.ArrayList
$textIdx = @{}
$sha1 = [System.Security.Cryptography.SHA1]::Create()
function Add-Text([string]$content) {
    $norm = (($content -replace "`r`n", "`n") -replace "`r", "`n").Trim() + "`n"
    $key = [BitConverter]::ToString($sha1.ComputeHash([Text.Encoding]::UTF8.GetBytes($norm)))
    if (-not $textIdx.ContainsKey($key)) {
        [void]$texts.Add($norm)
        $textIdx[$key] = $texts.Count - 1
    }
    $textIdx[$key]
}

# 라이센스 파일이 패키지에 동봉되지 않은 MIT 크레이트용 표준 전문(SPDX 템플릿).
$mitTemplate = @'
MIT License

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in
all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
THE SOFTWARE.
'@

function Get-LicenseFiles($p) {
    $dir = Split-Path $p.manifest_path
    $files = @(Get-ChildItem $dir -File | Where-Object { $_.Name -match '^(LICENSE|LICENCE|COPYING|UNLICENSE|NOTICE)' })
    # egui 기본 폰트 패키지: 폰트별 라이센스(.txt)가 fonts/ 아래에 있다(OFL·UFL·Hack·emoji).
    if ($p.name -eq 'epaint_default_fonts') {
        $files += @(Get-ChildItem (Join-Path $dir 'fonts') -File -Filter '*.txt')
    }
    $files
}

# 3) 1차 패스: 파일명으로 식별 가능한 범용 전문을 canonical로 등록(동봉 파일이 없는
#    크레이트의 폴백). Apache-2.0 등은 저작권자 무관 범용 텍스트라 공유해도 정확하다.
$canonical = @{}
$canonPatterns = @(
    @{ re = 'APACHE'; id = 'Apache-2.0' },
    @{ re = 'UNICODE'; id = 'Unicode-3.0' },
    @{ re = '^UNLICENSE'; id = 'Unlicense' },
    @{ re = 'BOOST'; id = 'BSL-1.0' },
    @{ re = 'ZLIB'; id = 'Zlib' },
    @{ re = '^LICENSE-MIT'; id = 'MIT' }
)
foreach ($p in $pkgs) {
    foreach ($f in (Get-LicenseFiles $p)) {
        foreach ($c in $canonPatterns) {
            if ($f.Name -match $c.re -and -not $canonical.ContainsKey($c.id)) {
                $canonical[$c.id] = Add-Text ([IO.File]::ReadAllText($f.FullName))
            }
        }
    }
}
if (-not $canonical.ContainsKey('MIT')) { $canonical['MIT'] = Add-Text $mitTemplate }
# 패키지에 동봉되지 않는 표준 전문(저장소 동봉본): CC0(hexf-parse), GPL-3.0(LGPL-3.0의 모체
# — LGPL-3.0은 GPL-3.0의 보충 조항이라 전문 고지에 GPL-3.0 본문이 함께 필요).
$canonical['CC0-1.0'] = Add-Text ([IO.File]::ReadAllText((Join-Path $PSScriptRoot 'licenses\CC0-1.0.txt')))
$gpl3Idx = Add-Text ([IO.File]::ReadAllText((Join-Path $PSScriptRoot 'licenses\GPL-3.0.txt')))

# 4) 2차 패스: 크레이트별 항목. 듀얼 라이센스는 MIT 우선(허용 시 선택권 행사),
#    NOTICE 파일은 Apache-2.0 §4(d)에 따라 항상 보존.
$entries = @()
foreach ($p in $pkgs) {
    $files = Get-LicenseFiles $p
    $expr = $p.license
    if (-not $expr) { $expr = 'see repository' }
    $picked = @()
    if ($p.name -eq 'epaint_default_fonts') {
        $picked = $files
    }
    else {
        $mitFile = $files | Where-Object { $_.Name -match 'MIT' } | Select-Object -First 1
        $plain = $files | Where-Object { $_.Name -match '^(LICENSE|LICENCE|COPYING)([._-](txt|md|markdown))?$' } | Select-Object -First 1
        $apache = $files | Where-Object { $_.Name -match 'APACHE' } | Select-Object -First 1
        $any = $files | Where-Object { $_.Name -notmatch '^NOTICE' } | Select-Object -First 1
        if ($expr -match 'MIT' -and $mitFile) { $picked = @($mitFile) }
        elseif ($plain) { $picked = @($plain) }
        elseif ($expr -match 'Apache' -and $apache) { $picked = @($apache) }
        elseif ($any) { $picked = @($any) }
        $picked = @($picked) + @($files | Where-Object { $_.Name -match '^NOTICE' })
    }
    $tIdx = @()
    foreach ($f in $picked) { if ($f) { $tIdx += Add-Text ([IO.File]::ReadAllText($f.FullName)) } }
    if ($tIdx.Count -eq 0) {
        # 동봉 파일 없음 → 선언된 표현식에서 첫 매칭 canonical로 폴백.
        foreach ($id in @('MIT', 'Apache-2.0', 'Unicode-3.0', 'Unlicense', 'BSL-1.0', 'Zlib', 'CC0-1.0')) {
            if ($expr -match [regex]::Escape($id) -and $canonical.ContainsKey($id)) { $tIdx += $canonical[$id]; break }
        }
    }
    if ($expr -match 'LGPL-3\.0') { $tIdx += $gpl3Idx }
    $entries += [pscustomobject]@{ n = $p.name; v = "$($p.version)"; l = $expr; r = $p.repository; t = @($tIdx) }
}

$doc = [pscustomobject]@{
    generated = (Get-Date -Format 'yyyy-MM-dd')
    crates    = $entries
    texts     = $texts.ToArray()
}
$json = $doc | ConvertTo-Json -Depth 6 -Compress
[IO.File]::WriteAllText($out, $json, (New-Object Text.UTF8Encoding($false)))
"crates: $($entries.Count) · texts: $($texts.Count) · $([math]::Round((Get-Item $out).Length/1KB)) KB -> $out"
