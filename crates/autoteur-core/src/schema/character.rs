use serde::{Deserialize, Serialize};

use crate::schema::common::{default_schema_version, PromptFragments, Visual};

/// `characters/<slug>.toml`. The filename stem is the character's identity;
/// this file has no `id` field.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CharacterFile {
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    pub name: String,
    /// Display names only; not valid as references.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub aliases: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub voice: Option<Voice>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt: Option<PromptFragments>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visual: Option<Visual>,
}

/// `[voice]` — used by the dialogue/audio pipeline, never the image prompt.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Voice {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub voice_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reference_audio: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}
