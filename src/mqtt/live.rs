//! rumqttc worker threads: connect, stay connected, feed UI via channel.

use crate::mqtt::profile::ConnectionProfile;
use crate::mqtt::session::SessionState;
use eframe::egui::Context;
use rumqttc::{Client, Event, MqttOptions, Packet, QoS, Transport};
use std::sync::mpsc::Sender;
use std::thread;
use std::time::Duration;

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

/// Build options + spawn event-loop thread; returns a cloneable `Client` for sub/pub.
pub fn spawn(
    profile: &ConnectionProfile,
    event_tx: Sender<BrokerEvent>,
    ctx: Context,
) -> Result<Client, String> {
    let mut opts = MqttOptions::new(&profile.client_id, &profile.host, profile.port);
    opts.set_keep_alive(Duration::from_secs(30));
    opts.set_clean_session(true);
    if !profile.username.is_empty() {
        opts.set_credentials(&profile.username, &profile.password);
    }
    if profile.use_tls {
        opts.set_transport(Transport::tls_with_default_config());
    }

    let (client, mut connection) = Client::new(opts, 100);
    let profile_id = profile.id.clone();
    let tx = event_tx;

    // Notify connecting immediately
    let _ = tx.send(BrokerEvent::State {
        profile_id: profile_id.clone(),
        state: SessionState::Connecting,
        detail: format!("Connecting to {}:{}…", profile.host, profile.port),
    });
    ctx.request_repaint();

    thread::Builder::new()
        .name(format!("mqtt-{}", profile_id))
        .spawn(move || {
            for notification in connection.iter() {
                match notification {
                    Ok(Event::Incoming(Packet::ConnAck(ack))) => {
                        let detail = if ack.code == rumqttc::ConnectReturnCode::Success {
                            "Connected".into()
                        } else {
                            format!("ConnAck: {:?}", ack.code)
                        };
                        let state = if ack.code == rumqttc::ConnectReturnCode::Success {
                            SessionState::Connected
                        } else {
                            SessionState::Disconnected
                        };
                        let _ = tx.send(BrokerEvent::State {
                            profile_id: profile_id.clone(),
                            state,
                            detail,
                        });
                        ctx.request_repaint();
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
                        let _ = tx.send(BrokerEvent::State {
                            profile_id: profile_id.clone(),
                            state: SessionState::Disconnected,
                            detail: "Broker disconnected".into(),
                        });
                        ctx.request_repaint();
                    }
                    Ok(_) => {}
                    Err(e) => {
                        // Keep iterating — rumqttc reconnects automatically.
                        let _ = tx.send(BrokerEvent::State {
                            profile_id: profile_id.clone(),
                            state: SessionState::Connecting,
                            detail: format!("Reconnecting… ({e})"),
                        });
                        ctx.request_repaint();
                    }
                }
            }
            let _ = tx.send(BrokerEvent::State {
                profile_id: profile_id.clone(),
                state: SessionState::Disconnected,
                detail: "Session ended".into(),
            });
            ctx.request_repaint();
        })
        .map_err(|e| format!("spawn worker: {e}"))?;

    Ok(client)
}

pub fn to_qos(q: crate::mqtt::publish::Qos) -> QoS {
    match q {
        crate::mqtt::publish::Qos::AtMostOnce => QoS::AtMostOnce,
        crate::mqtt::publish::Qos::AtLeastOnce => QoS::AtLeastOnce,
        crate::mqtt::publish::Qos::ExactlyOnce => QoS::ExactlyOnce,
    }
}
