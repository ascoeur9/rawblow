#!/usr/bin/env bash
# RawBlow — macOS 에서 GitHub 릴리즈 생성 (scripts/release-all.ps1 의 GitHub 절반을 이식)
#
# 왜 별도 스크립트인가: release-all.ps1 은 PowerShell + Windows 전용(NSIS 인스톨러
# 빌드, dumpbin 검증, MSIX 스토어 제출)이라 macOS 에서 못 돈다. 이 스크립트는 Mac 에서
# 할 수 있는 부분 — macOS 빌드 + 태그 + gh 릴리즈 — 만 담당한다.
#
# ⚠️ 이 스크립트는 Windows 인스톨러를 만들지 못한다. dist/ 에 Windows 에서 빌드한
#    RawBlow-Setup-v<버전>.exe 를 미리 복사해 두면 함께 첨부하고, 없으면 경고 후
#    macOS 에셋만 올린다. MS 스토어 제출은 Windows 에서 별도로 해야 한다.
#
# 사용법 (저장소 루트에서):
#     bash scripts/release-github.sh              # 빌드 + 태그 푸시 + 릴리즈 게시
#     bash scripts/release-github.sh --dry-run    # 빌드·본문 생성까지만, 푸시/게시 없음
#     bash scripts/release-github.sh --draft      # draft 로 생성(수동 게시)
#     bash scripts/release-github.sh --skip-build # dist/ 산출물 그대로 사용
#
# 전제:
#   - Cargo.toml 의 version 이 이번 릴리즈 버전 (미리 범프)
#   - scripts/release-notes/v<버전>.md 작성 (형식은 scripts/_release-notes.ps1 헤더 참조)
#   - gh CLI 인증됨 (gh auth status)
set -euo pipefail

cd "$(dirname "$0")/.."
REPO="$PWD"

DRY_RUN=0; DRAFT=0; SKIP_BUILD=0
for arg in "$@"; do
    case "$arg" in
        --dry-run)    DRY_RUN=1 ;;
        --draft)      DRAFT=1 ;;
        --skip-build) SKIP_BUILD=1 ;;
        *) echo "!! 알 수 없는 옵션: $arg" >&2; exit 1 ;;
    esac
done

VER="$(sed -n 's/^version[[:space:]]*=[[:space:]]*"\([^"]*\)".*/\1/p' Cargo.toml | head -1)"
[ -n "$VER" ] || { echo "!! Cargo.toml 에서 version 을 찾지 못함" >&2; exit 1; }
TAG="v${VER}"
NOTES="scripts/release-notes/v${VER}.md"

echo "=== RawBlow 릴리즈 ${TAG} (macOS 에서 실행) ==="
[ "$DRY_RUN" = 1 ] && echo "(DryRun — 게시/푸시 없음)"

# ── 릴리즈 노트 ────────────────────────────────────────────────────────
# 노트부터 강제: 없으면 즉시 중단(release-all.ps1 과 동일 방침).
[ -f "$NOTES" ] || { echo "!! 릴리즈 노트 없음: $NOTES  → 템플릿: scripts/release-notes/v0.6.0.md" >&2; exit 1; }

# <!-- @github --> / <!-- @changelog --> 구획 추출 (다음 마커 전까지).
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

BODY_FILE="$(mktemp -t rawblow-ghbody).md"
GH_SECTION="$(section github)"
if [ -n "$(printf '%s' "$GH_SECTION" | tr -d '[:space:]')" ]; then
    printf '%s\n' "$GH_SECTION" > "$BODY_FILE"
else
    # @github 이 없으면 @changelog 를 보일러플레이트로 감싼다(_release-notes.ps1 과 동일 템플릿).
    CHANGELOG="$(section changelog)"
    [ -n "$(printf '%s' "$CHANGELOG" | tr -d '[:space:]')" ] || {
        echo "!! 본문을 만들 수 없음 — $NOTES 에 <!-- @github --> 또는 <!-- @changelog --> 필요" >&2; exit 1; }
    cat > "$BODY_FILE" <<BODY
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
fi
echo "==> 릴리즈 본문: $BODY_FILE ($(wc -l < "$BODY_FILE" | tr -d ' ') 줄)"

# ── 태그 중복 방지 (버전 범프 잊음 방지) ───────────────────────────────
if [ "$DRY_RUN" = 0 ] && [ -n "$(git tag --list "$TAG")" ]; then
    echo "!! 태그 $TAG 이 이미 존재합니다 — Cargo.toml 버전을 올렸는지 확인하세요(재릴리즈면 태그를 먼저 삭제)." >&2
    exit 1
fi

# ── macOS 빌드 ─────────────────────────────────────────────────────────
MAC_ZIP="dist/RawBlow-v${VER}-macos-arm64.zip"
if [ "$SKIP_BUILD" = 0 ]; then
    echo "==> macOS 빌드"
    bash scripts/build-macos.sh
fi
[ -f "$MAC_ZIP" ] || { echo "!! macOS 산출물 없음: $MAC_ZIP" >&2; exit 1; }

# ── 에셋 수집 ──────────────────────────────────────────────────────────
ASSETS=("$MAC_ZIP")
WIN_EXE="dist/RawBlow-Setup-v${VER}.exe"
if [ -f "$WIN_EXE" ]; then
    ASSETS+=("$WIN_EXE")
else
    echo "!! 경고: Windows 인스톨러 없음($WIN_EXE) — macOS 에셋만 올립니다." >&2
    echo "   Windows 빌드는 Windows 에서 scripts\\build-release-windows.ps1 로 만들어 dist/ 에 넣으세요." >&2
    echo "   MS 스토어 제출도 Windows 전용(scripts\\submit-msix-store.ps1)이라 여기선 못 합니다." >&2
fi
echo "==> 에셋: ${ASSETS[*]}"

# ── 태그 + 릴리즈 ──────────────────────────────────────────────────────
if [ "$DRY_RUN" = 1 ]; then
    echo "  [DryRun] 태그 $TAG 생성·푸시 생략 / gh release create 생략"
    echo "  [DryRun] 본문 미리보기 → $BODY_FILE"
    exit 0
fi

echo "==> 태그 생성·푸시"
git tag -a "$TAG" -m "release: $TAG"
git push origin "$TAG"

echo "==> gh release create"
GH_ARGS=(release create "$TAG" --title "RawBlow $TAG" --notes-file "$BODY_FILE")
[ "$DRAFT" = 1 ] && GH_ARGS+=(--draft)
gh "${GH_ARGS[@]}" "${ASSETS[@]}"

echo "✅ GitHub 릴리즈 게시: https://github.com/ascoeur9/rawblow/releases/tag/$TAG"
