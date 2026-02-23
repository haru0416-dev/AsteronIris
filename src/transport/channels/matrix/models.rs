use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub(super) struct SyncResponse {
    pub(super) next_batch: String,
    #[serde(default)]
    pub(super) rooms: Rooms,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct Rooms {
    #[serde(default)]
    pub(super) join: std::collections::HashMap<String, JoinedRoom>,
}

#[derive(Debug, Deserialize)]
pub(super) struct JoinedRoom {
    #[serde(default)]
    pub(super) timeline: Timeline,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct Timeline {
    #[serde(default)]
    pub(super) events: Vec<TimelineEvent>,
}

#[derive(Debug, Deserialize)]
pub(super) struct TimelineEvent {
    #[serde(rename = "type")]
    pub(super) event_type: String,
    pub(super) sender: String,
    #[serde(default)]
    pub(super) content: EventContent,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct EventContent {
    #[serde(default)]
    pub(super) body: Option<String>,
    #[serde(default)]
    pub(super) msgtype: Option<String>,
    #[serde(default)]
    pub(super) url: Option<String>,
    #[serde(default)]
    pub(super) info: Option<EventContentInfo>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct EventContentInfo {
    #[serde(default)]
    pub(super) mimetype: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct WhoAmIResponse {
    pub(super) user_id: String,
}
