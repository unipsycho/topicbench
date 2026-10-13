//! Publish panel state (payload, QoS, retain) — not yet sent via rumqttc.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Qos {
    AtMostOnce = 0,
    AtLeastOnce = 1,
    ExactlyOnce = 2,
}

impl Qos {
    pub fn as_u8(self) -> u8 {
        self as u8
    }
}

#[derive(Clone, Debug)]
pub struct PublishDraft {
    pub profile_id: Option<String>,
    pub topic: String,
    pub payload: String,
    pub qos: Qos,
    pub retain: bool,
}

impl Default for PublishDraft {
    fn default() -> Self {
        Self {
            profile_id: None,
            topic: "sensors/demo".into(),
            payload: r#"{"ok":true}"#.into(),
            qos: Qos::AtMostOnce,
            retain: false,
        }
    }
}

/// Placeholder publish — returns a status string until rumqttc client is wired.
pub fn publish_placeholder(draft: &PublishDraft) -> String {
    let broker = draft.profile_id.as_deref().unwrap_or("(no profile)");
    format!(
        "Publish stub → {broker} topic={} qos={} retain={} ({} bytes)",
        draft.topic,
        draft.qos.as_u8(),
        draft.retain,
        draft.payload.len()
    )
}
