#!/usr/bin/env bash
# scripts/release-notes/v<버전>.md 에서 GitHub 릴리즈 본문을 stdout 으로 출력.
# @github 구획이 있으면 그대로, 없으면 @changelog 를 보일러플레이트로 감싼다.
# (scripts/_release-notes.ps1 Build-GitHubBody 와 같은 규칙)
set -euo pipefail
VER="${1:-}"
[ -n "$VER" ] || { echo "usage: github-release-body.sh <version>" >&2; exit 1; }
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
NOTES="$ROOT/scripts/release-notes/v${VER}.md"
[ -f "$NOTES" ] || { echo "!! 릴리즈 노트 없음: $NOTES" >&2; exit 1; }

section() {
    awk -v want="$1" '
        /^[[:space:]]*<!--[[:space:]]*@/ {
            cur = $0
            sub(/^[[:space:]]*<!--[[:space:]]*@/, "", cur)
            sub(/[[:space:]]*-->.*$/, "", cur)
            split(cur, p, /[[:space:]]+/)
            on = (p[1] == want)
            next
        }
        on { print }
    ' "$NOTES"
}

GH_SECTION="$(section github)"
if [ -n "$(printf '%s' "$GH_SECTION" | tr -d '[:space:]')" ]; then
    printf '%s\n' "$GH_SECTION"
    exit 0
fi

CHANGELOG="$(section changelog)"
[ -n "$(printf '%s' "$CHANGELOG" | tr -d '[:space:]')" ] || {
    echo "!! $NOTES 에 <!-- @github --> 또는 <!-- @changelog --> 필요" >&2
    exit 1
}

cat <<BODY
## RawBlow v${VER}

> **다운로드** — Windows(x64) \`RawBlow-Setup-v${VER}.exe\` · macOS(arm64) \`RawBlow-v${VER}-macos-arm64.zip\`

### ⚠️ AI 컬링은 테스트 중인 기능입니다
얼굴 검출 · AI 선명도 · 객체 포함 등 ONNX 기반 판정은 **아직 테스트 중이라 오류가 있을 수 있습니다.** 결과를 맹신하지 마세요. 원본 파일은 삭제되지 않으며, 걸러진 컷은 숨김/표시로만 처리됩니다.

### 변경 사항
${CHANGELOG}

### Windows 설치 — 처음 실행 시
\`RawBlow-Setup-v${VER}.exe\` 를 실행하면 설치됩니다. 서명 미적용이라 첫 실행 시 SmartScreen 경고가 뜨면 **추가 정보 → 실행**을 누르세요. GPU 추론(webgpu)·VC++ 런타임 DLL이 모두 동봉돼 별도 재배포 설치 없이 동작합니다.

### macOS 설치 — 처음 실행 시 (중요)
이 앱은 ad-hoc 서명만 되어 있고 공증(notarize)되지 않았습니다. macOS 15+ 에서는 우클릭→열기가 통하지 않으니, **터미널**에서 압축 해제 후 아래를 실행하세요:
\`xattr -dr com.apple.quarantine RawBlow.app\`
BODY
