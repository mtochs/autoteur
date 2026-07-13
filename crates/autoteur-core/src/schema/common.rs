use std::collections::BTreeMap;

use serde::{Deserialize, Deserializer, Serialize};

use crate::id::Slug;

/// Current schema version for every file type. Bumped only for breaking
/// changes; additive evolution never bumps it.
pub const CURRENT_SCHEMA_VERSION: u32 = 1;

pub(crate) fn default_schema_version() -> u32 {
    CURRENT_SCHEMA_VERSION
}

/// Named corkboard color tokens. Raw `#rrggbb` is accepted as an escape
/// hatch; tokens keep files theme-neutral.
pub const COLOR_TOKENS: [&str; 8] = [
    "rose", "amber", "lime", "teal", "sky", "violet", "slate", "sand",
];

pub fn is_valid_color(value: &str) -> bool {
    COLOR_TOKENS.contains(&value)
        || (value.len() == 7
            && value.starts_with('#')
            && value[1..].chars().all(|c| c.is_ascii_hexdigit()))
}

/// `[prompt]` table shared by characters and world entries.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PromptFragments {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fragment: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub negative: Option<String>,
    /// Named fragment variants; a shot pins one with `slug:variant`.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub variants: BTreeMap<Slug, String>,
}

/// `[visual]` table shared by characters and world entries. Everything here
/// attaches to generation as inputs — never as prompt text.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Visual {
    /// Repo-relative paths, forward slashes; the first image is primary.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reference_images: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub adapters: Vec<Adapter>,
}

/// A LoRA or embedding attached to an entity's visual identity.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Adapter {
    pub kind: AdapterKind,
    /// Provider ref (`civitai:123456`), URL, or repo-relative path.
    pub source: String,
    #[serde(
        default,
        deserialize_with = "de_lenient_opt_f64",
        skip_serializing_if = "Option::is_none"
    )]
    pub weight: Option<f64>,
    /// Trigger token(s), auto-prepended to the owner's prompt fragment.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trigger: Option<String>,
    /// Embedding token, e.g. `<mara-chen>`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "String", into = "String")]
pub enum AdapterKind {
    Lora,
    Embedding,
    /// Unrecognized kind, preserved verbatim (forward compatibility).
    Other(String),
}

impl From<String> for AdapterKind {
    fn from(value: String) -> Self {
        match value.as_str() {
            "lora" => Self::Lora,
            "embedding" => Self::Embedding,
            _ => Self::Other(value),
        }
    }
}

impl From<AdapterKind> for String {
    fn from(value: AdapterKind) -> Self {
        match value {
            AdapterKind::Lora => "lora".to_owned(),
            AdapterKind::Embedding => "embedding".to_owned(),
            AdapterKind::Other(s) => s,
        }
    }
}

/// TOML distinguishes `6` from `6.0`; agents write both. Accept either for
/// any float-valued field, reject everything else with a message fit for
/// the "fix it for me" banner, and refuse nan/inf (they poison duration
/// math downstream).
pub(crate) fn de_lenient_opt_f64<'de, D>(deserializer: D) -> Result<Option<f64>, D::Error>
where
    D: Deserializer<'de>,
{
    struct NumVisitor;

    impl serde::de::Visitor<'_> for NumVisitor {
        type Value = f64;

        fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("a number (e.g. 6 or 6.0)")
        }

        fn visit_i64<E: serde::de::Error>(self, v: i64) -> Result<f64, E> {
            Ok(v as f64)
        }

        fn visit_u64<E: serde::de::Error>(self, v: u64) -> Result<f64, E> {
            Ok(v as f64)
        }

        fn visit_f64<E: serde::de::Error>(self, v: f64) -> Result<f64, E> {
            if v.is_finite() {
                Ok(v)
            } else {
                Err(E::custom("expected a finite number (not nan or inf)"))
            }
        }
    }

    deserializer.deserialize_any(NumVisitor).map(Some)
}
