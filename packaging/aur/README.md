# AUR packaging (OSS only)

Arch / Omarchy package for **TopicBench** free OSS — never TopicBench Pro.

## Package

| AUR name | Style | Notes |
|----------|--------|--------|
| **`topicbench-bin`** | Prebuilt binary from GitHub Releases | Preferred. Installs `topicbench-linux-x64` → `/usr/bin/topicbench`, plus `.desktop` + icon. |

**Why `-bin`, not source?** Pacman users expect a normal ELF under `/usr/bin` with desktop integration and system GUI deps (`libxcb`, `libxkbcommon`, Wayland, Vulkan/GL). The Release ships `topicbench-linux-x64` for download and the in-app updater; `-bin` wraps that same asset. A source package (`topicbench`) is fine later; `-bin` ships today without a Rust toolchain on the user machine. Name `topicbench` is reserved via `provides`/`conflicts` for a future source PKGBUILD.

No AUR name collision as of package prep (RPC search empty for `topicbench`, `topicbench-bin`, `topicbench-git`).

## Install (after AUR publish)

```bash
yay -S topicbench-bin
# or: paru -S topicbench-bin
```

## Maintain: bump for a new release

1. Edit `topicbench-bin/PKGBUILD`: set `pkgver`, reset `pkgrel=1`, update the binary `sha256sums[0]` from the Release `SHA256SUMS` line for `topicbench-linux-x64`.
2. From `topicbench-bin/`:

   ```bash
   makepkg -g >> /dev/null   # or recompute local file sums if desktop/icon/licenses changed
   makepkg --printsrcinfo > .SRCINFO
   makepkg -si                 # local install smoke test
   namcap PKGBUILD *.pkg.tar.zst
   ```

3. Sync the same files to the AUR git repo (see below) and `git push`.

In-tree path mirrors the AUR package tree so GitHub stays the source of truth for packaging files; AUR is a publish target.

## Publish to AUR (Mike)

Requires an [AUR account](https://aur.archlinux.org/) and SSH key registered at https://aur.archlinux.org/account/ (SSH Public Key).

### One-time SSH on M17

```bash
# Generate a key used only for AUR (ed25519), or reuse an existing one:
ssh-keygen -t ed25519 -f ~/.ssh/id_ed25519_aur -C 'aur-topicbench'

# Add the .pub contents on https://aur.archlinux.org/account/
# Then ~/.ssh/config:
Host aur.archlinux.org
  User aur
  IdentityFile ~/.ssh/id_ed25519_aur
  IdentitiesOnly yes

ssh aur@aur.archlinux.org   # should greet with your AUR username
```

### First publish

```bash
git clone ssh://aur@aur.archlinux.org/topicbench-bin.git
cd topicbench-bin
cp -a /path/to/topicbench/packaging/aur/topicbench-bin/{PKGBUILD,.SRCINFO,topicbench.desktop,topicbench.png,LICENSE-MIT,LICENSE-APACHE} .
# git add / commit / push — AUR rejects empty first commits; include PKGBUILD + .SRCINFO

git add PKGBUILD .SRCINFO topicbench.desktop topicbench.png LICENSE-MIT LICENSE-APACHE
git commit -m "topicbench-bin 1.2.1-1"
git push
```

Package URL after push: https://aur.archlinux.org/packages/topicbench-bin

### Later version bumps

```bash
cd topicbench-bin   # AUR clone
# copy updated files from OSS packaging/aur/topicbench-bin/
git add -A
git commit -m "topicbench-bin <pkgver>-<pkgrel>"
git push
```

Optional helper: `aur publish` from [aurutils](https://aur.archlinux.org/packages/aurutils) if installed (`aur publish` from the package directory after commit).
