use serde::{Deserialize, Serialize};

use crate::id::Slug;
use crate::schema::common::default_schema_version;

/// `story/beats.toml`. Block order IS board order; there are no position
/// fields. A feature film has no episodes and no `episode` keys.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BeatsFile {
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub episodes: Vec<Episode>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub beats: Vec<Beat>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Episode {
    /// Frozen at creation.
    pub id: Slug,
    pub title: String,
    /// Default card tint for this episode's beats.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Beat {
    /// Frozen at creation; scenes reference beats by this id.
    pub id: Slug,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    /// Series only: an `[[episodes]]` id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub episode: Option<Slug>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub act: Option<u32>,
    /// Manual tint override; absent = inherit episode color / act palette.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}
