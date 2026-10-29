#!/usr/bin/env bash
# Build topicbench-linux-x64 linked for glibc 2.35 (Ubuntu 22.04 / Debian 12+).
# Uses cargo-zigbuild + zig (no Docker required). On Omarchy/Arch, native
# cargo build links a newer glibc and will fail on Debian/Ubuntu LTS.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
command -v cargo-zigbuild >/dev/null || cargo install cargo-zigbuild --locked
command -v zig >/dev/null || { echo "zig required (mise use -g zig@0.14.1)" >&2; exit 1; }
cargo zigbuild --release --target x86_64-unknown-linux-gnu.2.35
OUT="$ROOT/target/x86_64-unknown-linux-gnu/release/topicbench"
install -m 755 "$OUT" "${1:-$ROOT/dist/topicbench-linux-x64}"
objdump -T "${1:-$ROOT/dist/topicbench-linux-x64}" | grep -o 'GLIBC_[0-9.]*' | sort -Vu | tail -5
