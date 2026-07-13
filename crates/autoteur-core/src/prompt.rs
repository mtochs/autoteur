//! Prompt resolution: the pure function from committed files to the exact
//! generation request. Computed live for the compose view, snapshotted
//! per-take into the manifest, never written back into authored files.
//!
//! Two load-bearing rules:
//! - Prompt text and identity assets are separate channels. Reference
//!   images and adapters attach from the effective cast and world even
//!   under a fully literal shot prompt.
//! - Substitution reads the template only, in a single pass. Braces inside
//!   authored prose (action text, fragments, dialogue) are never
//!   re-interpreted as placeholders.

use std::collections::{BTreeMap, BTreeSet};

use crate::id::{CastEntry, Slug};
use crate::schema::character::CharacterFile;
use crate::schema::common::{Adapter, Visual};
use crate::schema::project::Defaults;
use crate::schema::scene::SceneFile;
use crate::schema::shots::Shot;
use crate::schema::world::{WorldFile, WorldKind};

/// Used when neither the shot nor the project defines a template.
pub const DEFAULT_PROMPT_TEMPLATE: &str =
    "{style}\n{framing}. {action}\n{camera}\n{characters}\n{location}\n{world}\n{mood}\n{extra}";

const KNOWN_SLOTS: [&str; 10] = [
    "style",
    "framing",
    "camera",
    "action",
    "characters",
    "location",
    "world",
    "mood",
    "dialogue",
    "extra",
];

pub struct PromptContext<'a> {
    pub defaults: &'a Defaults,
    pub scene: &'a SceneFile,
    pub characters: &'a BTreeMap<Slug, CharacterFile>,
    pub world: &'a BTreeMap<Slug, WorldFile>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedPrompt {
    pub prompt: String,
    pub negative: Option<String>,
    /// In attachment order: cast first, then location, world, style.
    pub reference_images: Vec<OwnedRef>,
    pub adapters: Vec<OwnedAdapter>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct OwnedRef {
    pub owner: Slug,
    pub path: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct OwnedAdapter {
    pub owner: Slug,
    pub adapter: Adapter,
}

/// Expand terse framing vocabulary into phrases a generation model reads
/// correctly ("MCU" means nothing to a diffusion model). Unrecognized
/// framing passes through verbatim.
pub fn expand_framing(framing: &str) -> String {
    match framing.trim().to_ascii_lowercase().as_str() {
        "wide" | "ws" => "wide shot".to_owned(),
        "extreme-wide" | "ews" => "extreme wide shot".to_owned(),
        "medium" | "ms" => "medium shot".to_owned(),
        "medium-close-up" | "mcu" => "medium close-up".to_owned(),
        "close-up" | "cu" => "close-up".to_owned(),
        "extreme-close-up" | "ecu" => "extreme close-up".to_owned(),
        "two-shot" => "two shot".to_owned(),
        "ots" | "over-the-shoulder" => "over-the-shoulder shot".to_owned(),
        "pov" => "point-of-view shot".to_owned(),
        "insert" => "insert shot".to_owned(),
        "aerial" => "aerial shot".to_owned(),
        _ => framing.trim().to_owned(),
    }
}

pub fn resolve(ctx: &PromptContext<'_>, shot: &Shot) -> ResolvedPrompt {
    let mut warnings = Vec::new();

    // Effective cast: absent = inherit scene cast, [] = nobody, explicit = exact.
    let cast: Vec<CastEntry> = match &shot.characters {
        Some(entries) => entries.clone(),
        None => ctx
            .scene
            .characters
            .iter()
            .cloned()
            .map(CastEntry::of)
            .collect(),
    };

    // Effective setting: absent = inherit scene location + world; explicit
    // shot world replaces both.
    let (location, world_slugs): (Option<Slug>, Vec<Slug>) = match &shot.world {
        Some(world) => (None, world.clone()),
        None => (ctx.scene.location.clone(), ctx.scene.world.clone()),
    };

    // Style = project defaults + effective world entries of kind "style".
    let mut style_slugs: Vec<Slug> = ctx.defaults.style.clone();
    let mut other_world: Vec<Slug> = Vec::new();
    for slug in &world_slugs {
        match ctx.world.get(slug) {
            Some(entry) if entry.kind == WorldKind::Style => style_slugs.push(slug.clone()),
            Some(_) => other_world.push(slug.clone()),
            None => warnings.push(format!("unknown world entry `{slug}`")),
        }
    }
    let mut seen = BTreeSet::new();
    style_slugs.retain(|s| seen.insert(s.clone()));

    // A world entry's text = its adapter trigger tokens, then its fragment —
    // same contract as characters, so world/style LoRAs actually activate.
    let world_text_of = |slug: &Slug, warnings: &mut Vec<String>| -> Option<String> {
        match ctx.world.get(slug) {
            Some(entry) => {
                let fragment = entry.prompt.as_ref().and_then(|p| p.fragment.clone());
                segments_with_triggers(entry.visual.as_ref(), fragment)
            }
            None => {
                warnings.push(format!("unknown world entry `{slug}`"));
                None
            }
        }
    };

    let style_text = join_fragments(&style_slugs, &world_text_of, &mut warnings);
    let world_text = join_fragments(&other_world, &world_text_of, &mut warnings);
    let location_text = location
        .as_ref()
        .and_then(|slug| world_text_of(slug, &mut warnings))
        .unwrap_or_default();

    // Character lines: adapter triggers, then the (possibly variant) fragment.
    let mut character_lines: Vec<String> = Vec::new();
    for entry in &cast {
        let Some(character) = ctx.characters.get(&entry.character) else {
            warnings.push(format!("unknown character `{}`", entry.character));
            continue;
        };
        let fragments = character.prompt.as_ref();
        let base = fragments.and_then(|p| p.fragment.clone());
        let text = match &entry.variant {
            Some(variant) => match fragments.and_then(|p| p.variants.get(variant).cloned()) {
                Some(v) => Some(v),
                None => {
                    warnings.push(format!(
                        "character `{}` has no variant `{variant}`",
                        entry.character
                    ));
                    base
                }
            },
            None => base,
        };
        if let Some(line) = segments_with_triggers(character.visual.as_ref(), text) {
            character_lines.push(line);
        }
    }
    let characters_text = character_lines.join("\n");

    let dialogue_text = shot
        .dialogue
        .iter()
        .map(|cue| {
            let name = ctx
                .characters
                .get(&cue.character)
                .map(|c| c.name.as_str())
                .unwrap_or_else(|| cue.character.as_str());
            match &cue.delivery {
                Some(delivery) => format!("{name}: \"{}\" ({delivery})", cue.line),
                None => format!("{name}: \"{}\"", cue.line),
            }
        })
        .collect::<Vec<_>>()
        .join("\n");

    let framing_text = shot
        .framing
        .as_deref()
        .map(expand_framing)
        .unwrap_or_default();

    let template = shot
        .prompt
        .clone()
        .or_else(|| ctx.defaults.prompt_template.clone())
        .unwrap_or_else(|| DEFAULT_PROMPT_TEMPLATE.to_owned());

    let mut values: BTreeMap<&str, String> = BTreeMap::new();
    values.insert("style", style_text);
    values.insert("framing", framing_text);
    values.insert(
        "camera",
        shot.camera.as_deref().unwrap_or("").trim().to_owned(),
    );
    values.insert(
        "action",
        shot.action.as_deref().unwrap_or("").trim().to_owned(),
    );
    values.insert("characters", characters_text);
    values.insert("location", location_text);
    values.insert("world", world_text);
    values.insert(
        "mood",
        ctx.scene.mood.as_deref().unwrap_or("").trim().to_owned(),
    );
    values.insert("dialogue", dialogue_text);
    values.insert(
        "extra",
        shot.prompt_extra.as_deref().unwrap_or("").trim().to_owned(),
    );

    let prompt = render_template(&template, &values, &mut warnings);

    // Negative: project default, then style/location/world, cast, shot extra.
    let mut negative_parts: Vec<String> = Vec::new();
    negative_parts.extend(ctx.defaults.negative.clone());
    for slug in style_slugs
        .iter()
        .chain(location.iter())
        .chain(&other_world)
    {
        if let Some(entry) = ctx.world.get(slug) {
            negative_parts.extend(entry.prompt.as_ref().and_then(|p| p.negative.clone()));
        }
    }
    for entry in &cast {
        if let Some(character) = ctx.characters.get(&entry.character) {
            negative_parts.extend(character.prompt.as_ref().and_then(|p| p.negative.clone()));
        }
    }
    negative_parts.extend(shot.negative_extra.clone());
    let negative = (!negative_parts.is_empty()).then(|| negative_parts.join(", "));

    // Identity attachments — always, regardless of prompt text.
    let mut reference_images = Vec::new();
    let mut adapters = Vec::new();
    for entry in &cast {
        if let Some(visual) = ctx
            .characters
            .get(&entry.character)
            .and_then(|c| c.visual.as_ref())
        {
            collect_visual(
                &entry.character,
                visual,
                &mut reference_images,
                &mut adapters,
            );
        }
    }
    for slug in location
        .iter()
        .chain(other_world.iter())
        .chain(style_slugs.iter())
    {
        if let Some(visual) = ctx.world.get(slug).and_then(|w| w.visual.as_ref()) {
            collect_visual(slug, visual, &mut reference_images, &mut adapters);
        }
    }

    warnings.sort();
    warnings.dedup();

    ResolvedPrompt {
        prompt,
        negative,
        reference_images,
        adapters,
        warnings,
    }
}

/// Adapter trigger tokens, then the fragment text, comma-joined.
fn segments_with_triggers(visual: Option<&Visual>, fragment: Option<String>) -> Option<String> {
    let mut segments: Vec<String> = Vec::new();
    if let Some(visual) = visual {
        segments.extend(visual.adapters.iter().filter_map(|a| a.trigger.clone()));
    }
    segments.extend(fragment);
    (!segments.is_empty()).then(|| segments.join(", "))
}

fn join_fragments(
    slugs: &[Slug],
    fragment_of: &impl Fn(&Slug, &mut Vec<String>) -> Option<String>,
    warnings: &mut Vec<String>,
) -> String {
    slugs
        .iter()
        .filter_map(|s| fragment_of(s, warnings))
        .collect::<Vec<_>>()
        .join("\n")
}

fn collect_visual(
    owner: &Slug,
    visual: &Visual,
    reference_images: &mut Vec<OwnedRef>,
    adapters: &mut Vec<OwnedAdapter>,
) {
    reference_images.extend(visual.reference_images.iter().map(|path| OwnedRef {
        owner: owner.clone(),
        path: path.clone(),
    }));
    adapters.extend(visual.adapters.iter().map(|adapter| OwnedAdapter {
        owner: owner.clone(),
        adapter: adapter.clone(),
    }));
}

enum Part<'t> {
    Literal(&'t str),
    Slot(&'t str),
}

/// Split one template line into literal runs and recognized slots. Unknown
/// `{token}`s stay literal; their names are reported so typos like
/// `{actoin}` surface (but only for templates that use slots at all —
/// braces in a fully literal prompt are prose).
fn tokenize_line<'t>(line: &'t str, unknown: &mut Vec<String>) -> Vec<Part<'t>> {
    let mut parts = Vec::new();
    let mut rest = line;
    while let Some(open) = rest.find('{') {
        let Some(close_rel) = rest[open + 1..].find('}') else {
            break;
        };
        let name = &rest[open + 1..open + 1 + close_rel];
        let token_like = (1..=30).contains(&name.len())
            && name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
        let lit_end = open + 1 + close_rel + 1;
        if token_like && KNOWN_SLOTS.contains(&name) {
            if open > 0 {
                parts.push(Part::Literal(&rest[..open]));
            }
            parts.push(Part::Slot(name));
        } else {
            if token_like {
                unknown.push(name.to_owned());
            }
            parts.push(Part::Literal(&rest[..lit_end]));
        }
        rest = &rest[lit_end..];
    }
    if !rest.is_empty() {
        parts.push(Part::Literal(rest));
    }
    parts
}

/// Single-pass render. Slot values are spliced verbatim and never
/// re-scanned. Separator literals directly after an empty slot are
/// absorbed; lines whose slots all resolve empty are dropped; a template
/// with no recognized slots is a literal prompt and passes through intact.
fn render_template(
    template: &str,
    values: &BTreeMap<&str, String>,
    warnings: &mut Vec<String>,
) -> String {
    let mut unknown_tokens = Vec::new();
    let mut rendered: Vec<(String, bool, bool)> = Vec::new(); // (text, had_slot, all_slots_empty)
    let mut any_slot = false;

    for line in template.lines() {
        let parts = tokenize_line(line, &mut unknown_tokens);
        let mut assembled = String::new();
        let mut had_slot = false;
        let mut all_empty = true;
        let mut prev_slot_empty = false;
        let mut leading_empty_slot = false;
        let mut first = true;
        for part in &parts {
            match part {
                Part::Literal(lit) => {
                    if !(prev_slot_empty && is_separator_only(lit)) {
                        assembled.push_str(lit);
                        prev_slot_empty = false;
                    }
                }
                Part::Slot(name) => {
                    had_slot = true;
                    any_slot = true;
                    let value = values.get(*name).map(String::as_str).unwrap_or("");
                    if value.is_empty() {
                        prev_slot_empty = true;
                        if first {
                            leading_empty_slot = true;
                        }
                    } else {
                        all_empty = false;
                        prev_slot_empty = false;
                        assembled.push_str(value);
                    }
                }
            }
            first = false;
        }
        if leading_empty_slot {
            assembled = assembled.trim_start().to_owned();
        }
        rendered.push((assembled, had_slot, all_empty));
    }

    if !any_slot {
        // Literal prompt: no cleanup, no placeholder warnings on prose braces.
        return template.trim().to_owned();
    }
    for token in unknown_tokens {
        warnings.push(format!(
            "unknown placeholder `{{{token}}}` in prompt template"
        ));
    }

    let mut out: Vec<String> = Vec::new();
    for (assembled, had_slot, all_empty) in rendered {
        if had_slot && all_empty {
            continue; // nothing resolved on this line, drop it (labels too)
        }
        let text = if had_slot {
            assembled.trim_end().to_owned()
        } else {
            assembled
        };
        if text.trim().is_empty() {
            if out.last().is_some_and(|l| !l.trim().is_empty()) {
                out.push(String::new());
            }
            continue;
        }
        out.push(text);
    }
    while out.last().is_some_and(|l| l.trim().is_empty()) {
        out.pop();
    }
    out.join("\n")
}

fn is_separator_only(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| ",.;:| -".contains(c))
}
