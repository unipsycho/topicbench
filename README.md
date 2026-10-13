# MQTT Monitor

Dedicated repository for the Rust + egui MQTT Explorer–style monitor.

**Clone:** `git clone https://gitlab.com/agileautomation/mqtt-master.git`

Rust desktop skeleton for an **MQTT Explorer–style** monitor: connection profiles, multiple open brokers, wildcard subscribe, shared pin board, and publish.

GUI stack: **egui / eframe** — same toolkit used by ArtCraft Crafting Apps ([PhotoCraft](https://github.com/storytold/photocraft), DesignCraft, FilmCraft). MQTT client dependency: **rumqttc** (worker-thread `EventLoop` + channels into the UI; not wired yet).

## Requirements

- Rust stable ≥ 1.80 (`rustup update stable`)
- Linux GUI libs for egui (X11 and/or Wayland): typically `libxcb`, `libxkbcommon`, and a Vulkan/GL stack for wgpu

## Run

```bash
git clone https://gitlab.com/agileautomation/mqtt-master.git
cd mqtt-monitor
cargo run
```

Release build: `cargo build --release`  
Unit tests: `cargo test` (topic-filter validation).

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

v1 is a **working window + placeholders**. Next: persist profiles, rumqttc connect/subscribe/publish per session, and live pin updates.
