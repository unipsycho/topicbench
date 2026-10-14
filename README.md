# MQTT Master

Dedicated repository for the Rust + egui **MQTT Master** monitor (Explorer-style).

**Clone:** `git clone https://gitlab.com/agileautomation/mqtt-master.git`

Rust desktop app for multi-broker MQTT monitoring: connection profiles, wildcard subscribe, shared sticky board, topic tree, and publish.

GUI stack: **egui / eframe** — same toolkit used by ArtCraft Crafting Apps ([PhotoCraft](https://github.com/storytold/photocraft), DesignCraft, FilmCraft). MQTT client dependency: **rumqttc** (worker-thread `EventLoop` + channels into the UI; not wired yet).

## Requirements

- Rust stable ≥ 1.80 (`rustup update stable`)
- Linux GUI libs for egui (X11 and/or Wayland): typically `libxcb`, `libxkbcommon`, and a Vulkan/GL stack for wgpu

## Run

```bash
git clone https://gitlab.com/agileautomation/mqtt-master.git
cd mqtt-master
cargo run
```

Profiles save to `~/.config/mqtt-monitor/profiles.json`.  
Settings save to `~/.config/mqtt-monitor/settings.json`.

Release build: `cargo build --release`  
Unit tests: `cargo test`.

Builds are **local only** — there is no GitLab CI / pipeline config in this repo.

## Package (optional)

[cargo-packager](https://crates.io/crates/cargo-packager) metadata lives in `Cargo.toml` (`[package.metadata.packager]`).

```bash
cargo install cargo-packager --locked
cargo packager --release
```

## Layout

| Path | Role |
|------|------|
| `src/app.rs` | eframe shell layout |
| `src/mqtt/profile.rs` | Connection profiles |
| `src/mqtt/session.rs` | Multi-profile open/connected sessions |
| `src/mqtt/subscribe.rs` | Topic filters + `+` / `#` validation |
| `src/mqtt/pins.rs` | Shared pin board across profiles |
| `src/mqtt/publish.rs` | Publish draft (QoS / retain) |
| `src/ui/*` | egui panels |

Profiles are editable and persisted; subscriptions pick an open device/profile. Next: rumqttc connect/subscribe/publish per session, and live pin updates.
