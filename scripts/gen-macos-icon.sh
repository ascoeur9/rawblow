#!/usr/bin/env bash
# RawBlow — macOS 앱 아이콘(.icns) 생성 스크립트
#
# 사용법 (저장소 루트에서):
#     bash scripts/gen-macos-icon.sh [출력경로.icns]
#   기본 출력: packaging/RawBlow.icns (로컬 전용; build-macos.sh 가 이걸 번들에 넣는다)
#
# 하는 일:
#   1) examples/gen_macos_icon 로 macOS 표준 여백(본체 824/1024)을 적용한
#      아이콘셋 PNG 10종을 logo.rs 기하 그대로 렌더
#   2) iconutil 로 .icns 합성
#
# ★ 왜: 아이콘이 캔버스를 꽉 채우면(풀블리드) cmd+Tab·Dock에서 다른 앱보다
#   커 보인다. macOS 는 가장자리에 약 9.77% 여백을 둔 본체를 기대한다.
set -euo pipefail

cd "$(dirname "$0")/.."
OUT="${1:-packaging/RawBlow.icns}"

[ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"

ICONSET="$(mktemp -d)/RawBlow.iconset"
echo "==> 아이콘셋 렌더 (logo.rs 기하 + macOS 여백)"
cargo run -q -p rawblow-app --example gen_macos_icon -- "$ICONSET"

echo "==> iconutil 합성 → $OUT"
mkdir -p "$(dirname "$OUT")"
iconutil -c icns "$ICONSET" -o "$OUT"
rm -rf "$(dirname "$ICONSET")"
echo "==> 완료: $OUT"
