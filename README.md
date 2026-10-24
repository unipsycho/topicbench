# TopicBench

Free, open-source MQTT explorer and monitor for builders and testers.

<p align="center">
  <img src="docs/images/topicbench.png" alt="TopicBench main window" width="900">
</p>

## Why TopicBench

- **100% Rust** — built entirely in Rust to handle massive topic trees and topic lists with very efficient memory and CPU use
- **Simultaneous multi-broker** — connect to several brokers at once and monitor or search their topics in a single window
- **Multi-broker profiles** — save hosts, TLS/WSS options, and credentials (passwords in the OS keyring)
- **Live topic tree** — subscribe with wildcards and browse traffic as it arrives
- **Publish & recent** — send messages quickly and keep recent payloads handy
- **Payload inspect** — clear views for JSON and plain text
- **Stickies & graphs** — pin important topics and watch values over time
- **Search** — find topics and payloads without leaving the desktop UI
- **Free & open source** — use it, fork it, ship it
- **Updates via GitHub Releases** — in-app check when releases are published

## Features

- Connection profiles (MQTT TCP / WebSocket, optional TLS)
- Concurrent live sessions across brokers in one window
- Topic subscriptions with `+` / `#`
- Shared sticky board across profiles
- Publish with QoS and retain
- Full keyboard shortcuts & navigation — F1 / `?` help, panel toggles, search focus, and more

## TopicBench Pro

Paid extras for teams that outgrow the free explorer — same app, unlock Pro when you need them.

- **MCP server** — connect, subscribe, publish, and wait from Cursor / CI against live sessions
- **Topic & payload simulation** — generators and formulas for whole payloads or JSON fields
- **Record → simulate** — capture live traffic, then suggest Simulate rows from the tape
- **Scenario player** — sequenced, timed multi-topic publish scripts
- **Combined multi-topic graph** — shared time axis across Graph stickies
- **History scrubber** — scrub per-topic history in the Payload panel
- **Topic bookmarks** — jump list into deep trees
- **Custom color rules** — paint tree rows by topic / payload predicates
- **Topic tree export & import** — JSON hierarchy out, republish onto a target broker
- **Webhooks** — on topic match, POST JSON to n8n / Slack / alert pipelines
- **CSV / Parquet export** — time-series dumps for Excel and Pandas

<p align="center">
  <a href="https://www.agileautomation.ca/products/topic-bench"><strong style="font-size: 1.5em">Learn about TopicBench Pro →</strong></a>
</p>

## Download

**[v1.0.0](https://github.com/unipsycho/topicbench/releases/tag/v1.0.0)** — Linux (`topicbench-linux-x64`) and Windows (`topicbench-windows-x64.exe`), plus `SHA256SUMS`.

All releases: [github.com/unipsycho/topicbench/releases](https://github.com/unipsycho/topicbench/releases)  
Repo: [github.com/unipsycho/topicbench](https://github.com/unipsycho/topicbench)

## Build from source

**Requirements:** Rust stable ≥ 1.80 (`rustup update stable`), plus Linux GUI libs (typically `libxcb`, `libxkbcommon`, and a Vulkan/GL stack).

```bash
git clone https://github.com/unipsycho/topicbench.git
cd topicbench
cargo run --release
```

Profiles: `~/.config/topicbench/profiles.json`  
Settings: `~/.config/topicbench/settings.json`

**Windows:** native `cargo build --release` on Windows works; the Windows icon is embedded via `winres` (MinGW `windres` when cross-compiling from Linux).

## License

MIT OR Apache-2.0 (see `Cargo.toml`).
