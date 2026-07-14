//! File-level validation. Errors mark a file stale (the UI keeps last-good
//! state, exactly like a syntax error); warnings surface as gentle notices.
//! The big target is silent misplacement: TOML puts any key appearing after
//! a table header inside that table, so an agent appending at end-of-file
//! can park a key on the wrong entity without any parse error.

use std::collections::BTreeSet;

use toml_edit::{DocumentMut, Item, Table, TableLike};

use crate::schema::beats::BeatsFile;
use crate::schema::character::CharacterFile;
use crate::schema::common::is_valid_color;
use crate::schema::scene::SceneFile;
use crate::schema::shots::{ShotStatus, ShotsFile};
use crate::schema::world::WorldFile;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    /// The file is unsafe to act on; treat like a parse failure.
    Error,
    /// Worth a notice; the file remains usable.
    Warning,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct Lint {
    pub severity: Severity,
    pub message: String,
}

impl Lint {
    fn error(message: String) -> Self {
        Self {
            severity: Severity::Error,
            message,
        }
    }

    fn warning(message: String) -> Self {
        Self {
            severity: Severity::Warning,
            message,
        }
    }
}

pub fn has_errors(lints: &[Lint]) -> bool {
    lints.iter().any(|l| l.severity == Severity::Error)
}

const BEAT_KEYS: &[&str] = &["id", "title", "summary", "episode", "act", "color", "notes"];
const EPISODE_KEYS: &[&str] = &["id", "title", "color"];
const SHOT_KEYS: &[&str] = &[
    "id",
    "framing",
    "camera",
    "action",
    "characters",
    "world",
    "dialogue",
    "duration_s",
    "status",
    "selected_take",
    "prompt",
    "prompt_extra",
    "negative_extra",
    "notes",
];
const DIALOGUE_KEYS: &[&str] = &["character", "line", "delivery"];
const CHARACTER_ROOT_KEYS: &[&str] = &[
    "schema_version",
    "name",
    "aliases",
    "description",
    "voice",
    "prompt",
    "visual",
];
const WORLD_ROOT_KEYS: &[&str] = &[
    "schema_version",
    "name",
    "kind",
    "description",
    "prompt",
    "visual",
];
const VOICE_KEYS: &[&str] = &["provider", "voice_id", "style", "reference_audio", "notes"];
const PROMPT_KEYS: &[&str] = &["fragment", "negative", "variants"];
const VISUAL_KEYS: &[&str] = &["reference_images", "adapters"];
const ADAPTER_KEYS: &[&str] = &["kind", "source", "weight", "trigger", "token"];

pub fn lint_beats(doc: &DocumentMut, data: &BeatsFile) -> Vec<Lint> {
    let mut lints = Vec::new();
    check_schema_version(doc, &mut lints);

    let mut episode_ids = BTreeSet::new();
    for episode in &data.episodes {
        if !episode_ids.insert(episode.id.clone()) {
            lints.push(Lint::error(format!(
                "two episodes share the id `{}`",
                episode.id
            )));
        }
        if let Some(color) = &episode.color {
            check_color(color, &format!("episode `{}`", episode.id), &mut lints);
        }
    }

    let mut beat_ids = BTreeSet::new();
    for beat in &data.beats {
        if !beat_ids.insert(beat.id.clone()) {
            lints.push(Lint::error(format!("two beats share the id `{}`", beat.id)));
        }
        if let Some(color) = &beat.color {
            check_color(color, &format!("beat `{}`", beat.id), &mut lints);
        }
        if let Some(episode) = &beat.episode {
            if !data.episodes.is_empty() && !episode_ids.contains(episode) {
                lints.push(Lint::warning(format!(
                    "beat `{}` references unknown episode `{episode}`",
                    beat.id
                )));
            }
            if data.episodes.is_empty() {
                lints.push(Lint::warning(format!(
                    "beat `{}` has an episode but no [[episodes]] are defined",
                    beat.id
                )));
            }
        }
    }

    check_blocks(doc, "beats", BEAT_KEYS, &["schema_version"], &mut lints);
    check_blocks(
        doc,
        "episodes",
        EPISODE_KEYS,
        &["schema_version"],
        &mut lints,
    );
    lints
}

pub fn lint_shots(doc: &DocumentMut, data: &ShotsFile) -> Vec<Lint> {
    let mut lints = Vec::new();
    check_schema_version(doc, &mut lints);

    let mut shot_ids = BTreeSet::new();
    for shot in &data.shots {
        if !shot_ids.insert(shot.id.clone()) {
            lints.push(Lint::error(format!("two shots share the id `{}`", shot.id)));
        }
        if let ShotStatus::Other(s) = &shot.status {
            lints.push(Lint::warning(format!(
                "shot `{}` has unknown status {s:?} (expected planned | ready | locked | omitted)",
                shot.id
            )));
        }
    }

    check_blocks(doc, "shots", SHOT_KEYS, &["schema_version"], &mut lints);

    // Dialogue cues must be single-line inline tables. Positional arrays
    // (["mara", "hi"]) field-map silently through serde and swapped fields
    // produce a wrong cue; sub-table blocks invite the key-after-table
    // trap. Both get flagged, and unknown cue keys are typos.
    if let Some(aot) = doc.get("shots").and_then(Item::as_array_of_tables) {
        for table in aot.iter() {
            let shot_label = block_label(table, "shot");
            match table.get("dialogue") {
                Some(Item::Value(value)) => {
                    let cues = value.as_array().into_iter().flat_map(|arr| arr.iter());
                    for cue in cues {
                        if let Some(inline) = cue.as_inline_table() {
                            check_cue_keys(inline, &shot_label, &mut lints);
                        } else {
                            lints.push(Lint::warning(format!(
                                "a dialogue cue on {shot_label} is not an inline table — write cues as {{ character = \"...\", line = \"...\" }}, one per line"
                            )));
                        }
                    }
                }
                Some(Item::ArrayOfTables(cues)) => {
                    lints.push(Lint::warning(format!(
                        "{shot_label} writes dialogue as [[shots.dialogue]] blocks — house style is single-line inline tables (keys added after those blocks land inside the last cue)"
                    )));
                    for cue in cues.iter() {
                        check_cue_keys(cue, &shot_label, &mut lints);
                    }
                }
                _ => {}
            }
        }
    }
    lints
}

fn check_cue_keys(cue: &dyn TableLike, shot_label: &str, lints: &mut Vec<Lint>) {
    for (key, _) in cue.iter() {
        if !DIALOGUE_KEYS.contains(&key) {
            lints.push(Lint::warning(format!(
                "unknown key `{key}` on a dialogue cue of {shot_label}"
            )));
        }
    }
}

pub fn lint_scene(doc: &DocumentMut, _data: &SceneFile) -> Vec<Lint> {
    let mut lints = Vec::new();
    check_schema_version(doc, &mut lints);
    lints
}

pub fn lint_character(doc: &DocumentMut, _data: &CharacterFile) -> Vec<Lint> {
    let mut lints = Vec::new();
    check_schema_version(doc, &mut lints);
    check_subtable(doc, "voice", VOICE_KEYS, CHARACTER_ROOT_KEYS, &mut lints);
    check_subtable(doc, "prompt", PROMPT_KEYS, CHARACTER_ROOT_KEYS, &mut lints);
    check_subtable(doc, "visual", VISUAL_KEYS, CHARACTER_ROOT_KEYS, &mut lints);
    check_variants(doc, CHARACTER_ROOT_KEYS, &mut lints);
    check_adapters(doc, CHARACTER_ROOT_KEYS, &mut lints);
    lints
}

pub fn lint_world(doc: &DocumentMut, data: &WorldFile) -> Vec<Lint> {
    let mut lints = Vec::new();
    check_schema_version(doc, &mut lints);
    if let crate::schema::world::WorldKind::Other(kind) = &data.kind {
        lints.push(Lint::warning(format!(
            "unknown kind {kind:?} (expected location | prop | vehicle | style)"
        )));
    }
    check_subtable(doc, "prompt", PROMPT_KEYS, WORLD_ROOT_KEYS, &mut lints);
    check_subtable(doc, "visual", VISUAL_KEYS, WORLD_ROOT_KEYS, &mut lints);
    check_variants(doc, WORLD_ROOT_KEYS, &mut lints);
    check_adapters(doc, WORLD_ROOT_KEYS, &mut lints);
    lints
}

fn check_schema_version(doc: &DocumentMut, lints: &mut Vec<Lint>) {
    if !doc.contains_key("schema_version") {
        lints.push(Lint::warning(
            "missing schema_version (expected as the first key of the file)".to_owned(),
        ));
    }
}

fn check_color(color: &str, context: &str, lints: &mut Vec<Lint>) {
    if !is_valid_color(color) {
        lints.push(Lint::warning(format!(
            "{context} has unknown color {color:?} (expected one of rose, amber, lime, teal, sky, violet, slate, sand, or #rrggbb)"
        )));
    }
}

/// Flag unknown keys inside every block of a `[[key]]` array. Unknown keys
/// at document root stay silently preserved (forward compatibility); inside
/// a block they are almost always a misplaced append.
fn check_blocks(
    doc: &DocumentMut,
    key: &str,
    known: &[&str],
    root_keys: &[&str],
    lints: &mut Vec<Lint>,
) {
    let Some(aot) = doc.get(key).and_then(Item::as_array_of_tables) else {
        return;
    };
    let singular = key.strip_suffix('s').unwrap_or(key);
    for table in aot.iter() {
        let label = block_label(table, singular);
        check_table_keys(table, known, root_keys, &label, lints);
    }
}

// `as_table_like` covers standard, dotted, AND inline-table spellings —
// `voice = { ... }` must get the same checks as `[voice]`.
fn check_subtable(
    doc: &DocumentMut,
    name: &str,
    known: &[&str],
    root_keys: &[&str],
    lints: &mut Vec<Lint>,
) {
    if let Some(table) = doc.get(name).and_then(Item::as_table_like) {
        check_table_keys(table, known, root_keys, &format!("[{name}]"), lints);
    }
}

/// Root keys appended after `[prompt.variants]` become bogus variants with
/// no error anywhere else — the variants table accepts freeform names, so
/// only known-root-key collisions are detectable.
fn check_variants(doc: &DocumentMut, root_keys: &[&str], lints: &mut Vec<Lint>) {
    let Some(variants) = doc
        .get("prompt")
        .and_then(Item::as_table_like)
        .and_then(|p| p.get("variants"))
        .and_then(Item::as_table_like)
    else {
        return;
    };
    for (key, _) in variants.iter() {
        if root_keys.contains(&key) {
            lints.push(Lint::warning(format!(
                "top-level key `{key}` found inside [prompt.variants] — it parsed as a variant named {key:?}; move it up under schema_version"
            )));
        }
    }
}

fn check_adapters(doc: &DocumentMut, root_keys: &[&str], lints: &mut Vec<Lint>) {
    let Some(adapters) = doc
        .get("visual")
        .and_then(Item::as_table_like)
        .and_then(|v| v.get("adapters"))
    else {
        return;
    };
    let context = |i: usize| format!("[[visual.adapters]] entry {}", i + 1);
    if let Some(aot) = adapters.as_array_of_tables() {
        for (i, table) in aot.iter().enumerate() {
            check_table_keys(table, ADAPTER_KEYS, root_keys, &context(i), lints);
        }
    } else if let Some(array) = adapters.as_array() {
        for (i, value) in array.iter().enumerate() {
            if let Some(inline) = value.as_inline_table() {
                check_table_keys(inline, ADAPTER_KEYS, root_keys, &context(i), lints);
            }
        }
    }
}

fn check_table_keys(
    table: &dyn TableLike,
    known: &[&str],
    root_keys: &[&str],
    context: &str,
    lints: &mut Vec<Lint>,
) {
    for (key, _) in table.iter() {
        if known.contains(&key) {
            continue;
        }
        if root_keys.contains(&key) {
            lints.push(Lint::warning(format!(
                "top-level key `{key}` found inside {context} — keys appended after a table header land in that table; move it up under schema_version"
            )));
        } else {
            lints.push(Lint::warning(format!("unknown key `{key}` on {context}")));
        }
    }
}

fn block_label(table: &Table, singular: &str) -> String {
    match table.get("id").and_then(Item::as_str) {
        Some(id) => format!("{singular} `{id}`"),
        None => format!("a {singular} with no id"),
    }
}
