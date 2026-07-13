use serde::{Deserialize, Serialize};

use crate::id::{CastEntry, ShotId, Slug, TakeId};
use crate::schema::common::{de_lenient_opt_f64, default_schema_version};

/// `scenes/<NNN>-<slug>/shots.toml`. Block order IS cut order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ShotsFile {
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub shots: Vec<Shot>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Shot {
    /// Per-scene letter, frozen at creation, never reused.
    pub id: ShotId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub framing: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub camera: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action: Option<String>,
    /// Absent = inherit the scene cast. `[]` = nobody in frame. Explicit
    /// list = exactly these are injected (with optional variant pins).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub characters: Option<Vec<CastEntry>>,
    /// Absent = inherit the scene's location + world. Explicit list =
    /// replaces both for this shot.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub world: Option<Vec<Slug>>,
    /// Ordered cues; array order is speaking order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dialogue: Vec<DialogueCue>,
    /// Target seconds; actual trim lives in timeline.toml.
    #[serde(
        default,
        deserialize_with = "de_lenient_opt_f64",
        skip_serializing_if = "Option::is_none"
    )]
    pub duration_s: Option<f64>,
    /// Authored intent only — derived facts (take counts, generating,
    /// circled) are computed live and never stored here.
    #[serde(default)]
    pub status: ShotStatus,
    /// The circled take. Absent = no selection; never an empty string.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected_take: Option<TakeId>,
    /// Custom prompt template; placeholders keep identity injection alive.
    /// A literal prompt is a template with zero placeholders.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_extra: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub negative_extra: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DialogueCue {
    pub character: Slug,
    pub line: String,
    /// Optional parenthetical / performance direction.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delivery: Option<String>,
}

/// Authored shot status. Unknown values are preserved (never data loss) and
/// surfaced as a validation warning.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "String", into = "String")]
pub enum ShotStatus {
    #[default]
    Planned,
    Ready,
    Locked,
    Omitted,
    Other(String),
}

impl From<String> for ShotStatus {
    fn from(value: String) -> Self {
        match value.as_str() {
            "planned" => Self::Planned,
            "ready" => Self::Ready,
            "locked" => Self::Locked,
            "omitted" => Self::Omitted,
            _ => Self::Other(value),
        }
    }
}

impl From<ShotStatus> for String {
    fn from(value: ShotStatus) -> Self {
        match value {
            ShotStatus::Planned => "planned".to_owned(),
            ShotStatus::Ready => "ready".to_owned(),
            ShotStatus::Locked => "locked".to_owned(),
            ShotStatus::Omitted => "omitted".to_owned(),
            ShotStatus::Other(s) => s,
        }
    }
}
