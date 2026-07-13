use serde::{Deserialize, Serialize};

use crate::id::{ShotRef, Slug};
use crate::schema::common::{de_lenient_opt_f64, default_schema_version};

/// `timeline.toml` — Editing Room assembly. Entries reference shots and
/// resolve `selected_take` live (one source of truth with Dailies); trims
/// clamp to the current take's duration at render time.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TimelineFile {
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    /// Feature form: the single implicit sequence.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub entries: Vec<TimelineEntry>,
    /// Series form: one sequence per episode.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sequences: Vec<Sequence>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Sequence {
    pub episode: Slug,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub entries: Vec<TimelineEntry>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TimelineEntry {
    pub shot: ShotRef,
    #[serde(
        default,
        deserialize_with = "de_lenient_opt_f64",
        skip_serializing_if = "Option::is_none"
    )]
    pub in_s: Option<f64>,
    #[serde(
        default,
        deserialize_with = "de_lenient_opt_f64",
        skip_serializing_if = "Option::is_none"
    )]
    pub out_s: Option<f64>,
}
