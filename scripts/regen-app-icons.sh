#!/usr/bin/env bash
# Regenerate Linux packaging + window PNGs from Mike’s Windows ICO (assets/icons/app.ico).
# Requires ImageMagick (`magick` or `convert`).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
ICO="$ROOT/assets/icons/app.ico"
OUT="$ROOT/packaging/icons"
MAGICK="$(command -v magick || true)"
CONVERT="$(command -v convert || true)"

if [[ ! -f "$ICO" ]]; then
  echo "missing $ICO" >&2
  exit 1
fi

# Usage: im_write <input...> <output>
# Drops date/time PNG chunks so hashes stay stable across regenerations.
im_write() {
  if [[ $# -lt 2 ]]; then
    echo "im_write needs input and output" >&2
    exit 1
  fi
  local out="${*: -1}"
  local -a inputs=("${@:1:$#-1}")
  if [[ -n "$MAGICK" ]]; then
    magick "${inputs[@]}" -define png:exclude-chunks=date,time "$out"
  elif [[ -n "$CONVERT" ]]; then
    convert "${inputs[@]}" -define png:exclude-chunks=date,time "$out"
  else
    echo "need ImageMagick (magick/convert)" >&2
    exit 1
  fi
}

mkdir -p "$OUT"
# ICO frames: [0]16 [1]24 [2]32 [3]48 [4]64 [5]96 [6]128 [7]256
for idx_size in 0:16 1:24 2:32 3:48 4:64 6:128 7:256; do
  idx=${idx_size%%:*}
  sz=${idx_size##*:}
  im_write "$ICO[$idx]" "PNG32:$OUT/topicbench-${sz}.png"
done
# 512 not in ICO — Lanczos upscale from 256
im_write "$OUT/topicbench-256.png" -filter Lanczos -resize 512x512 "PNG32:$OUT/topicbench-512.png"

# Canonical 256 for .desktop / AppImage root / AUR / pixmaps
cp -f "$OUT/topicbench-256.png" "$ROOT/packaging/topicbench.png"
cp -f "$OUT/topicbench-256.png" "$ROOT/packaging/aur/topicbench-bin/topicbench.png"
# egui window/taskbar icon (same art as PE-embedded app.ico)
cp -f "$OUT/topicbench-256.png" "$ROOT/assets/icons/topicbench-app-icon-256.png"

echo "Regenerated packaging icons from $ICO"
sha256sum "$OUT/topicbench-256.png" "$ROOT/packaging/topicbench.png"
