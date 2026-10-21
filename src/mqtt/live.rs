//! rumqttc worker threads: connect, stay connected, feed UI via channel.
//!
//! Intentional disconnect sets `stop` so the worker exits and does **not**
//! auto-reconnect until the user Connects again.

use crate::mqtt::certs;
use crate::mqtt::profile::{ConnectionProfile, ProfileTransport};
use crate::mqtt::session::SessionState;
use crate::mqtt::settings::WIRE_MAX_PACKET;
use eframe::egui::Context;
use rumqttc::{Client, Event, MqttOptions, Packet, QoS, Transport};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

/// Events from a session worker into the UI thread.
///
/// `generation` identifies which `connect()` spawned the worker — stale events from a
/// previous reconnect attempt are ignored so they cannot clobber the new session.
#[derive(Debug, Clone)]
pub enum BrokerEvent {
    State {
        profile_id: String,
        generation: u64,
        state: SessionState,
        detail: String,
    },
    Message {
        profile_id: String,
        generation: u64,
        topic: String,
        payload: String,
        /// MQTT RETAIN flag from the Publish packet.
        retain: bool,
        /// Raw publish payload length in bytes (before soft-limit placeholder).
        payload_bytes: usize,
    },
    /// Non-fatal note (e.g. skipped oversized payload) — does not change connection state.
    Notice {
        profile_id: String,
        generation: u64,
        detail: String,
    },
}

/// Handle returned to the UI: client for pub/sub + stop flag for Disconnect.
pub struct LiveHandle {
    pub client: Client,
    pub stop: Arc<AtomicBool>,
}

const MIN_BACKOFF: Duration = Duration::from_millis(500);
const MAX_BACKOFF: Duration = Duration::from_secs(15);

/// MQTT TCP / TLS, or WebSocket / WSS (same certs folder as TLS).
fn apply_transport(profile: &ConnectionProfile, opts: &mut MqttOptions) -> Result<(), String> {
    let want_tls = profile.want_tls();
    match profile.transport {
        ProfileTransport::Mqtt => {
            if !want_tls {
                return Ok(());
            }
            match certs::discover_optional(&profile.certs_dir) {
                Ok(Some(resolved)) => {
                    let (ca, client_cert, key) =
                        resolved.load_pem().map_err(|e| format!("TLS certs: {e}"))?;
                    opts.set_transport(Transport::tls(ca, Some((client_cert, key)), None));
                }
                Ok(None) => {
                    opts.set_transport(Transport::tls_with_default_config());
                }
                Err(e) => return Err(format!("TLS certs: {e}")),
            }
        }
        ProfileTransport::WebSocket => {
            if !want_tls {
                opts.set_transport(Transport::Ws);
                return Ok(());
            }
            match certs::discover_optional(&profile.certs_dir) {
                Ok(Some(resolved)) => {
                    let (ca, client_cert, key) =
                        resolved.load_pem().map_err(|e| format!("TLS certs: {e}"))?;
                    opts.set_transport(Transport::wss(ca, Some((client_cert, key)), None));
                }
                Ok(None) => {
                    opts.set_transport(Transport::wss_with_default_config());
                }
                Err(e) => return Err(format!("TLS certs: {e}")),
            }
        }
    }
    Ok(())
}
const STATUS_MIN_INTERVAL: Duration = Duration::from_millis(400);

/// Build options + spawn event-loop thread.
///
/// `soft_max_bytes` is the Settings UI limit: payloads larger than this are replaced
/// with a placeholder and the session stays connected. rumqttc’s wire decode limit
/// is always [`WIRE_MAX_PACKET`] so oversize frames do not kill the connection.
pub fn spawn(
    profile: &ConnectionProfile,
    soft_max_bytes: usize,
    generation: u64,
    event_tx: Sender<BrokerEvent>,
    ctx: Context,
    ui_focused: Arc<AtomicBool>,
) -> Result<LiveHandle, String> {
    // WebSocket: broker_addr is a full ws(s):// URL; rumqttc ignores the port arg then.
    let mut opts = MqttOptions::new(&profile.client_id, &profile.broker_addr(), profile.port);
    opts.set_keep_alive(Duration::from_secs(30));
    opts.set_clean_session(true);
    opts.set_pending_throttle(Duration::from_secs(1));
    // Wire limit stays high — Settings soft limit is enforced after decode.
    opts.set_max_packet_size(WIRE_MAX_PACKET, WIRE_MAX_PACKET);
    if !profile.username.is_empty() {
        opts.set_credentials(&profile.username, &profile.password);
    }
    apply_transport(profile, &mut opts)?;

    // Larger request channel — `#` floods + retained bursts shouldn't stall pub/sub requests.
    let (client, mut connection) = Client::new(opts, 1024);
    let stop = Arc::new(AtomicBool::new(false));
    let stop_w = stop.clone();
    let profile_id = profile.id.clone();
    let endpoint = profile.endpoint_label();
    let soft_max = soft_max_bytes.max(1);
    let soft_kb = (soft_max / 1024).max(1);
    let tx = event_tx;

    emit_state(
        &tx,
        &ctx,
        &ui_focused,
        &profile_id,
        generation,
        SessionState::Connecting,
        format!("Connecting to {endpoint}…"),
        &mut None,
        &mut Instant::now()
            .checked_sub(STATUS_MIN_INTERVAL)
            .unwrap_or_else(Instant::now),
        true,
    );

    thread::Builder::new()
        .name(format!("mqtt-{profile_id}"))
        .spawn(move || {
            let mut backoff = MIN_BACKOFF;
            let mut last_key: Option<(SessionState, String)> = None;
            let mut last_emit = Instant::now()
                .checked_sub(STATUS_MIN_INTERVAL)
                .unwrap_or_else(Instant::now);
            // Keep the real failure reason — generic "Session ended" must not overwrite it.
            let mut fatal_detail: Option<String> = None;

            for notification in connection.iter() {
                // User Disconnect — leave the loop; do not reconnect.
                if stop_w.load(Ordering::SeqCst) {
                    break;
                }

                match notification {
                    Ok(Event::Incoming(Packet::ConnAck(ack))) => {
                        if stop_w.load(Ordering::SeqCst) {
                            break;
                        }
                        if ack.code == rumqttc::ConnectReturnCode::Success {
                            backoff = MIN_BACKOFF;
                            emit_state(
                                &tx,
                                &ctx,
                                &ui_focused,
                                &profile_id,
                                generation,
                                SessionState::Connected,
                                format!("Connected to {endpoint}"),
                                &mut last_key,
                                &mut last_emit,
                                true,
                            );
                        } else {
                            emit_state(
                                &tx,
                                &ctx,
                                &ui_focused,
                                &profile_id,
                                generation,
                                SessionState::Disconnected,
                                format!(
                                    "Connection refused: {:?} (retry in {}s)",
                                    ack.code,
                                    backoff.as_secs().max(1)
                                ),
                                &mut last_key,
                                &mut last_emit,
                                true,
                            );
                            if stop_w.load(Ordering::SeqCst) {
                                break;
                            }
                            thread::sleep(backoff);
                            backoff = (backoff.saturating_mul(2)).min(MAX_BACKOFF);
                        }
                    }
                    Ok(Event::Incoming(Packet::Publish(p))) => {
                        if stop_w.load(Ordering::SeqCst) {
                            break;
                        }
                        let retain = p.retain;
                        let raw_len = p.payload.len();
                        let topic = p.topic;
                        // Soft Settings limit: keep the session; show a placeholder in the tree.
                        let (payload, skipped) = if raw_len > soft_max {
                            let size_kb = (raw_len + 1023) / 1024;
                            let placeholder = format!(
                                "[skipped oversized payload: {size_kb} KB > {soft_kb} KB limit]"
                            );
                            (placeholder, true)
                        } else {
                            (String::from_utf8_lossy(&p.payload).into_owned(), false)
                        };
                        // Inbox applier coalesces + wakes egui only while focused.
                        // Never request_repaint here — present-while-hidden misses Hyprland ANR pongs.
                        let _ = tx.send(BrokerEvent::Message {
                            profile_id: profile_id.clone(),
                            generation,
                            topic: topic.clone(),
                            payload,
                            retain,
                            payload_bytes: raw_len,
                        });
                        if skipped {
                            let size_kb = (raw_len + 1023) / 1024;
                            let _ = tx.send(BrokerEvent::Notice {
                                profile_id: profile_id.clone(),
                                generation,
                                detail: format!(
                                    "Skipped oversized topic {topic} ({size_kb} KB > {soft_kb} KB limit) — still connected"
                                ),
                            });
                        }
                    }
                    Ok(Event::Incoming(Packet::Disconnect)) => {
                        if stop_w.load(Ordering::SeqCst) {
                            break;
                        }
                        let detail = "Broker sent disconnect".to_string();
                        fatal_detail = Some(detail.clone());
                        emit_state(
                            &tx,
                            &ctx,
                            &ui_focused,
                            &profile_id,
                            generation,
                            SessionState::Disconnected,
                            detail,
                            &mut last_key,
                            &mut last_emit,
                            true,
                        );
                    }
                    Ok(_) => {}
                    Err(e) => {
                        if stop_w.load(Ordering::SeqCst) {
                            break;
                        }
                        let err = e.to_string();
                        // Even wire-limit / decode errors: reconnect, do not treat as fatal stop.
                        // Soft oversize is handled above; this path is rare (e.g. > WIRE_MAX_PACKET).
                        emit_state(
                            &tx,
                            &ctx,
                            &ui_focused,
                            &profile_id,
                            generation,
                            SessionState::Connecting,
                            format!("Reconnecting in {}s… ({err})", backoff.as_secs().max(1)),
                            &mut last_key,
                            &mut last_emit,
                            false,
                        );
                        if stop_w.load(Ordering::SeqCst) {
                            break;
                        }
                        thread::sleep(backoff);
                        backoff = (backoff.saturating_mul(2)).min(MAX_BACKOFF);
                    }
                }
            }

            // Final stable status: intentional stop vs last fatal reason vs unexpected end.
            let detail = if stop_w.load(Ordering::SeqCst) {
                "Disconnected".to_string()
            } else if let Some(fatal) = fatal_detail {
                fatal
            } else if let Some((_, last_detail)) = last_key.clone() {
                // Keep the last meaningful status (e.g. reconnect error) instead of masking it.
                if last_detail.starts_with("Connected") {
                    "Session ended (connection closed)".to_string()
                } else {
                    last_detail
                }
            } else {
                "Session ended (connection closed)".to_string()
            };
            emit_state(
                &tx,
                &ctx,
                &ui_focused,
                &profile_id,
                generation,
                SessionState::Disconnected,
                detail,
                &mut last_key,
                &mut last_emit,
                true,
            );
        })
        .map_err(|e| format!("spawn worker: {e}"))?;

    Ok(LiveHandle { client, stop })
}

#[allow(clippy::too_many_arguments)]
fn emit_state(
    tx: &Sender<BrokerEvent>,
    ctx: &Context,
    ui_focused: &AtomicBool,
    profile_id: &str,
    generation: u64,
    state: SessionState,
    detail: String,
    last_key: &mut Option<(SessionState, String)>,
    last_emit: &mut Instant,
    force_if_changed: bool,
) {
    let key = (state, detail.clone());
    let changed = last_key.as_ref() != Some(&key);
    if !changed {
        return;
    }
    let allow = force_if_changed
        || state != SessionState::Connecting
        || last_emit.elapsed() >= STATUS_MIN_INTERVAL;
    if !allow {
        return;
    }
    *last_key = Some(key);
    *last_emit = Instant::now();
    let _ = tx.send(BrokerEvent::State {
        profile_id: profile_id.to_string(),
        generation,
        state,
        detail,
    });
    // Inbox applier also wakes while focused; skip present storms when unfocused.
    if ui_focused.load(Ordering::Relaxed) {
        ctx.request_repaint();
    }
}

pub fn to_qos(q: crate::mqtt::publish::Qos) -> QoS {
    match q {
        crate::mqtt::publish::Qos::AtMostOnce => QoS::AtMostOnce,
        crate::mqtt::publish::Qos::AtLeastOnce => QoS::AtLeastOnce,
        crate::mqtt::publish::Qos::ExactlyOnce => QoS::ExactlyOnce,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mqtt::profile::ConnectionProfile;
    use crate::mqtt::settings::MAX_PACKET_DEFAULT;
    use std::sync::mpsc::{self, Receiver};
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

    /// Public Mosquitto sandbox — anonymous MQTT on 1883.
    const MOSQUITTO_HOST: &str = "test.mosquitto.org";
    const MOSQUITTO_PORT: u16 = 1883;

    fn unique_id(prefix: &str) -> String {
        let ns = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        format!("{prefix}-{ns}")
    }

    fn mosquitto_profile(id: &str) -> ConnectionProfile {
        let mut p = ConnectionProfile::new_blank(id.to_string());
        p.name = "Mosquitto test".into();
        p.host = MOSQUITTO_HOST.into();
        p.port = MOSQUITTO_PORT;
        p.client_id = unique_id("mqtt-master-live");
        p.subscriptions = vec!["#".into()];
        p
    }

    /// Drain until Connected (or timeout).
    fn wait_state(
        rx: &Receiver<BrokerEvent>,
        profile_id: &str,
        want: SessionState,
        timeout: Duration,
    ) -> bool {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            let remain = deadline.saturating_duration_since(Instant::now());
            match rx.recv_timeout(remain.min(Duration::from_millis(200))) {
                Ok(BrokerEvent::State {
                    profile_id: id,
                    state,
                    ..
                }) if id == profile_id && state == want => return true,
                Ok(_) => {}
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => return false,
            }
        }
        false
    }

    #[test]
    fn spawn_connects_to_mosquitto_and_stop_disconnects() {
        let profile = mosquitto_profile("live-mosq");
        let (tx, rx) = mpsc::channel();
        let ctx = Context::default();
        let ui_focused = Arc::new(AtomicBool::new(false));

        let handle = spawn(
            &profile,
            MAX_PACKET_DEFAULT,
            1,
            tx,
            ctx,
            ui_focused,
        )
        .expect("spawn worker");

        assert!(
            wait_state(&rx, &profile.id, SessionState::Connected, Duration::from_secs(20)),
            "expected Connected from {MOSQUITTO_HOST}"
        );

        // Intentional stop — worker must emit Disconnected and not keep reconnecting.
        handle.stop.store(true, Ordering::SeqCst);
        let _ = handle.client.disconnect();

        assert!(
            wait_state(
                &rx,
                &profile.id,
                SessionState::Disconnected,
                Duration::from_secs(10)
            ),
            "expected Disconnected after stop"
        );

        // No further Connected chatter after intentional stop.
        let quiet_until = Instant::now() + Duration::from_secs(2);
        while Instant::now() < quiet_until {
            match rx.recv_timeout(Duration::from_millis(100)) {
                Ok(BrokerEvent::State {
                    state: SessionState::Connected,
                    ..
                }) => panic!("worker reconnected after intentional stop"),
                Ok(_) | Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
    }

    #[test]
    fn spawn_receives_messages_on_root_wildcard() {
        let profile = mosquitto_profile("live-hash");
        let (tx, rx) = mpsc::channel();
        let ctx = Context::default();
        let ui_focused = Arc::new(AtomicBool::new(false));

        let handle = spawn(
            &profile,
            MAX_PACKET_DEFAULT,
            1,
            tx,
            ctx,
            ui_focused,
        )
        .expect("spawn worker");

        assert!(
            wait_state(&rx, &profile.id, SessionState::Connected, Duration::from_secs(20)),
            "expected Connected"
        );

        // Subscribe at root `#` — public broker traffic (or our own publish) should pile in.
        handle
            .client
            .subscribe("#", QoS::AtMostOnce)
            .expect("subscribe #");

        let topic = format!("mqtt-master/test/{}", unique_id("ping"));
        let payload = format!("hello-{}", unique_id("p"));
        handle
            .client
            .publish(&topic, QoS::AtLeastOnce, false, payload.as_bytes())
            .expect("publish");

        let deadline = Instant::now() + Duration::from_secs(15);
        let mut saw_own = false;
        let mut any_msg = 0u32;
        while Instant::now() < deadline {
            match rx.recv_timeout(Duration::from_millis(200)) {
                Ok(BrokerEvent::Message {
                    profile_id: id,
                    topic: t,
                    payload: p,
                    ..
                }) if id == profile.id => {
                    any_msg += 1;
                    if t == topic && p == payload {
                        saw_own = true;
                        break;
                    }
                }
                Ok(_) | Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }

        handle.stop.store(true, Ordering::SeqCst);
        let _ = handle.client.disconnect();

        assert!(
            saw_own || any_msg > 0,
            "expected own roundtrip or `#` traffic from {MOSQUITTO_HOST} (saw_own={saw_own}, any={any_msg})"
        );
    }
}
