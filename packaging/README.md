# Release packaging (OSS)

Scripts under `scripts/` build GitHub Release assets from one Linux x64 binary
(prefer `dist/topicbench-linux-x64` from `scripts/build-linux-glibc235.sh`).

**Canonical Linux download:** bare `topicbench-linux-x64` (also the in-app updater asset).  
**Not published:** AppImage, `.deb`, and `.rpm` are dropped from Releases / Downloads advertising (prefer AUR later for Arch). Legacy `scripts/build-appimage.sh`, `scripts/build-rpm.sh`, `scripts/build-deb.sh`, and `nfpm.yaml` may remain in-tree but must not be attached to GitHub Releases or linked from the README.

`.deb` / `.rpm` via nfpm ship AppStream metainfo (`packaging/ca.agileautomation.TopicBench.metainfo.xml`) so KDE Discover shows **Agile Automation** and SPDX license `MIT OR Apache-2.0`, plus Debian `copyright` under `/usr/share/doc/topicbench/`.

## Assets to attach each release

| Asset | Script / source | In-app updater |
| --- | --- | --- |
| `topicbench-linux-x64` | `scripts/build-linux-glibc235.sh` | **Yes** — replaces this file |
| `topicbench-windows-x64.exe` | `scripts/build-windows-mingw.sh` (MinGW-w64, **not** zigbuild) | **Yes** — replaces this file |
| `TopicBench-windows-x64-portable.zip` | `scripts/build-windows-portable-zip.sh` | Notify only (zip is the human download) |
| `SHA256SUMS` | `sha256sum` over the assets above | Required for apply path |

Arch / Omarchy: AUR [`topicbench-bin`](aur/topicbench-bin) wraps `topicbench-linux-x64` (see [`aur/README.md`](aur/README.md)).

## Updates behavior

- **Check / notify:** every install polls GitHub Releases latest (Settings → Check now, and quiet startup).
- **Apply in place:** when the running binary's directory is writable — typically the bare Linux binary or Windows portable / bare `.exe`.
- **Root-owned paths (e.g. `/usr/bin`):** Linux stages the verified payload in a temp dir, then elevates via `pkexec` (polkit GUI) or `sudo -n` to swap the binary while preserving owner/mode. If elevation is refused or unavailable, the updater opens the release page with a clear error (same fallback as missing assets).
- **AUR:** users still get the update banner; package-managed installs may still prefer `yay`/pacman when elevation is declined, but an elevated in-place swap works when the user authorizes it.

## Build + upload (M17) — repeatable release procedure

Everything below is run from the repo root on M17 after the version bump is on `main`
(`Cargo.toml` + `Cargo.lock`). `dist/` is git-ignored.

```bash
V=$(sed -n 's/^version = "\([^"]*\)"/\1/p' Cargo.toml | head -1)   # e.g. 1.2.9
D=dist/release-$V; mkdir -p "$D"

# 1) Linux: glibc 2.35 via cargo-zigbuild + zig (canonical updater asset)
./scripts/build-linux-glibc235.sh "$D/topicbench-linux-x64"

# 2) Windows: MinGW-w64 (NOT zigbuild) — toolchain lives in ~/.local/opt/mingw-w64
./scripts/build-windows-mingw.sh "$D/topicbench-windows-x64.exe"

# 3) Portable zip (use absolute paths)
WIN_EXE=$PWD/$D/topicbench-windows-x64.exe OUT_DIR=$PWD/$D ./scripts/build-windows-portable-zip.sh
rm -rf "$D/windows-portable"

# 4) Checksums (from inside $D, so names are bare)
(cd "$D" && sha256sum topicbench-linux-x64 topicbench-windows-x64.exe \
  TopicBench-windows-x64-portable.zip > SHA256SUMS)

# 5) Docs: README download links + AUR pkgver / sha256sums[0] (Linux binary hash), push main
sed -i "s/v[0-9]\+\.[0-9]\+\.[0-9]\+/v$V/g" README.md      # check the diff — download block only
#    packaging/aur/topicbench-bin/PKGBUILD: pkgver=$V, pkgrel=1, sha256sums[0]=<linux sha>

# 6) Tag + GitHub Release (assets: the four files in $D)
git tag -a "v$V" -m "TopicBench $V" && git push origin "v$V"
gh release create "v$V" "$D"/topicbench-linux-x64 "$D"/topicbench-windows-x64.exe \
  "$D"/TopicBench-windows-x64-portable.zip "$D"/SHA256SUMS \
  --title "TopicBench $V" --notes-file <notes.md> --verify-tag
```

### Toolchain facts (do not re-derive)

| Target | Tool | Notes |
|--------|------|-------|
| Linux | `cargo zigbuild --target x86_64-unknown-linux-gnu.2.35` (zig 0.14.1 via mise) | Native `cargo build` on Arch links glibc 2.43 and breaks Debian/Ubuntu LTS. |
| Windows | `cargo build --release --target x86_64-pc-windows-gnu` with **`~/.local/opt/mingw-w64/usr/bin` on `PATH`** | `.cargo/config.toml` sets linker `x86_64-w64-mingw32-gcc`; `build.rs` embeds the icon with `x86_64-w64-mingw32-windres`. Expected size ≈ 35 MB. |
| Windows (don't) | `cargo zigbuild --target x86_64-pc-windows-gnu` | Works and embeds the icon with `WINDRES=llvm-windres`, but yields a ~28 MB binary that differs from all prior releases and cannot be run here (no Wine). 1.2.9 initially shipped this by mistake and was replaced. |

No Wine / Windows host on M17, so the Windows exe is verified by build output only (`file` = PE32+ GUI,
`.rsrc` present, version string embedded, size ≈ 35 MB), not by running it.

### Verify the release like the updater does

```bash
curl -s https://api.github.com/repos/unipsycho/topicbench/releases/latest   # tag_name == v$V, 4 assets
# download all four assets, then:  sha256sum -c SHA256SUMS
```

The in-app updater replaces the bare `topicbench-linux-x64` / `topicbench-windows-x64.exe`
asset and requires `SHA256SUMS` (see `src/update/github.rs`: `ASSET_NAME`, `SHA256SUMS_NAME`).

### Pro (separate)

Pro builds and the clients-host upload (R2 via Wrangler + `latest.json`) are a separate flow:
see `topicbench-pro/planning/workflows/topicbench-pro-clients-release.md`.
