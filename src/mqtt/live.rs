//! rumqttc worker threads: connect, stay connected, feed UI via channel.
//!
//! State updates are debounced and reconnects use exponential backoff so a
//! failing broker (auth, offline, etc.) cannot flood the UI hundreds of times/sec.

use crate::mqtt::profile::ConnectionProfile;
use crate::mqtt::session::SessionState;
use eframe::egui::Context;
use rumqttc::{Client, Event, MqttOptions, Packet, QoS, Transport};
use std::sync::mpsc::Sender;
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

const MIN_BACKOFF: Duration = Duration::from_millis(500);
const MAX_BACKOFF: Duration = Duration::from_secs(15);
/// Don't re-emit identical/near-identical reconnect spam faster than this.
const STATUS_MIN_INTERVAL: Duration = Duration::from_millis(400);

/// Build options + spawn event-loop thread; returns a cloneable `Client` for sub/pub.
pub fn spawn(
    profile: &ConnectionProfile,
    event_tx: Sender<BrokerEvent>,
    ctx: Context,
) -> Result<Client, String> {
    let mut opts = MqttOptions::new(&profile.client_id, &profile.host, profile.port);
    opts.set_keep_alive(Duration::from_secs(30));
    opts.set_clean_session(true);
    // Soften rumqttc's own reconnect aggression where supported.
    opts.set_pending_throttle(Duration::from_secs(1));
    if !profile.username.is_empty() {
        opts.set_credentials(&profile.username, &profile.password);
    }
    if profile.use_tls {
        opts.set_transport(Transport::tls_with_default_config());
    }

    let (client, mut connection) = Client::new(opts, 100);
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
                match notification {
                    Ok(Event::Incoming(Packet::ConnAck(ack))) => {
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
                            // Auth/refusal — do not spin; back off before rumqttc retries.
                            emit_state(
                                &tx,
                                &ctx,
                                &profile_id,
                                SessionState::Disconnected,
                                format!("Connection refused: {:?} (retry in {}s)", ack.code, backoff.as_secs().max(1)),
                                &mut last_key,
                                &mut last_emit,
                                true,
                            );
                            thread::sleep(backoff);
                            backoff = (backoff.saturating_mul(2)).min(MAX_BACKOFF);
                        }
                    }
                    Ok(Event::Incoming(Packet::Publish(p))) => {
                        let payload = String::from_utf8_lossy(&p.payload).into_owned();
                        let _ = tx.send(BrokerEvent::Message {
                            profile_id: profile_id.clone(),
                            topic: p.topic,
                            payload,
                        });
                        ctx.request_repaint();
                    }
                    Ok(Event::Incoming(Packet::Disconnect)) => {
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
                        // rumqttc reconnects on the next iter poll — pause first.
                        emit_state(
                            &tx,
                            &ctx,
                            &profile_id,
                            SessionState::Connecting,
                            format!("Reconnecting in {}s… ({e})", backoff.as_secs().max(1)),
                            &mut last_key,
                            &mut last_emit,
                            false, // throttle identical reconnect spam
                        );
                        thread::sleep(backoff);
                        backoff = (backoff.saturating_mul(2)).min(MAX_BACKOFF);
                    }
                }
            }

            emit_state(
                &tx,
                &ctx,
                &profile_id,
                SessionState::Disconnected,
                "Session ended".into(),
                &mut last_key,
                &mut last_emit,
                true,
            );
        })
        .map_err(|e| format!("spawn worker: {e}"))?;

    Ok(client)
}

/// Emit a state event only when it changes, or (for noisy reconnects) at most every STATUS_MIN_INTERVAL.
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
    // Always emit distinct Connected/Disconnected immediately; throttle Connecting churn.
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
