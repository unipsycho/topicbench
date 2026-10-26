#!/usr/bin/env bash
# Build TopicBench-windows-x64-portable.zip from a Windows PE binary.
# Default input: topicbench-windows-x64.exe (cwd or WIN_EXE=) or cross-built target.
# Keeps the bare .exe as the in-app updater asset; this zip is the human download.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT_DIR="${OUT_DIR:-$ROOT/dist}"
ZIP_NAME="TopicBench-windows-x64-portable.zip"
STAGE="$OUT_DIR/windows-portable"

# Prefer explicit path, then release-named copy, then cargo cross-build output
WIN_EXE="${WIN_EXE:-}"
if [[ -z "$WIN_EXE" ]]; then
  for candidate in \
    "$ROOT/topicbench-windows-x64.exe" \
    "$ROOT/target/x86_64-pc-windows-gnu/release/topicbench.exe" \
    "$ROOT/target/x86_64-pc-windows-msvc/release/topicbench.exe"
  do
    if [[ -f "$candidate" ]]; then
      WIN_EXE="$candidate"
      break
    fi
  done
fi

if [[ -z "$WIN_EXE" || ! -f "$WIN_EXE" ]]; then
  echo "Windows PE not found. Set WIN_EXE=/path/to/topicbench-windows-x64.exe" >&2
  exit 1
fi

command -v zip >/dev/null || { echo "zip required" >&2; exit 1; }

rm -rf "$STAGE"
mkdir -p "$STAGE"
install -m 644 "$WIN_EXE" "$STAGE/TopicBench.exe"
cat > "$STAGE/README.txt" <<'EOF'
TopicBench portable (Windows x64)

Unzip this folder anywhere and run TopicBench.exe from that folder.
No installer. Settings, profiles, and Pro license.json live in the
topicbench subfolder next to this exe (same place as license-request.json).

Checksums for this release: SHA256SUMS on the GitHub Release.
EOF

mkdir -p "$OUT_DIR"
rm -f "$OUT_DIR/$ZIP_NAME"
(cd "$STAGE" && zip -9 -r "$OUT_DIR/$ZIP_NAME" TopicBench.exe README.txt)

echo "Built: $OUT_DIR/$ZIP_NAME (from $WIN_EXE)"
sha256sum "$OUT_DIR/$ZIP_NAME"
