#!/usr/bin/env bash
# Build topicbench-<ver>-1.x86_64.rpm from a Linux x64 TopicBench binary (nfpm).
# Prefers the glibc-2.35 release binary when present. Requires: nfpm on PATH.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT_DIR="${OUT_DIR:-$ROOT/dist}"
NFPM="${NFPM:-$(command -v nfpm || true)}"
ARCH="${ARCH:-amd64}" # nfpm arch; RPM filename still uses x86_64

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
export VERSION ARCH BIN
# Make BIN absolute so nfpm can read it regardless of cwd.
BIN="$(cd "$(dirname "$BIN")" && pwd)/$(basename "$BIN")"
export BIN

RPM_NAME="topicbench-${VERSION}-1.x86_64.rpm"
rm -f "$OUT_DIR/$RPM_NAME"

(cd "$ROOT" && "$NFPM" package -f packaging/nfpm.yaml -p rpm -t "$OUT_DIR")

# nfpm may name amd64 packages with amd64 or x86_64 depending on version — normalize.
shopt -s nullglob
built=( "$OUT_DIR"/topicbench-"${VERSION}"-*.rpm )
if [[ ${#built[@]} -eq 0 ]]; then
  echo "nfpm did not write an rpm under $OUT_DIR" >&2
  exit 1
fi
if [[ "${built[0]}" != "$OUT_DIR/$RPM_NAME" ]]; then
  mv -f "${built[0]}" "$OUT_DIR/$RPM_NAME"
fi

echo "Built: $OUT_DIR/$RPM_NAME (from $BIN)"
sha256sum "$OUT_DIR/$RPM_NAME"
