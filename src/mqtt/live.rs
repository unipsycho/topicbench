//! rumqttc worker threads: connect, stay connected, feed UI via channel.
//!
//! Intentional disconnect sets `stop` so the worker exits and does **not**
//! auto-reconnect until the user Connects again.
//!
//! MQTT 3.1.1 and MQTT 5 use separate rumqttc client types; [`LiveClient`] wraps both.

use crate::mqtt::certs;
use crate::mqtt::profile::{ConnectionProfile, MqttVersion, ProfileTransport};
use crate::mqtt::publish::{MqttMsgProps, Qos as AppQos};
use crate::mqtt::session::SessionState;
use crate::mqtt::settings::WIRE_MAX_PACKET;
use eframe::egui::Context;
use rumqttc::{Client as ClientV311, Event as EventV311, MqttOptions as OptsV311, Packet as PacketV311, Transport};
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
        /// MQTT 5 properties (empty for 3.1.1 / absent props).
        props: MqttMsgProps,
    },
    /// Non-fatal note (e.g. skipped oversized payload) — does not change connection state.
    Notice {
        profile_id: String,
        generation: u64,
        detail: String,
    },
}

/// Protocol-specific rumqttc client handle.
pub enum LiveClient {
    V311(ClientV311),
    V5(rumqttc::v5::Client),
}

impl LiveClient {
    pub fn disconnect(&self) {
        match self {
            Self::V311(c) => {
                let _ = c.disconnect();
            }
            Self::V5(c) => {
                let _ = c.disconnect();
            }
        }
    }

    pub fn subscribe(&self, filter: &str, qos: AppQos) -> Result<(), String> {
        match self {
            Self::V311(c) => c
                .subscribe(filter, to_qos_v311(qos))
                .map_err(|e| format!("subscribe: {e}")),
            Self::V5(c) => c
                .subscribe(filter, to_qos_v5(qos))
                .map_err(|e| format!("subscribe: {e}")),
        }
    }

    pub fn unsubscribe(&self, filter: &str) -> Result<(), String> {
        match self {
            Self::V311(c) => c
                .unsubscribe(filter)
                .map_err(|e| format!("unsubscribe: {e}")),
            Self::V5(c) => c
                .unsubscribe(filter)
                .map_err(|e| format!("unsubscribe: {e}")),
        }
    }

    pub fn publish(
        &self,
        topic: &str,
        qos: AppQos,
        retain: bool,
        payload: &[u8],
        props: Option<&MqttMsgProps>,
    ) -> Result<(), String> {
        match self {
            Self::V311(c) => c
                .publish(topic, to_qos_v311(qos), retain, payload)
                .map_err(|e| format!("publish: {e}")),
            Self::V5(c) => {
                // rumqttc v5 Publish payload requires owned Bytes ('static).
                let owned = payload.to_vec();
                let cleaned = props.map(|p| p.cleaned()).filter(|p| !p.is_empty());
                if let Some(p) = cleaned {
                    let mut pp = rumqttc::v5::mqttbytes::v5::PublishProperties::default();
                    pp.user_properties = p.user_properties;
                    pp.content_type = p.content_type;
                    pp.message_expiry_interval = p.message_expiry_interval;
                    pp.payload_format_indicator = p.payload_format_indicator;
                    c.publish_with_properties(topic, to_qos_v5(qos), retain, owned, pp)
                        .map_err(|e| format!("publish: {e}"))
                } else {
                    c.publish(topic, to_qos_v5(qos), retain, owned)
                        .map_err(|e| format!("publish: {e}"))
                }
            }
        }
    }

    pub fn is_v5(&self) -> bool {
        matches!(self, Self::V5(_))
    }
}

/// Handle returned to the UI: client for pub/sub + stop flag for Disconnect.
pub struct LiveHandle {
    pub client: LiveClient,
    pub stop: Arc<AtomicBool>,
}

const MIN_BACKOFF: Duration = Duration::from_millis(500);
const MAX_BACKOFF: Duration = Duration::from_secs(15);
const STATUS_MIN_INTERVAL: Duration = Duration::from_millis(400);

/// Build TLS / WS / WSS transport shared by v3 and v5 options.
fn build_transport(profile: &ConnectionProfile) -> Result<Transport, String> {
    let want_tls = profile.want_tls();
    match profile.transport {
        ProfileTransport::Mqtt => {
            if !want_tls {
                return Ok(Transport::Tcp);
            }
            match certs::discover_optional(&profile.certs_dir) {
                Ok(Some(resolved)) => {
                    let (ca, client_cert, key) =
                        resolved.load_pem().map_err(|e| format!("TLS certs: {e}"))?;
                    Ok(Transport::tls(ca, Some((client_cert, key)), None))
                }
                Ok(None) => Ok(Transport::tls_with_default_config()),
                Err(e) => Err(format!("TLS certs: {e}")),
            }
        }
        ProfileTransport::WebSocket => {
            if !want_tls {
                return Ok(Transport::Ws);
            }
            match certs::discover_optional(&profile.certs_dir) {
                Ok(Some(resolved)) => {
                    let (ca, client_cert, key) =
                        resolved.load_pem().map_err(|e| format!("TLS certs: {e}"))?;
                    Ok(Transport::wss(ca, Some((client_cert, key)), None))
                }
                Ok(None) => Ok(Transport::wss_with_default_config()),
                Err(e) => Err(format!("TLS certs: {e}")),
            }
        }
    }
}

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
    match profile.mqtt_version {
        MqttVersion::V311 => spawn_v311(profile, soft_max_bytes, generation, event_tx, ctx, ui_focused),
        MqttVersion::V5 => spawn_v5(profile, soft_max_bytes, generation, event_tx, ctx, ui_focused),
    }
}

fn spawn_v311(
    profile: &ConnectionProfile,
    soft_max_bytes: usize,
    generation: u64,
    event_tx: Sender<BrokerEvent>,
    ctx: Context,
    ui_focused: Arc<AtomicBool>,
) -> Result<LiveHandle, String> {
    let client_id = profile.connect_client_id();
    let mut opts = OptsV311::new(&client_id, &profile.broker_addr(), profile.port);
    opts.set_keep_alive(Duration::from_secs(30));
    opts.set_clean_session(true);
    opts.set_pending_throttle(Duration::from_secs(1));
    opts.set_max_packet_size(WIRE_MAX_PACKET, WIRE_MAX_PACKET);
    if !profile.username.is_empty() {
        opts.set_credentials(&profile.username, &profile.password);
    }
    opts.set_transport(build_transport(profile)?);

    let (client, mut connection) = ClientV311::new(opts, 1024);
    let stop = Arc::new(AtomicBool::new(false));
    let stop_w = stop.clone();
    let profile_id = profile.id.clone();
    let endpoint = profile.endpoint_label();
    let soft_max = soft_max_bytes.max(1);
    let soft_kb = (soft_max / 1024).max(1);
    let tx = event_tx;

    emit_connecting(&tx, &ctx, &ui_focused, &profile_id, generation, &endpoint);

    thread::Builder::new()
        .name(format!("mqtt-{profile_id}"))
        .spawn(move || {
            let mut backoff = MIN_BACKOFF;
            let mut last_key: Option<(SessionState, String)> = None;
            let mut last_emit = Instant::now()
                .checked_sub(STATUS_MIN_INTERVAL)
                .unwrap_or_else(Instant::now);
            let mut fatal_detail: Option<String> = None;

            for notification in connection.iter() {
                if stop_w.load(Ordering::SeqCst) {
                    break;
                }

                match notification {
                    Ok(EventV311::Incoming(PacketV311::ConnAck(ack))) => {
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
                                format!("Connected to {endpoint} (MQTT 3.1.1)"),
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
                    Ok(EventV311::Incoming(PacketV311::Publish(p))) => {
                        if stop_w.load(Ordering::SeqCst) {
                            break;
                        }
                        handle_publish_bytes(
                            &tx,
                            &profile_id,
                            generation,
                            p.topic,
                            p.payload.as_ref(),
                            p.retain,
                            soft_max,
                            soft_kb,
                            MqttMsgProps::default(),
                        );
                    }
                    Ok(EventV311::Incoming(PacketV311::Disconnect)) => {
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
                        reconnect_backoff(
                            &tx,
                            &ctx,
                            &ui_focused,
                            &profile_id,
                            generation,
                            &e.to_string(),
                            &mut backoff,
                            &mut last_key,
                            &mut last_emit,
                            &stop_w,
                        );
                    }
                }
            }

            emit_final(
                &tx,
                &ctx,
                &ui_focused,
                &profile_id,
                generation,
                &stop_w,
                fatal_detail,
                &mut last_key,
                &mut last_emit,
            );
        })
        .map_err(|e| format!("spawn worker: {e}"))?;

    Ok(LiveHandle {
        client: LiveClient::V311(client),
        stop,
    })
}

fn spawn_v5(
    profile: &ConnectionProfile,
    soft_max_bytes: usize,
    generation: u64,
    event_tx: Sender<BrokerEvent>,
    ctx: Context,
    ui_focused: Arc<AtomicBool>,
) -> Result<LiveHandle, String> {
    use rumqttc::v5::mqttbytes::v5::{ConnectProperties, Packet as PacketV5};
    use rumqttc::v5::{Client as ClientV5, Event as EventV5, MqttOptions as OptsV5};

    let client_id = profile.connect_client_id();
    let mut opts = OptsV5::new(&client_id, &profile.broker_addr(), profile.port);
    opts.set_keep_alive(Duration::from_secs(30));
    opts.set_clean_start(true);
    opts.set_pending_throttle(Duration::from_secs(1));
    if !profile.username.is_empty() {
        opts.set_credentials(&profile.username, &profile.password);
    }
    opts.set_transport(build_transport(profile)?);

    // Wire soft-limit via CONNECT max_packet_size (also raises decode ceiling).
    let mut conn_props = ConnectProperties::new();
    conn_props.max_packet_size = Some(WIRE_MAX_PACKET as u32);
    conn_props.session_expiry_interval = Some(profile.session_expiry_secs);
    // Ask broker for reason strings on failures.
    conn_props.request_problem_info = Some(1);
    opts.set_connect_properties(conn_props);

    let (client, mut connection) = ClientV5::new(opts, 1024);
    let stop = Arc::new(AtomicBool::new(false));
    let stop_w = stop.clone();
    let profile_id = profile.id.clone();
    let endpoint = profile.endpoint_label();
    let soft_max = soft_max_bytes.max(1);
    let soft_kb = (soft_max / 1024).max(1);
    let tx = event_tx;

    emit_connecting(&tx, &ctx, &ui_focused, &profile_id, generation, &endpoint);

    thread::Builder::new()
        .name(format!("mqtt5-{profile_id}"))
        .spawn(move || {
            let mut backoff = MIN_BACKOFF;
            let mut last_key: Option<(SessionState, String)> = None;
            let mut last_emit = Instant::now()
                .checked_sub(STATUS_MIN_INTERVAL)
                .unwrap_or_else(Instant::now);
            let mut fatal_detail: Option<String> = None;

            for notification in connection.iter() {
                if stop_w.load(Ordering::SeqCst) {
                    break;
                }

                match notification {
                    Ok(EventV5::Incoming(PacketV5::ConnAck(ack))) => {
                        if stop_w.load(Ordering::SeqCst) {
                            break;
                        }
                        if ack.code == rumqttc::v5::mqttbytes::v5::ConnectReturnCode::Success {
                            backoff = MIN_BACKOFF;
                            emit_state(
                                &tx,
                                &ctx,
                                &ui_focused,
                                &profile_id,
                                generation,
                                SessionState::Connected,
                                format!("Connected to {endpoint} (MQTT 5)"),
                                &mut last_key,
                                &mut last_emit,
                                true,
                            );
                        } else {
                            let reason = ack
                                .properties
                                .as_ref()
                                .and_then(|p| p.reason_string.clone())
                                .unwrap_or_default();
                            let detail = if reason.is_empty() {
                                format!(
                                    "Connection refused: {:?} (retry in {}s)",
                                    ack.code,
                                    backoff.as_secs().max(1)
                                )
                            } else {
                                format!(
                                    "Connection refused: {:?} — {reason} (retry in {}s)",
                                    ack.code,
                                    backoff.as_secs().max(1)
                                )
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
                            if stop_w.load(Ordering::SeqCst) {
                                break;
                            }
                            thread::sleep(backoff);
                            backoff = (backoff.saturating_mul(2)).min(MAX_BACKOFF);
                        }
                    }
                    Ok(EventV5::Incoming(PacketV5::Publish(p))) => {
                        if stop_w.load(Ordering::SeqCst) {
                            break;
                        }
                        let topic = String::from_utf8_lossy(&p.topic).into_owned();
                        let props = props_from_v5(p.properties.as_ref());
                        handle_publish_bytes(
                            &tx,
                            &profile_id,
                            generation,
                            topic,
                            p.payload.as_ref(),
                            p.retain,
                            soft_max,
                            soft_kb,
                            props,
                        );
                    }
                    Ok(EventV5::Incoming(PacketV5::Disconnect(d))) => {
                        if stop_w.load(Ordering::SeqCst) {
                            break;
                        }
                        let reason = d
                            .properties
                            .as_ref()
                            .and_then(|p| p.reason_string.clone())
                            .unwrap_or_default();
                        let detail = if reason.is_empty() {
                            format!("Broker disconnect: {:?}", d.reason_code)
                        } else {
                            format!("Broker disconnect: {:?} — {reason}", d.reason_code)
                        };
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
                        reconnect_backoff(
                            &tx,
                            &ctx,
                            &ui_focused,
                            &profile_id,
                            generation,
                            &e.to_string(),
                            &mut backoff,
                            &mut last_key,
                            &mut last_emit,
                            &stop_w,
                        );
                    }
                }
            }

            emit_final(
                &tx,
                &ctx,
                &ui_focused,
                &profile_id,
                generation,
                &stop_w,
                fatal_detail,
                &mut last_key,
                &mut last_emit,
            );
        })
        .map_err(|e| format!("spawn worker: {e}"))?;

    Ok(LiveHandle {
        client: LiveClient::V5(client),
        stop,
    })
}

fn props_from_v5(
    p: Option<&rumqttc::v5::mqttbytes::v5::PublishProperties>,
) -> MqttMsgProps {
    let Some(p) = p else {
        return MqttMsgProps::default();
    };
    MqttMsgProps {
        user_properties: p.user_properties.clone(),
        content_type: p.content_type.clone(),
        message_expiry_interval: p.message_expiry_interval,
        payload_format_indicator: p.payload_format_indicator,
    }
}

fn handle_publish_bytes(
    tx: &Sender<BrokerEvent>,
    profile_id: &str,
    generation: u64,
    topic: String,
    payload_bytes: &[u8],
    retain: bool,
    soft_max: usize,
    soft_kb: usize,
    props: MqttMsgProps,
) {
    let raw_len = payload_bytes.len();
    let (payload, skipped) = if raw_len > soft_max {
        let size_kb = (raw_len + 1023) / 1024;
        let placeholder =
            format!("[skipped oversized payload: {size_kb} KB > {soft_kb} KB limit]");
        (placeholder, true)
    } else {
        (String::from_utf8_lossy(payload_bytes).into_owned(), false)
    };
    let _ = tx.send(BrokerEvent::Message {
        profile_id: profile_id.to_string(),
        generation,
        topic: topic.clone(),
        payload,
        retain,
        payload_bytes: raw_len,
        props,
    });
    if skipped {
        let size_kb = (raw_len + 1023) / 1024;
        let _ = tx.send(BrokerEvent::Notice {
            profile_id: profile_id.to_string(),
            generation,
            detail: format!(
                "Skipped oversized topic {topic} ({size_kb} KB > {soft_kb} KB limit) — still connected"
            ),
        });
    }
}

fn emit_connecting(
    tx: &Sender<BrokerEvent>,
    ctx: &Context,
    ui_focused: &AtomicBool,
    profile_id: &str,
    generation: u64,
    endpoint: &str,
) {
    emit_state(
        tx,
        ctx,
        ui_focused,
        profile_id,
        generation,
        SessionState::Connecting,
        format!("Connecting to {endpoint}…"),
        &mut None,
        &mut Instant::now()
            .checked_sub(STATUS_MIN_INTERVAL)
            .unwrap_or_else(Instant::now),
        true,
    );
}

fn reconnect_backoff(
    tx: &Sender<BrokerEvent>,
    ctx: &Context,
    ui_focused: &AtomicBool,
    profile_id: &str,
    generation: u64,
    err: &str,
    backoff: &mut Duration,
    last_key: &mut Option<(SessionState, String)>,
    last_emit: &mut Instant,
    stop: &AtomicBool,
) {
    emit_state(
        tx,
        ctx,
        ui_focused,
        profile_id,
        generation,
        SessionState::Connecting,
        format!("Reconnecting in {}s… ({err})", backoff.as_secs().max(1)),
        last_key,
        last_emit,
        false,
    );
    if stop.load(Ordering::SeqCst) {
        return;
    }
    thread::sleep(*backoff);
    *backoff = (backoff.saturating_mul(2)).min(MAX_BACKOFF);
}

fn emit_final(
    tx: &Sender<BrokerEvent>,
    ctx: &Context,
    ui_focused: &AtomicBool,
    profile_id: &str,
    generation: u64,
    stop: &AtomicBool,
    fatal_detail: Option<String>,
    last_key: &mut Option<(SessionState, String)>,
    last_emit: &mut Instant,
) {
    let detail = if stop.load(Ordering::SeqCst) {
        "Disconnected".to_string()
    } else if let Some(fatal) = fatal_detail {
        fatal
    } else if let Some((_, last_detail)) = last_key.clone() {
        if last_detail.starts_with("Connected") {
            "Session ended (connection closed)".to_string()
        } else {
            last_detail
        }
    } else {
        "Session ended (connection closed)".to_string()
    };
    emit_state(
        tx,
        ctx,
        ui_focused,
        profile_id,
        generation,
        SessionState::Disconnected,
        detail,
        last_key,
        last_emit,
        true,
    );
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
    if ui_focused.load(Ordering::Relaxed) {
        ctx.request_repaint();
    }
}

pub fn to_qos_v311(q: AppQos) -> rumqttc::QoS {
    match q {
        AppQos::AtMostOnce => rumqttc::QoS::AtMostOnce,
        AppQos::AtLeastOnce => rumqttc::QoS::AtLeastOnce,
        AppQos::ExactlyOnce => rumqttc::QoS::ExactlyOnce,
    }
}

fn to_qos_v5(q: AppQos) -> rumqttc::v5::mqttbytes::QoS {
    match q {
        AppQos::AtMostOnce => rumqttc::v5::mqttbytes::QoS::AtMostOnce,
        AppQos::AtLeastOnce => rumqttc::v5::mqttbytes::QoS::AtLeastOnce,
        AppQos::ExactlyOnce => rumqttc::v5::mqttbytes::QoS::ExactlyOnce,
    }
}

/// Back-compat alias used by older call sites.
pub fn to_qos(q: AppQos) -> rumqttc::QoS {
    to_qos_v311(q)
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

        handle.stop.store(true, Ordering::SeqCst);
        handle.client.disconnect();

        assert!(
            wait_state(
                &rx,
                &profile.id,
                SessionState::Disconnected,
                Duration::from_secs(10)
            ),
            "expected Disconnected after stop"
        );

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

        handle
            .client
            .subscribe("#", AppQos::AtMostOnce)
            .expect("subscribe #");

        let topic = format!("mqtt-master/test/{}", unique_id("ping"));
        let payload = format!("hello-{}", unique_id("p"));
        handle
            .client
            .publish(
                &topic,
                AppQos::AtLeastOnce,
                false,
                payload.as_bytes(),
                None,
            )
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
        handle.client.disconnect();

        assert!(
            saw_own || any_msg > 0,
            "expected own roundtrip or `#` traffic from {MOSQUITTO_HOST} (saw_own={saw_own}, any={any_msg})"
        );
    }
}
