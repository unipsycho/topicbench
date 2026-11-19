#!/usr/bin/env bash
# Build topicbench-windows-x64.exe with the MinGW-w64 toolchain (the one every release
# since v1.0.0 used). Do NOT use cargo-zigbuild for the Windows exe — it links a different
# runtime and produced a ~28 MB binary vs the ~35 MB MinGW build; untested on Windows.
#
# Toolchain: user-local MinGW-w64 extracted to ~/.local/opt/mingw-w64 (binutils, headers,
# crt, winpthreads, gcc) — `pacman -S mingw-w64-gcc` needs sudo and was not used.
# .cargo/config.toml (committed) points the windows-gnu linker at x86_64-w64-mingw32-gcc;
# build.rs uses x86_64-w64-mingw32-windres to embed assets/icons/app.ico and force-links
# resource.o so GNU ld keeps the icon.
#
# Usage: scripts/build-windows-mingw.sh [output.exe]   (default dist/topicbench-windows-x64.exe)
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
MINGW_BIN="${MINGW_BIN:-$HOME/.local/opt/mingw-w64/usr/bin}"
[[ -d "$MINGW_BIN" ]] && export PATH="$MINGW_BIN:$PATH"
command -v x86_64-w64-mingw32-gcc >/dev/null || { echo "MinGW-w64 gcc not on PATH (expected in $MINGW_BIN)" >&2; exit 1; }
command -v x86_64-w64-mingw32-windres >/dev/null || { echo "x86_64-w64-mingw32-windres missing — icon would not embed" >&2; exit 1; }
rustup target add x86_64-pc-windows-gnu >/dev/null
cargo build --release --target x86_64-pc-windows-gnu 2>&1 | tee /dev/stderr | grep -q "icon embed failed" && { echo "icon embed failed — fix toolchain" >&2; exit 1; } || true
OUT="${1:-$ROOT/dist/topicbench-windows-x64.exe}"
mkdir -p "$(dirname "$OUT")"
install -m 755 "$ROOT/target/x86_64-pc-windows-gnu/release/topicbench.exe" "$OUT"
file "$OUT" | cut -c1-100
ls -l "$OUT"
