use serde::{Deserialize, Serialize};

use crate::schema::common::{default_schema_version, PromptFragments, Visual};

/// `world/<slug>.toml` — locations, props, vehicles, style bibles. The
/// filename stem is the entry's identity.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorldFile {
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    pub name: String,
    pub kind: WorldKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt: Option<PromptFragments>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visual: Option<Visual>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "String", into = "String")]
pub enum WorldKind {
    Location,
    Prop,
    Vehicle,
    /// Style bibles feed the `{style}` prompt slot instead of `{world}`.
    Style,
    Other(String),
}

impl From<String> for WorldKind {
    fn from(value: String) -> Self {
        match value.as_str() {
            "location" => Self::Location,
            "prop" => Self::Prop,
            "vehicle" => Self::Vehicle,
            "style" => Self::Style,
            _ => Self::Other(value),
        }
    }
}

impl From<WorldKind> for String {
    fn from(value: WorldKind) -> Self {
        match value {
            WorldKind::Location => "location".to_owned(),
            WorldKind::Prop => "prop".to_owned(),
            WorldKind::Vehicle => "vehicle".to_owned(),
            WorldKind::Style => "style".to_owned(),
            WorldKind::Other(s) => s,
        }
    }
}
