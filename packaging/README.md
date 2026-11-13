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
| `topicbench-windows-x64.exe` | Windows / mingw release build | **Yes** — replaces this file |
| `TopicBench-windows-x64-portable.zip` | `scripts/build-windows-portable-zip.sh` | Notify only (zip is the human download) |
| `SHA256SUMS` | `sha256sum` over the assets above | Required for apply path |

Arch / Omarchy: AUR [`topicbench-bin`](aur/topicbench-bin) wraps `topicbench-linux-x64` (see [`aur/README.md`](aur/README.md)).

## Updates behavior

- **Check / notify:** every install polls GitHub Releases latest (Settings → Check now, and quiet startup).
- **Apply in place:** when the running binary's directory is writable — typically the bare Linux binary or Windows portable / bare `.exe`.
- **Root-owned paths (e.g. `/usr/bin`):** Linux stages the verified payload in a temp dir, then elevates via `pkexec` (polkit GUI) or `sudo -n` to swap the binary while preserving owner/mode. If elevation is refused or unavailable, the updater opens the release page with a clear error (same fallback as missing assets).
- **AUR:** users still get the update banner; package-managed installs may still prefer `yay`/pacman when elevation is declined, but an elevated in-place swap works when the user authorizes it.

## Build + upload (M17)

```bash
# 1) glibc 2.35 Linux binary (canonical updater asset)
./scripts/build-linux-glibc235.sh

# 2) Attach to the GitHub Release and refresh checksums
cd dist
sha256sum \
  topicbench-linux-x64 \
  topicbench-windows-x64.exe \
  TopicBench-windows-x64-portable.zip \
  > SHA256SUMS
gh release upload "v$(sed -n 's/^version = "\([^"]*\)"/\1/p' ../Cargo.toml | head -1)" \
  topicbench-linux-x64 topicbench-windows-x64.exe \
  TopicBench-windows-x64-portable.zip SHA256SUMS --clobber
```
