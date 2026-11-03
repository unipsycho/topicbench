#!/usr/bin/env bash
# Build topicbench_<ver>-1_<arch>.deb from a Linux x64 TopicBench binary (nfpm).
# Prefers the glibc-2.35 release binary when present. Requires: nfpm on PATH.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT_DIR="${OUT_DIR:-$ROOT/dist}"
NFPM="${NFPM:-$(command -v nfpm || true)}"
ARCH="${ARCH:-amd64}"

version_from_cargo() {
  sed -n 's/^version = "\([^"]*\)"/\1/p' "$ROOT/Cargo.toml" | head -1
}

resolve_bin() {
  if [[ -n "${TOPICBENCH_BIN:-}" && -x "$TOPICBENCH_BIN" ]]; then
    echo "$TOPICBENCH_BIN"
    return
  fi
  local candidate
  for candidate in \
    "$OUT_DIR/topicbench-linux-x64" \
    "$ROOT/dist/topicbench-linux-x64" \
    "$ROOT/target/x86_64-unknown-linux-gnu/release/topicbench" \
    "$ROOT/target/release/topicbench"
  do
    if [[ -x "$candidate" ]]; then
      echo "$candidate"
      return
    fi
  done
  return 1
}

if [[ -z "$NFPM" || ! -x "$NFPM" ]]; then
  echo "nfpm not found. Install from https://github.com/goreleaser/nfpm/releases" >&2
  echo "  or: go install github.com/goreleaser/nfpm/v2/cmd/nfpm@latest" >&2
  exit 1
fi

VERSION="${VERSION:-$(version_from_cargo)}"
BIN="$(resolve_bin || true)"
if [[ -z "${BIN:-}" ]]; then
  echo "No TopicBench binary found — building with cargo build --release" >&2
  (cd "$ROOT" && cargo build --release)
  BIN="$ROOT/target/release/topicbench"
fi

mkdir -p "$OUT_DIR"
# nfpm expands ${VAR} from the process environment in the config file.
# Make BIN absolute so nfpm can read it regardless of cwd.
BIN="$(cd "$(dirname "$BIN")" && pwd)/$(basename "$BIN")"
export VERSION ARCH BIN

# nfpm names debs with release suffix (release: "1" in nfpm.yaml).
DEB_NAME="topicbench_${VERSION}-1_${ARCH}.deb"
rm -f "${OUT_DIR}/${DEB_NAME}"

(cd "$ROOT" && "$NFPM" package -f packaging/nfpm.yaml -p deb -t "$OUT_DIR")

if test ! -f "${OUT_DIR}/${DEB_NAME}"; then
  echo "nfpm did not write ${OUT_DIR}/${DEB_NAME}" >&2
  exit 1
fi

echo "Built: ${OUT_DIR}/${DEB_NAME} (from $BIN)"
sha256sum "${OUT_DIR}/${DEB_NAME}"
