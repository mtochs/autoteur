//! Prompt resolution: the pure function from committed files to the exact
//! generation request. Computed live for the compose view, snapshotted
//! per-take into the manifest, never written back into authored files.
//!
//! The load-bearing rule: prompt text and identity assets are separate
//! channels. Reference images and adapters attach from the effective cast
//! and world even under a fully literal shot prompt.

use std::collections::{BTreeMap, BTreeSet};

use crate::id::{CastEntry, Slug};
use crate::schema::character::CharacterFile;
use crate::schema::common::Adapter;
use crate::schema::project::Defaults;
use crate::schema::scene::SceneFile;
use crate::schema::shots::Shot;
use crate::schema::world::{WorldFile, WorldKind};

/// Used when neither the shot nor the project defines a template.
pub const DEFAULT_PROMPT_TEMPLATE: &str =
    "{style}\n{framing}. {action}\n{characters}\n{location}\n{world}\n{mood}\n{extra}";

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

    let world_fragment = |slug: &Slug, warnings: &mut Vec<String>| -> Option<String> {
        match ctx.world.get(slug) {
            Some(entry) => entry.prompt.as_ref().and_then(|p| p.fragment.clone()),
            None => {
                warnings.push(format!("unknown world entry `{slug}`"));
                None
            }
        }
    };

    let style_text = join_fragments(&style_slugs, &world_fragment, &mut warnings);
    let world_text = join_fragments(&other_world, &world_fragment, &mut warnings);
    let location_text = location
        .as_ref()
        .and_then(|slug| world_fragment(slug, &mut warnings))
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
        let mut segments: Vec<String> = Vec::new();
        if let Some(visual) = &character.visual {
            segments.extend(visual.adapters.iter().filter_map(|a| a.trigger.clone()));
        }
        segments.extend(text);
        if !segments.is_empty() {
            character_lines.push(segments.join(", "));
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

    let substituted = template
        .replace("{style}", &style_text)
        .replace("{framing}", &framing_text)
        .replace("{camera}", shot.camera.as_deref().unwrap_or("").trim())
        .replace("{action}", shot.action.as_deref().unwrap_or("").trim())
        .replace("{characters}", &characters_text)
        .replace("{location}", &location_text)
        .replace("{world}", &world_text)
        .replace("{mood}", ctx.scene.mood.as_deref().unwrap_or(""))
        .replace("{dialogue}", &dialogue_text)
        .replace("{extra}", shot.prompt_extra.as_deref().unwrap_or(""));

    for placeholder in find_placeholders(&substituted) {
        warnings.push(format!(
            "unresolved placeholder `{{{placeholder}}}` left in prompt"
        ));
    }

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
        prompt: tidy(&substituted),
        negative,
        reference_images,
        adapters,
        warnings,
    }
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
    visual: &crate::schema::common::Visual,
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

/// Collapse the artifacts of empty placeholder slots: runs of spaces, lines
/// reduced to orphaned leading punctuation, and stacked blank lines.
fn tidy(text: &str) -> String {
    let mut out: Vec<String> = Vec::new();
    for raw in text.lines() {
        let mut line = raw.trim().to_owned();
        while line.contains("  ") {
            line = line.replace("  ", " ");
        }
        let stripped = line.trim_start_matches(|c: char| ",.;: ".contains(c));
        let line = if stripped.len() != line.len() {
            stripped.to_owned()
        } else {
            line
        };
        if line.is_empty() {
            if out.last().is_some_and(|l| !l.is_empty()) {
                out.push(String::new());
            }
        } else {
            out.push(line);
        }
    }
    while out.last().is_some_and(String::is_empty) {
        out.pop();
    }
    out.join("\n")
}

fn find_placeholders(text: &str) -> Vec<String> {
    let mut found = Vec::new();
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'{' {
            if let Some(end) = text[i + 1..].find('}') {
                let inner = &text[i + 1..i + 1 + end];
                if (1..=30).contains(&inner.len())
                    && inner
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
                {
                    found.push(inner.to_owned());
                }
                i += end + 2;
                continue;
            }
        }
        i += 1;
    }
    found
}
