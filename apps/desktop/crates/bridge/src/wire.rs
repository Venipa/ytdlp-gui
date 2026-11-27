//! Wire messages of protocol v1 (JSON text frames `{"v":1,"id":..,"type":..,..}`).
use serde::{Deserialize, Serialize};

/// Protocol version this build speaks.
pub const PROTO: u32 = 1;

/// Page kind in a [`Checked`] answer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    /// One video.
    Video,
    /// A playlist.
    Playlist,
    /// A channel or profile.
    Channel,
    /// Cannot tell.
    Unknown,
}

/// One download preset the bar can offer.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PresetInfo {
    /// Stable id such as `video:1080`.
    pub id: String,
    /// Label already localized by the app.
    pub label: String,
}

/// Answer to `check`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Checked {
    /// The URL that was checked.
    pub url: String,
    /// Whether a yt-dlp extractor (not generic) claims it.
    pub supported: bool,
    /// Extractor key, when supported.
    pub extractor: Option<String>,
    /// Extractor name, when supported.
    pub name: Option<String>,
    /// Page kind.
    pub kind: Kind,
    /// Presets to offer; only when supported.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub presets: Vec<PresetInfo>,
    /// Id of the default preset; only when supported.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preset: Option<String>,
}

/// The `add` request.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AddReq {
    /// http(s) link.
    pub url: String,
    /// Preset id; `None` = the app's default for the URL.
    #[serde(default)]
    pub preset: Option<String>,
    /// `true` queues at once, `false` opens the link in the app's intake box.
    pub start: bool,
}

/// Where an added link ended up.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AddState {
    /// Stored and queued.
    Queued,
    /// Handed to the intake box.
    Intake,
}

/// Answer to `add`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Added {
    /// Download id when one was created.
    pub download: Option<i64>,
    /// Where it went.
    pub state: AddState,
    /// An equal link already was in the list.
    pub duplicate: bool,
}

/// A refusal, sent as an `error` message.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireError {
    /// Machine code: `unsupported`, `busy`, `engine_unavailable`, `rate_limited`, ...
    pub code: String,
    /// Human text for logs and tooltips.
    pub message: String,
    /// When to retry, if it helps.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry_after_ms: Option<u64>,
}

impl WireError {
    /// An error with `code` and `message`.
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            retry_after_ms: None,
        }
    }
}

/// App -> client messages.
#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum Resp {
    HelloOk {
        app: &'static str,
        app_version: String,
        proto: u32,
        instance: String,
        label: String,
    },
    Paired {
        token: String,
        device: String,
    },
    AuthOk {
        device: String,
    },
    Pong {},
    Checked(Checked),
    Added(Added),
    Error(WireError),
}

impl Resp {
    /// The frame text, with the envelope fields added.
    pub(crate) fn frame(&self, id: Option<u64>) -> String {
        let mut v = serde_json::to_value(self).unwrap_or_default();
        if let Some(o) = v.as_object_mut() {
            o.insert("v".into(), PROTO.into());
            if let Some(id) = id {
                o.insert("id".into(), id.into());
            }
        }
        v.to_string()
    }
}
