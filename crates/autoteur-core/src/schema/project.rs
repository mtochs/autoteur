use serde::{Deserialize, Serialize};

use crate::id::Slug;
use crate::schema::common::default_schema_version;

/// `autoteur.toml` — the project manifest.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProjectFile {
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    pub title: String,
    /// UI hint (enables the episode lane), not a schema switch: a feature's
    /// files are byte-for-byte valid series files with fields omitted.
    #[serde(default)]
    pub format: ProjectFormat,
    #[serde(default)]
    pub defaults: Defaults,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "String", into = "String")]
pub enum ProjectFormat {
    #[default]
    Feature,
    Series,
    Other(String),
}

impl From<String> for ProjectFormat {
    fn from(value: String) -> Self {
        match value.as_str() {
            "feature" => Self::Feature,
            "series" => Self::Series,
            _ => Self::Other(value),
        }
    }
}

impl From<ProjectFormat> for String {
    fn from(value: ProjectFormat) -> Self {
        match value {
            ProjectFormat::Feature => "feature".to_owned(),
            ProjectFormat::Series => "series".to_owned(),
            ProjectFormat::Other(s) => s,
        }
    }
}

/// `[defaults]` — project-wide generation defaults.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Defaults {
    /// Template used when a shot has no `prompt` of its own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_template: Option<String>,
    /// Base negative prompt, composed with entity negatives.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub negative: Option<String>,
    /// World slugs whose fragments fill `{style}` in every prompt.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub style: Vec<Slug>,
    /// Default provider id for generation, e.g. "replicate".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    /// Default model for video shots (`owner/name` or `owner/name:version`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub video_model: Option<String>,
    /// Default model for still/image shots.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_model: Option<String>,
}
