use serde::{Deserialize, Serialize};

use crate::id::Slug;
use crate::schema::common::default_schema_version;

/// `scenes/<NNN>-<slug>/scene.toml`. The directory slug is the scene's
/// identity; NNN is sort order only. This file intentionally has no `id`
/// field and no status field.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SceneFile {
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    pub title: String,
    /// Beats this scene realizes. Empty = unmapped (shown in a board tray).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub beats: Vec<Slug>,
    /// Scene cast; shots inherit this unless they override.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub characters: Vec<Slug>,
    /// Primary setting: a world entry with `kind = "location"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location: Option<Slug>,
    /// Additional world entries in play (props, vehicles, style bibles).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub world: Vec<Slug>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub int_ext: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mood: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub synopsis: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub director_notes: Option<String>,
}

/// Split a scene directory name `012-vault-breach` into its sort number and
/// identity slug. Returns `None` when the name doesn't follow the format.
pub fn parse_scene_dir_name(name: &str) -> Option<(u32, Slug)> {
    let (number, slug) = name.split_once('-')?;
    if number.is_empty() || !number.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    Some((number.parse().ok()?, slug.parse().ok()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scene_dir_names_parse() {
        let (n, slug) = parse_scene_dir_name("012-vault-breach").expect("valid");
        assert_eq!(n, 12);
        assert_eq!(slug.as_str(), "vault-breach");
        // 4-digit prefixes are fine; the app sorts numerically.
        assert!(parse_scene_dir_name("1000-finale").is_some());
        assert!(parse_scene_dir_name("vault-breach").is_none());
        assert!(parse_scene_dir_name("12_vault").is_none());
        assert!(parse_scene_dir_name("-vault").is_none());
    }
}
