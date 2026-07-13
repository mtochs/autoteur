//! Round-trip tests: the fixtures are the canonical example files from the
//! format proposal. Parsing must see exactly the documented semantics, and
//! surgical edits must leave every unrelated byte (especially comments)
//! untouched.

use autoteur_core::doc;
use autoteur_core::schema::beats::BeatsFile;
use autoteur_core::schema::character::CharacterFile;
use autoteur_core::schema::project::{ProjectFile, ProjectFormat};
use autoteur_core::schema::scene::SceneFile;
use autoteur_core::schema::shots::{ShotStatus, ShotsFile};
use autoteur_core::schema::world::{WorldFile, WorldKind};
use autoteur_core::toml_edit::Value;

const BEATS: &str = include_str!("fixtures/beats.toml");
const SCENE: &str = include_str!("fixtures/scene.toml");
const SHOTS: &str = include_str!("fixtures/shots.toml");
const CHARACTER: &str = include_str!("fixtures/character.toml");
const WORLD: &str = include_str!("fixtures/world.toml");
const PROJECT: &str = include_str!("fixtures/autoteur.toml");

#[test]
fn beats_fixture_parses_with_file_order() {
    let (beats, _) = doc::parse::<BeatsFile>(BEATS).expect("beats fixture parses");
    assert_eq!(beats.schema_version, 1);
    let episode_ids: Vec<_> = beats.episodes.iter().map(|e| e.id.as_str()).collect();
    assert_eq!(episode_ids, ["e01", "e02"]);
    assert_eq!(beats.episodes[0].color.as_deref(), Some("sky"));

    let beat_ids: Vec<_> = beats.beats.iter().map(|b| b.id.as_str()).collect();
    assert_eq!(
        beat_ids,
        ["cold-open-heist", "midpoint-betrayal", "e02-fallout"]
    );
    assert_eq!(beats.beats[0].act, Some(1));
    assert_eq!(beats.beats[1].color.as_deref(), Some("amber"));
    assert!(beats.beats[0].color.is_none());
    assert_eq!(
        beats.beats[1].episode.as_ref().map(|s| s.as_str()),
        Some("e01")
    );
}

#[test]
fn scene_fixture_parses() {
    let (scene, _) = doc::parse::<SceneFile>(SCENE).expect("scene fixture parses");
    assert_eq!(scene.title, "Breaching the Halcyon vault");
    assert_eq!(scene.beats.len(), 1);
    let cast: Vec<_> = scene.characters.iter().map(|c| c.as_str()).collect();
    assert_eq!(cast, ["mara-chen", "june-park"]);
    assert_eq!(
        scene.location.as_ref().map(|l| l.as_str()),
        Some("halcyon-vault")
    );
    assert_eq!(scene.int_ext.as_deref(), Some("INT"));
    assert!(scene
        .director_notes
        .as_deref()
        .expect("notes present")
        .contains("Whisper volume"));
}

#[test]
fn shots_fixture_parses_with_documented_semantics() {
    let (shots, _) = doc::parse::<ShotsFile>(SHOTS).expect("shots fixture parses");
    assert_eq!(shots.shots.len(), 3);

    let a = &shots.shots[0];
    assert_eq!(a.id.as_str(), "a");
    // Absent characters = inherit the scene cast.
    assert!(a.characters.is_none());
    assert_eq!(a.status, ShotStatus::Locked);
    assert_eq!(
        a.selected_take.as_ref().map(|t| t.as_str()),
        Some("tk_3f9c2a8b41de")
    );
    assert_eq!(a.duration_s, Some(6.0));
    assert_eq!(
        a.camera.as_deref(),
        Some("slow push-in from the breached blast door")
    );

    let b = &shots.shots[1];
    let cast = b.characters.as_ref().expect("explicit cast");
    assert_eq!(cast.len(), 2);
    assert_eq!(b.dialogue.len(), 3);
    assert_eq!(b.dialogue[0].character.as_str(), "mara-chen");
    assert_eq!(b.dialogue[1].delivery.as_deref(), Some("too calm"));
    assert!(b.selected_take.is_none());

    let c = &shots.shots[2];
    // Explicit empty list = nobody in frame (distinct from absent).
    assert_eq!(c.characters.as_deref(), Some(&[][..]));
    let world = c.world.as_ref().expect("world override");
    assert_eq!(world[0].as_str(), "crew-drill-rig");
    assert!(c
        .prompt
        .as_deref()
        .expect("custom template")
        .contains("{style}"));
    assert_eq!(
        c.negative_extra.as_deref(),
        Some("blurry digits, text artifacts")
    );
}

#[test]
fn character_fixture_parses() {
    let (character, _) = doc::parse::<CharacterFile>(CHARACTER).expect("character fixture parses");
    assert_eq!(character.name, "Mara Chen");
    assert_eq!(character.aliases, ["The Locksmith"]);
    let voice = character.voice.as_ref().expect("voice");
    assert_eq!(voice.provider.as_deref(), Some("elevenlabs"));
    let prompt = character.prompt.as_ref().expect("prompt");
    assert!(prompt
        .fragment
        .as_deref()
        .expect("fragment")
        .contains("gray streak"));
    assert!(prompt.variants.keys().any(|k| k.as_str() == "storm-gear"));
    let visual = character.visual.as_ref().expect("visual");
    assert_eq!(visual.reference_images.len(), 2);
    assert_eq!(visual.adapters.len(), 1);
    assert_eq!(visual.adapters[0].weight, Some(0.85));
    assert_eq!(
        visual.adapters[0].trigger.as_deref(),
        Some("m4rachen woman")
    );
}

#[test]
fn world_and_project_fixtures_parse() {
    let (world, _) = doc::parse::<WorldFile>(WORLD).expect("world fixture parses");
    assert_eq!(world.kind, WorldKind::Location);
    assert!(world
        .prompt
        .as_ref()
        .and_then(|p| p.fragment.as_deref())
        .expect("fragment")
        .contains("blast door"));

    let (project, _) = doc::parse::<ProjectFile>(PROJECT).expect("project fixture parses");
    assert_eq!(project.format, ProjectFormat::Series);
    assert_eq!(project.defaults.style.len(), 1);
    assert!(project.defaults.prompt_template.is_some());
}

#[test]
fn field_edit_changes_exactly_one_line_and_keeps_comments() {
    let (_, mut document) = doc::parse::<ShotsFile>(SHOTS).expect("parse");
    doc::set_block_field(&mut document, "shots", 1, "status", Value::from("locked")).expect("edit");
    let edited = document.to_string();

    let (reparsed, _) = doc::parse::<ShotsFile>(&edited).expect("reparse");
    assert_eq!(reparsed.shots[1].status, ShotStatus::Locked);

    let before: Vec<&str> = SHOTS.lines().collect();
    let after: Vec<&str> = edited.lines().collect();
    assert_eq!(before.len(), after.len(), "line count must not change");
    let changed: Vec<usize> = (0..before.len())
        .filter(|&i| before[i] != after[i])
        .collect();
    assert_eq!(changed.len(), 1, "exactly one line changes: {changed:?}");

    // The in-file spec comments survive the edit.
    assert!(edited.contains("# planned | ready | locked | omitted"));
    assert!(edited.contains("Shot order = block order"));
}

#[test]
fn circle_and_uncircle_are_key_presence() {
    let (_, mut document) = doc::parse::<ShotsFile>(SHOTS).expect("parse");

    // Un-circle shot a: delete the line.
    let removed =
        doc::remove_block_field(&mut document, "shots", 0, "selected_take").expect("remove");
    assert!(removed);
    let (reparsed, _) = doc::parse::<ShotsFile>(&document.to_string()).expect("reparse");
    assert!(reparsed.shots[0].selected_take.is_none());

    // Circle shot b: add one line.
    doc::set_block_field(
        &mut document,
        "shots",
        1,
        "selected_take",
        Value::from("tk_aaaaaaaaaaaa"),
    )
    .expect("set");
    let (reparsed, _) = doc::parse::<ShotsFile>(&document.to_string()).expect("reparse");
    assert_eq!(
        reparsed.shots[1].selected_take.as_ref().map(|t| t.as_str()),
        Some("tk_aaaaaaaaaaaa")
    );
}

#[test]
fn drag_reorder_moves_whole_blocks_and_keeps_their_comments() {
    let (_, mut document) = doc::parse::<BeatsFile>(BEATS).expect("parse");
    doc::move_block(&mut document, "beats", 0, 2).expect("move");
    let moved = document.to_string();

    let (reparsed, _) = doc::parse::<BeatsFile>(&moved).expect("reparse");
    let ids: Vec<_> = reparsed.beats.iter().map(|b| b.id.as_str()).collect();
    assert_eq!(ids, ["midpoint-betrayal", "e02-fallout", "cold-open-heist"]);

    // Episodes are untouched; the file header and block comments survive.
    assert_eq!(reparsed.episodes.len(), 2);
    assert!(moved.starts_with("# story/beats.toml"));
    assert!(moved.contains("# optional manual tint"));
    assert!(moved.contains("kebab-case, unique in this file"));
}

#[test]
fn malformed_toml_is_a_syntax_error() {
    assert!(doc::parse::<ShotsFile>("[[shots]\nid = \"a\"").is_err());
}

#[test]
fn wrong_types_are_schema_errors_not_panics() {
    assert!(doc::parse::<SceneFile>("title = 3").is_err());
    assert!(doc::parse::<ShotsFile>("[[shots]]\nid = \"NOT-VALID\"").is_err());
    assert!(doc::parse::<ShotsFile>("[[shots]]\nid = \"a\"\nselected_take = \"\"").is_err());
}

#[test]
fn lenient_numbers_accept_integer_durations() {
    let (shots, _) =
        doc::parse::<ShotsFile>("[[shots]]\nid = \"a\"\nduration_s = 6").expect("parse");
    assert_eq!(shots.shots[0].duration_s, Some(6.0));
}
