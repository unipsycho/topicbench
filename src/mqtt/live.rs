//! rumqttc worker threads: connect, stay connected, feed UI via channel.
//!
//! Intentional disconnect sets `stop` so the worker exits and does **not**
//! auto-reconnect until the user Connects again.

use crate::mqtt::certs;
use crate::mqtt::profile::ConnectionProfile;
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
#[derive(Debug, Clone)]
pub enum BrokerEvent {
    State {
        profile_id: String,
        state: SessionState,
        detail: String,
    },
    Message {
        profile_id: String,
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
const STATUS_MIN_INTERVAL: Duration = Duration::from_millis(400);

/// Build options + spawn event-loop thread.
///
/// `soft_max_bytes` is the Settings UI limit: payloads larger than this are replaced
/// with a placeholder and the session stays connected. rumqttc’s wire decode limit
/// is always [`WIRE_MAX_PACKET`] so oversize frames do not kill the connection.
pub fn spawn(
    profile: &ConnectionProfile,
    soft_max_bytes: usize,
    event_tx: Sender<BrokerEvent>,
    ctx: Context,
) -> Result<LiveHandle, String> {
    let mut opts = MqttOptions::new(&profile.client_id, &profile.host, profile.port);
    opts.set_keep_alive(Duration::from_secs(30));
    opts.set_clean_session(true);
    opts.set_pending_throttle(Duration::from_secs(1));
    // Wire limit stays high — Settings soft limit is enforced after decode.
    opts.set_max_packet_size(WIRE_MAX_PACKET, WIRE_MAX_PACKET);
    if !profile.username.is_empty() {
        opts.set_credentials(&profile.username, &profile.password);
    }
    // TLS: optional certs folder (CA + client cert + key) or system roots when Use TLS alone.
    let want_tls = profile.use_tls || !profile.certs_dir.trim().is_empty();
    if want_tls {
        match certs::discover_optional(&profile.certs_dir) {
            Ok(Some(resolved)) => {
                let (ca, client_cert, key) = resolved.load_pem().map_err(|e| {
                    format!("TLS certs: {e}")
                })?;
                opts.set_transport(Transport::tls(
                    ca,
                    Some((client_cert, key)),
                    None,
                ));
            }
            Ok(None) => {
                opts.set_transport(Transport::tls_with_default_config());
            }
            Err(e) => {
                return Err(format!("TLS certs: {e}"));
            }
        }
    }

    // Larger request channel — `#` floods + retained bursts shouldn't stall pub/sub requests.
    let (client, mut connection) = Client::new(opts, 1024);
    let stop = Arc::new(AtomicBool::new(false));
    let stop_w = stop.clone();
    let profile_id = profile.id.clone();
    let host = profile.host.clone();
    let port = profile.port;
    let soft_max = soft_max_bytes.max(1);
    let soft_kb = (soft_max / 1024).max(1);
    let tx = event_tx;

    emit_state(
        &tx,
        &ctx,
        &profile_id,
        SessionState::Connecting,
        format!("Connecting to {host}:{port}…"),
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
                                &profile_id,
                                SessionState::Connected,
                                format!("Connected to {host}:{port}"),
                                &mut last_key,
                                &mut last_emit,
                                true,
                            );
                        } else {
                            emit_state(
                                &tx,
                                &ctx,
                                &profile_id,
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
                        let _ = tx.send(BrokerEvent::Message {
                            profile_id: profile_id.clone(),
                            topic: topic.clone(),
                            payload,
                            retain,
                            payload_bytes: raw_len,
                        });
                        if skipped {
                            let size_kb = (raw_len + 1023) / 1024;
                            let _ = tx.send(BrokerEvent::Notice {
                                profile_id: profile_id.clone(),
                                detail: format!(
                                    "Skipped oversized topic {topic} ({size_kb} KB > {soft_kb} KB limit) — still connected"
                                ),
                            });
                        }
                        ctx.request_repaint();
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
                            &profile_id,
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
                            &profile_id,
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
                &profile_id,
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
    profile_id: &str,
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
        state,
        detail,
    });
    ctx.request_repaint();
}

pub fn to_qos(q: crate::mqtt::publish::Qos) -> QoS {
    match q {
        crate::mqtt::publish::Qos::AtMostOnce => QoS::AtMostOnce,
        crate::mqtt::publish::Qos::AtLeastOnce => QoS::AtLeastOnce,
        crate::mqtt::publish::Qos::ExactlyOnce => QoS::ExactlyOnce,
    }
}
