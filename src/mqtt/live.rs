//! rumqttc worker threads: connect, stay connected, feed UI via channel.
//!
//! Intentional disconnect sets `stop` so the worker exits and does **not**
//! auto-reconnect until the user Connects again.

use crate::mqtt::profile::ConnectionProfile;
use crate::mqtt::session::SessionState;
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
pub fn spawn(
    profile: &ConnectionProfile,
    event_tx: Sender<BrokerEvent>,
    ctx: Context,
) -> Result<LiveHandle, String> {
    let mut opts = MqttOptions::new(&profile.client_id, &profile.host, profile.port);
    opts.set_keep_alive(Duration::from_secs(30));
    opts.set_clean_session(true);
    opts.set_pending_throttle(Duration::from_secs(1));
    if !profile.username.is_empty() {
        opts.set_credentials(&profile.username, &profile.password);
    }
    if profile.use_tls {
        opts.set_transport(Transport::tls_with_default_config());
    }

    let (client, mut connection) = Client::new(opts, 100);
    let stop = Arc::new(AtomicBool::new(false));
    let stop_w = stop.clone();
    let profile_id = profile.id.clone();
    let host = profile.host.clone();
    let port = profile.port;
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
                        let payload = String::from_utf8_lossy(&p.payload).into_owned();
                        let _ = tx.send(BrokerEvent::Message {
                            profile_id: profile_id.clone(),
                            topic: p.topic,
                            payload,
                        });
                        ctx.request_repaint();
                    }
                    Ok(Event::Incoming(Packet::Disconnect)) => {
                        if stop_w.load(Ordering::SeqCst) {
                            break;
                        }
                        emit_state(
                            &tx,
                            &ctx,
                            &profile_id,
                            SessionState::Disconnected,
                            "Broker sent disconnect".into(),
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
                        emit_state(
                            &tx,
                            &ctx,
                            &profile_id,
                            SessionState::Connecting,
                            format!("Reconnecting in {}s… ({e})", backoff.as_secs().max(1)),
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

            // Final stable status: intentional stop vs unexpected end
            let detail = if stop_w.load(Ordering::SeqCst) {
                "Disconnected".to_string()
            } else {
                "Session ended".to_string()
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
