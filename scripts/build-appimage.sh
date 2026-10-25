#!/usr/bin/env bash
# Build TopicBench-x86_64.AppImage from the release binary.
# Prefers an existing target/release/topicbench; rebuilds with --release if missing.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT_DIR="${OUT_DIR:-$ROOT/dist}"
APPDIR="$OUT_DIR/TopicBench.AppDir"
APPIMAGE_NAME="TopicBench-x86_64.AppImage"
BIN="$ROOT/target/release/topicbench"
APPIMAGETOOL="${APPIMAGETOOL:-$(command -v appimagetool || true)}"

if [[ ! -x "$BIN" ]]; then
  echo "No release binary at $BIN — building with cargo build --release"
  (cd "$ROOT" && cargo build --release)
fi

if [[ -z "$APPIMAGETOOL" || ! -x "$APPIMAGETOOL" ]]; then
  echo "appimagetool not found. Install to ~/.local/bin or set APPIMAGETOOL=" >&2
  echo "  curl -fsSL -o ~/.local/bin/appimagetool \\" >&2
  echo "    https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-x86_64.AppImage" >&2
  echo "  chmod +x ~/.local/bin/appimagetool" >&2
  exit 1
fi

rm -rf "$APPDIR"
mkdir -p "$APPDIR/usr/bin" \
  "$APPDIR/usr/share/applications" \
  "$APPDIR/usr/share/icons/hicolor/256x256/apps"

install -m 755 "$BIN" "$APPDIR/usr/bin/topicbench"
install -m 644 "$ROOT/packaging/topicbench.desktop" "$APPDIR/topicbench.desktop"
install -m 644 "$ROOT/packaging/topicbench.desktop" "$APPDIR/usr/share/applications/topicbench.desktop"
install -m 644 "$ROOT/packaging/topicbench.png" "$APPDIR/topicbench.png"
install -m 644 "$ROOT/packaging/topicbench.png" "$APPDIR/usr/share/icons/hicolor/256x256/apps/topicbench.png"

# AppRun: launch the bundled binary from any cwd
cat > "$APPDIR/AppRun" <<'EOF'
#!/bin/sh
HERE="$(dirname "$(readlink -f "$0")")"
exec "$HERE/usr/bin/topicbench" "$@"
EOF
chmod 755 "$APPDIR/AppRun"

mkdir -p "$OUT_DIR"
# ARCH is required by appimagetool for the output name
export ARCH=x86_64
# Prefer extracting without FUSE when unavailable (common on containers)
export APPIMAGE_EXTRACT_AND_RUN="${APPIMAGE_EXTRACT_AND_RUN:-1}"

(cd "$OUT_DIR" && "$APPIMAGETOOL" "$APPDIR" "$APPIMAGE_NAME")

echo "Built: $OUT_DIR/$APPIMAGE_NAME"
sha256sum "$OUT_DIR/$APPIMAGE_NAME"
