use serde::{Deserialize, Serialize};

use crate::id::{ShotRef, TakeId};
use crate::schema::common::{de_lenient_opt_f64, default_schema_version};

/// `takes.manifest.toml` — committed, append-only record of every
/// generation. Written only by the generation pipeline; takes are immutable.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TakesManifest {
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub takes: Vec<TakeRecord>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TakeRecord {
    /// `tk_` + first 12 hex of the BLAKE3 of the primary output media.
    pub id: TakeId,
    /// `<scene-slug>/<shot-id>`.
    pub shot: ShotRef,
    pub provider: String,
    pub model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seed: Option<i64>,
    #[serde(
        default,
        deserialize_with = "de_lenient_opt_f64",
        skip_serializing_if = "Option::is_none"
    )]
    pub cost_usd: Option<f64>,
    /// RFC 3339 string; TOML native datetimes are not used in Autoteur files.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    /// Snapshot of the exact prompt sent — permanent even after fragments change.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_prompt: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub negative_prompt: Option<String>,
    /// Full provider inputs, verbatim.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inputs: Option<toml::Table>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub outputs: Vec<TakeOutput>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TakeOutput {
    /// Full BLAKE3 hex of the output file (the take id uses its first 12).
    pub hash: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    /// Location under the gitignored `takes/` store.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(
        default,
        deserialize_with = "de_lenient_opt_f64",
        skip_serializing_if = "Option::is_none"
    )]
    pub duration_s: Option<f64>,
}
