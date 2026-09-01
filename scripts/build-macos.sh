#!/usr/bin/env bash
# RawBlow — macOS(arm64) 릴리즈 빌드: .app 번들 + ad-hoc 서명 + 배포 zip
#
# 사용법 (저장소 루트에서):
#     bash scripts/build-macos.sh
#   산출물: dist/RawBlow.app  +  dist/RawBlow-v<버전>-macos-arm64.zip
#           (zip 파일명은 scripts/release-all.ps1 이 찾는 이름과 동일)
#
# 하는 일:
#   1) 빌드 경로 익명화(--remap-path-prefix)를 적용한 release 빌드
#      → 바이너리에 빌드한 사람의 홈 경로·사용자명이 안 박히게. BUILD.md "배포용 클린
#        릴리스 빌드" 참조. (+crt-static 은 Windows 전용이라 여기선 무관)
#   2) packaging/RawBlow.icns 없으면 gen-macos-icon.sh 로 생성
#   3) .app 번들 구성 + Info.plist(버전은 Cargo.toml 에서 주입)
#   4) codesign ad-hoc 서명 — 공증(notarize)은 안 함. 사용자는 첫 실행 전
#      `xattr -dr com.apple.quarantine RawBlow.app` 필요(릴리즈 노트에 안내).
#   5) ditto 로 zip — 서명·확장속성을 보존해야 하므로 `zip` 대신 ditto 를 쓴다.
#   6) 검증: 사용자명 미포함 + 서명 유효성
set -euo pipefail

cd "$(dirname "$0")/.."
REPO="$PWD"

[ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"

VER="$(sed -n 's/^version[[:space:]]*=[[:space:]]*"\([^"]*\)".*/\1/p' Cargo.toml | head -1)"
[ -n "$VER" ] || { echo "!! Cargo.toml 에서 version 을 찾지 못함" >&2; exit 1; }

APP="dist/RawBlow.app"
ZIP="dist/RawBlow-v${VER}-macos-arm64.zip"
echo "=== RawBlow macOS 빌드 v${VER} (arm64) ==="

# ── [1] 릴리즈 빌드 ────────────────────────────────────────────────────
# RUSTFLAGS 는 .cargo/config.toml 의 rustflags 를 덮어쓴다(append 아님) — macOS
# 타겟엔 config 쪽 설정이 없으므로 remap 만 넣으면 된다.
echo "==> cargo build --release (경로 익명화)"
RUSTFLAGS="--remap-path-prefix=$HOME=~" cargo build --release -p rawblow-app
BIN="target/release/rawblow"
[ -f "$BIN" ] || { echo "!! 빌드 산출물 없음: $BIN" >&2; exit 1; }

# ── [2] 아이콘 ─────────────────────────────────────────────────────────
ICNS="packaging/RawBlow.icns"
if [ ! -f "$ICNS" ]; then
    echo "==> $ICNS 없음 — 생성"
    bash scripts/gen-macos-icon.sh "$ICNS"
fi

# ── [3] 번들 구성 ──────────────────────────────────────────────────────
echo "==> .app 번들 구성 → $APP"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$BIN" "$APP/Contents/MacOS/rawblow"
cp "$ICNS" "$APP/Contents/Resources/RawBlow.icns"

cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>CFBundleDisplayName</key>
	<string>RawBlow</string>
	<key>CFBundleExecutable</key>
	<string>rawblow</string>
	<key>CFBundleIconFile</key>
	<string>RawBlow</string>
	<key>CFBundleIdentifier</key>
	<string>com.ascoeur9.rawblow</string>
	<key>CFBundleInfoDictionaryVersion</key>
	<string>6.0</string>
	<key>CFBundleName</key>
	<string>RawBlow</string>
	<key>CFBundlePackageType</key>
	<string>APPL</string>
	<key>CFBundleShortVersionString</key>
	<string>${VER}</string>
	<key>CFBundleVersion</key>
	<string>${VER}</string>
	<key>LSMinimumSystemVersion</key>
	<string>11.0</string>
	<key>NSHighResolutionCapable</key>
	<true/>
	<key>NSHumanReadableCopyright</key>
	<string>Copyright (c) 2026 Hare. All rights reserved.</string>
</dict>
</plist>
PLIST

# ── [4] ad-hoc 서명 ────────────────────────────────────────────────────
echo "==> ad-hoc 서명"
codesign --force --deep --sign - "$APP"

# ── [5] zip ────────────────────────────────────────────────────────────
echo "==> zip → $ZIP"
rm -f "$ZIP"
ditto -c -k --keepParent "$APP" "$ZIP"

# ── [6] 검증 ───────────────────────────────────────────────────────────
echo "==> 검증"
LEAK="$(strings "$APP/Contents/MacOS/rawblow" 2>/dev/null | grep -cE "(^|[^[:alnum:]_])${USER}([^[:alnum:]_]|$)" || true)"
if [ "$LEAK" != "0" ]; then
    echo "!! 바이너리에 사용자명($USER) 토큰이 ${LEAK}건 남아 있음 — 배포 금지" >&2
    exit 1
fi
codesign --verify --strict "$APP"
echo "   서명 OK · 사용자명 미포함 OK"

echo "=== 완료 ==="
echo "  $APP"
echo "  $ZIP  ($(du -h "$ZIP" | cut -f1))"
