//! Prompt-resolution tests: injection order, inheritance semantics, the
//! text-vs-identity separation, and graceful degradation on dangling refs.

use std::collections::BTreeMap;

use autoteur_core::id::{CastEntry, ShotId, Slug};
use autoteur_core::prompt::{expand_framing, resolve, PromptContext};
use autoteur_core::schema::character::CharacterFile;
use autoteur_core::schema::common::{Adapter, AdapterKind, PromptFragments, Visual};
use autoteur_core::schema::project::Defaults;
use autoteur_core::schema::scene::SceneFile;
use autoteur_core::schema::shots::{DialogueCue, Shot, ShotStatus};
use autoteur_core::schema::world::{WorldFile, WorldKind};

fn slug(s: &str) -> Slug {
    Slug::new(s).expect("valid slug")
}

fn character(name: &str, fragment: &str, negative: Option<&str>) -> CharacterFile {
    CharacterFile {
        schema_version: 1,
        name: name.to_owned(),
        aliases: vec![],
        description: None,
        voice: None,
        prompt: Some(PromptFragments {
            fragment: Some(fragment.to_owned()),
            negative: negative.map(str::to_owned),
            variants: BTreeMap::new(),
        }),
        visual: None,
    }
}

fn world_entry(name: &str, kind: WorldKind, fragment: &str) -> WorldFile {
    WorldFile {
        schema_version: 1,
        name: name.to_owned(),
        kind,
        description: None,
        prompt: Some(PromptFragments {
            fragment: Some(fragment.to_owned()),
            negative: None,
            variants: BTreeMap::new(),
        }),
        visual: None,
    }
}

fn shot(id: &str) -> Shot {
    Shot {
        id: ShotId::new(id).expect("valid shot id"),
        framing: None,
        camera: None,
        action: None,
        characters: None,
        world: None,
        dialogue: vec![],
        duration_s: None,
        status: ShotStatus::default(),
        selected_take: None,
        prompt: None,
        prompt_extra: None,
        negative_extra: None,
        notes: None,
    }
}

struct Fixture {
    defaults: Defaults,
    scene: SceneFile,
    characters: BTreeMap<Slug, CharacterFile>,
    world: BTreeMap<Slug, WorldFile>,
}

impl Fixture {
    fn new() -> Self {
        let mut mara = character(
            "Mara Chen",
            "Mara Chen, sharp jaw, gray streak, dark utility jacket",
            Some("glamour makeup"),
        );
        if let Some(prompt) = &mut mara.prompt {
            prompt.variants.insert(
                slug("storm-gear"),
                "Mara Chen, black storm poncho, rain-plastered hair".to_owned(),
            );
        }
        mara.visual = Some(Visual {
            reference_images: vec!["characters/refs/mara-chen/front.png".to_owned()],
            adapters: vec![Adapter {
                kind: AdapterKind::Lora,
                source: "civitai:123456".to_owned(),
                weight: Some(0.85),
                trigger: Some("m4rachen".to_owned()),
                token: None,
            }],
        });
        let june = character("June Park", "June Park, buzzed hair, gray coverall", None);

        let mut characters = BTreeMap::new();
        characters.insert(slug("mara-chen"), mara);
        characters.insert(slug("june-park"), june);

        let mut world = BTreeMap::new();
        world.insert(
            slug("halcyon-vault"),
            world_entry(
                "The Halcyon Vault",
                WorldKind::Location,
                "the Halcyon vault antechamber, circular blast door",
            ),
        );
        world.insert(
            slug("crew-drill-rig"),
            world_entry("Drill rig", WorldKind::Prop, "a tripod-mounted drill rig"),
        );
        world.insert(
            slug("neon-noir-style"),
            world_entry(
                "Neon noir",
                WorldKind::Style,
                "neo-noir, high contrast, cinematic film still",
            ),
        );

        Self {
            defaults: Defaults {
                prompt_template: None,
                negative: Some("watermark".to_owned()),
                style: vec![slug("neon-noir-style")],
            },
            scene: SceneFile {
                schema_version: 1,
                title: "Vault".to_owned(),
                beats: vec![],
                characters: vec![slug("mara-chen"), slug("june-park")],
                location: Some(slug("halcyon-vault")),
                world: vec![slug("crew-drill-rig")],
                int_ext: None,
                time: None,
                mood: Some("airless, blue-lit".to_owned()),
                synopsis: None,
                director_notes: None,
            },
            characters,
            world,
        }
    }

    fn ctx(&self) -> PromptContext<'_> {
        PromptContext {
            defaults: &self.defaults,
            scene: &self.scene,
            characters: &self.characters,
            world: &self.world,
        }
    }
}

#[test]
fn absent_characters_inherit_the_scene_cast() {
    let f = Fixture::new();
    let mut s = shot("a");
    s.action = Some("Two figures at the vault door.".to_owned());
    let resolved = resolve(&f.ctx(), &s);
    assert!(resolved.prompt.contains("Mara Chen"));
    assert!(resolved.prompt.contains("June Park"));
    // Injection order = scene cast order.
    let mara_at = resolved.prompt.find("Mara Chen").expect("mara injected");
    let june_at = resolved.prompt.find("June Park").expect("june injected");
    assert!(mara_at < june_at);
    assert!(resolved.warnings.is_empty(), "{:?}", resolved.warnings);
}

#[test]
fn empty_cast_means_nobody() {
    let f = Fixture::new();
    let mut s = shot("a");
    s.characters = Some(vec![]);
    let resolved = resolve(&f.ctx(), &s);
    assert!(!resolved.prompt.contains("Mara Chen"));
    assert!(!resolved
        .reference_images
        .iter()
        .any(|r| r.owner.as_str() == "mara-chen"));
}

#[test]
fn explicit_cast_is_exact() {
    let f = Fixture::new();
    let mut s = shot("a");
    s.characters = Some(vec![CastEntry::of(slug("june-park"))]);
    let resolved = resolve(&f.ctx(), &s);
    assert!(resolved.prompt.contains("June Park"));
    assert!(!resolved.prompt.contains("Mara Chen"));
}

#[test]
fn variant_pin_replaces_the_fragment() {
    let f = Fixture::new();
    let mut s = shot("a");
    s.characters = Some(vec!["mara-chen:storm-gear".parse().expect("cast entry")]);
    let resolved = resolve(&f.ctx(), &s);
    assert!(resolved.prompt.contains("storm poncho"));
    assert!(!resolved.prompt.contains("utility jacket"));
    // Identity assets still attach under a variant.
    assert!(resolved
        .reference_images
        .iter()
        .any(|r| r.owner.as_str() == "mara-chen"));
}

#[test]
fn literal_prompt_still_attaches_identity() {
    let f = Fixture::new();
    let mut s = shot("a");
    s.prompt = Some("a lone drill bit spinning down on concrete".to_owned());
    let resolved = resolve(&f.ctx(), &s);
    assert_eq!(
        resolved.prompt,
        "a lone drill bit spinning down on concrete"
    );
    // Cast is inherited, so Mara's reference image and LoRA still attach.
    assert!(resolved
        .reference_images
        .iter()
        .any(|r| r.owner.as_str() == "mara-chen"));
    assert!(resolved
        .adapters
        .iter()
        .any(|a| a.adapter.source == "civitai:123456"));
}

#[test]
fn world_override_replaces_location_and_world() {
    let f = Fixture::new();
    let mut s = shot("a");
    s.world = Some(vec![slug("crew-drill-rig")]);
    s.action = Some("Insert on the drill bit.".to_owned());
    let resolved = resolve(&f.ctx(), &s);
    assert!(!resolved.prompt.contains("Halcyon vault antechamber"));
    assert!(resolved.prompt.contains("drill rig"));
}

#[test]
fn framing_vocabulary_expands() {
    assert_eq!(expand_framing("ots"), "over-the-shoulder shot");
    assert_eq!(expand_framing("MCU"), "medium close-up");
    assert_eq!(expand_framing("dutch tilt"), "dutch tilt");

    let f = Fixture::new();
    let mut s = shot("a");
    s.framing = Some("ots".to_owned());
    s.action = Some("June watches.".to_owned());
    let resolved = resolve(&f.ctx(), &s);
    assert!(resolved.prompt.contains("over-the-shoulder shot"));
    assert!(!resolved.prompt.contains("ots"));
}

#[test]
fn negative_prompt_composes_in_order() {
    let f = Fixture::new();
    let mut s = shot("a");
    s.negative_extra = Some("blurry digits".to_owned());
    let resolved = resolve(&f.ctx(), &s);
    let negative = resolved.negative.expect("negative present");
    let watermark = negative.find("watermark").expect("project default");
    let makeup = negative.find("glamour makeup").expect("character negative");
    let blurry = negative.find("blurry digits").expect("shot extra");
    assert!(watermark < makeup && makeup < blurry, "{negative}");
}

#[test]
fn style_entries_feed_style_not_world() {
    let f = Fixture::new();
    let mut s = shot("a");
    s.prompt = Some("STYLE[{style}] WORLD[{world}]".to_owned());
    let resolved = resolve(&f.ctx(), &s);
    let style_section = resolved
        .prompt
        .split("WORLD[")
        .next()
        .expect("style section");
    assert!(style_section.contains("neo-noir"));
    assert!(!resolved.prompt["STYLE[".len()..]
        .split("WORLD[")
        .nth(1)
        .expect("world section")
        .contains("neo-noir"));
}

#[test]
fn dialogue_placeholder_formats_cues() {
    let f = Fixture::new();
    let mut s = shot("a");
    s.prompt = Some("{dialogue}".to_owned());
    s.dialogue = vec![DialogueCue {
        character: slug("mara-chen"),
        line: "Not in this building.".to_owned(),
        delivery: Some("flat".to_owned()),
    }];
    let resolved = resolve(&f.ctx(), &s);
    assert_eq!(
        resolved.prompt,
        "Mara Chen: \"Not in this building.\" (flat)"
    );
}

#[test]
fn trigger_tokens_prepend_to_the_fragment() {
    let f = Fixture::new();
    let mut s = shot("a");
    s.characters = Some(vec![CastEntry::of(slug("mara-chen"))]);
    s.prompt = Some("{characters}".to_owned());
    let resolved = resolve(&f.ctx(), &s);
    assert!(resolved.prompt.starts_with("m4rachen, Mara Chen"));
}

#[test]
fn dangling_references_warn_but_never_fail() {
    let f = Fixture::new();
    let mut s = shot("a");
    s.characters = Some(vec![CastEntry::of(slug("ghost"))]);
    s.world = Some(vec![slug("missing-prop")]);
    let resolved = resolve(&f.ctx(), &s);
    assert!(resolved
        .warnings
        .iter()
        .any(|w| w.contains("unknown character `ghost`")));
    assert!(resolved
        .warnings
        .iter()
        .any(|w| w.contains("unknown world entry `missing-prop`")));
}

#[test]
fn missing_variant_falls_back_with_warning() {
    let f = Fixture::new();
    let mut s = shot("a");
    s.characters = Some(vec!["mara-chen:nope".parse().expect("cast entry")]);
    let resolved = resolve(&f.ctx(), &s);
    assert!(
        resolved.prompt.contains("utility jacket"),
        "falls back to base"
    );
    assert!(resolved
        .warnings
        .iter()
        .any(|w| w.contains("no variant `nope`")));
}

#[test]
fn unresolved_placeholders_warn() {
    let f = Fixture::new();
    let mut s = shot("a");
    s.prompt = Some("something {wat} here".to_owned());
    let resolved = resolve(&f.ctx(), &s);
    assert!(resolved.warnings.iter().any(|w| w.contains("{wat}")));
}
