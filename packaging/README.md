# Release packaging (OSS)

Scripts under `scripts/` build GitHub Release assets from one Linux x64 binary
(prefer `dist/topicbench-linux-x64` from `scripts/build-linux-glibc235.sh`).

## Assets to attach each release

| Asset | Script / source | In-app updater |
| --- | --- | --- |
| `topicbench-linux-x64` | `scripts/build-linux-glibc235.sh` | **Yes** — replaces this file |
| `topicbench-windows-x64.exe` | Windows / mingw release build | **Yes** — replaces this file |
| `TopicBench-windows-x64-portable.zip` | `scripts/build-windows-portable-zip.sh` | Notify only (zip is the human download) |
| `topicbench_<ver>_amd64.deb` | existing Debian package build | Notify only — reinstall `.deb` |
| `topicbench-<ver>-1.x86_64.rpm` | `scripts/build-rpm.sh` (nfpm) | Notify only — reinstall `.rpm` |
| `TopicBench-x86_64.AppImage` | `scripts/build-appimage.sh` | Notify only — replace AppImage manually |
| `SHA256SUMS` | `sha256sum` over the assets above | Required for apply path |

Arch / Omarchy: AUR [`topicbench-bin`](aur/topicbench-bin) wraps `topicbench-linux-x64` (see [`aur/README.md`](aur/README.md)).

## Updates behavior

- **Check / notify:** every install polls GitHub Releases latest (Settings → Check now, and quiet startup).
- **Apply in place:** only when the running binary path is user-writable — typically the bare Linux binary or Windows portable / bare `.exe`.
- **`.deb` / `.rpm` / AUR / AppImage:** users still get the update banner; install usually falls back to the release page so they can fetch the new package.

## Build AppImage + RPM (M17)

```bash
# 1) glibc 2.35 Linux binary (canonical updater asset)
./scripts/build-linux-glibc235.sh

# 2) AppImage (needs appimagetool on PATH)
./scripts/build-appimage.sh

# 3) RPM (needs nfpm on PATH)
./scripts/build-rpm.sh

# 4) Attach to the GitHub Release and refresh checksums
cd dist
sha256sum \
  topicbench-linux-x64 \
  topicbench-windows-x64.exe \
  TopicBench-windows-x64-portable.zip \
  topicbench_*_amd64.deb \
  topicbench-*-1.x86_64.rpm \
  TopicBench-x86_64.AppImage \
  > SHA256SUMS
gh release upload "v$(sed -n 's/^version = "\([^"]*\)"/\1/p' ../Cargo.toml | head -1)" \
  topicbench-*-1.x86_64.rpm TopicBench-x86_64.AppImage SHA256SUMS --clobber
```
