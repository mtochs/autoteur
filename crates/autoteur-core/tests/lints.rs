//! Lint tests: the traps the format panel verified with real parsers must
//! all be caught — silently-misplaced keys, duplicate ids, unknown enums.

use autoteur_core::doc;
use autoteur_core::lint::{self, has_errors, Severity};
use autoteur_core::schema::beats::BeatsFile;
use autoteur_core::schema::character::CharacterFile;
use autoteur_core::schema::scene::SceneFile;
use autoteur_core::schema::shots::ShotsFile;

const BEATS: &str = include_str!("fixtures/beats.toml");
const CHARACTER: &str = include_str!("fixtures/character.toml");

#[test]
fn eof_appended_root_key_is_flagged_on_the_swallowing_beat() {
    // TOML parses this cleanly but parks the key inside the LAST beat —
    // the exact silent-misplacement trap from the proposal.
    let text = format!("{BEATS}\ndefault_act_count = 3\n");
    let (data, document) = doc::parse::<BeatsFile>(&text).expect("still valid TOML");
    let lints = lint::lint_beats(&document, &data);
    let hit = lints
        .iter()
        .find(|l| l.message.contains("default_act_count"))
        .expect("misplaced key is flagged");
    assert!(hit.message.contains("e02-fallout"), "{}", hit.message);
}

#[test]
fn duplicate_ids_are_errors() {
    let text = "[[shots]]\nid = \"a\"\n\n[[shots]]\nid = \"a\"\n";
    let (data, document) = doc::parse::<ShotsFile>(text).expect("parses");
    let lints = lint::lint_shots(&document, &data);
    assert!(has_errors(&lints));
    assert!(lints
        .iter()
        .any(|l| l.severity == Severity::Error && l.message.contains("share the id")));

    let text = format!(
        "{}\n[[beats]]\nid = \"cold-open-heist\"\ntitle = \"dup\"\n",
        BEATS
    );
    let (data, document) = doc::parse::<BeatsFile>(&text).expect("parses");
    assert!(has_errors(&lint::lint_beats(&document, &data)));
}

#[test]
fn root_key_appended_to_character_file_is_flagged_as_misplaced() {
    // Appends after [[visual.adapters]] land inside the adapter table.
    let text = format!("{CHARACTER}\naliases = [\"Emcee\"]\n");
    let (data, document) = doc::parse::<CharacterFile>(&text).expect("still valid TOML");
    let lints = lint::lint_character(&document, &data);
    let hit = lints
        .iter()
        .find(|l| l.message.contains("top-level key `aliases`"))
        .expect("misplacement is flagged");
    assert!(hit.message.contains("visual.adapters"), "{}", hit.message);
}

#[test]
fn unknown_status_and_unknown_dialogue_key_warn() {
    let text = "[[shots]]\nid = \"a\"\nstatus = \"weird\"\ndialogue = [\n  { character = \"mara\", line = \"hi\", mode = \"vo\" },\n]\n";
    let (data, document) = doc::parse::<ShotsFile>(text).expect("parses");
    let lints = lint::lint_shots(&document, &data);
    assert!(lints.iter().any(|l| l.message.contains("unknown status")));
    assert!(lints
        .iter()
        .any(|l| l.message.contains("unknown key `mode`")));
    assert!(!has_errors(&lints), "warnings must not mark the file stale");
}

#[test]
fn missing_schema_version_warns() {
    let (data, document) = doc::parse::<SceneFile>("title = \"x\"").expect("parses");
    let lints = lint::lint_scene(&document, &data);
    assert!(lints
        .iter()
        .any(|l| l.message.contains("missing schema_version")));
}

#[test]
fn clean_fixtures_produce_no_lints() {
    let (data, document) = doc::parse::<BeatsFile>(BEATS).expect("parses");
    assert!(lint::lint_beats(&document, &data).is_empty());
    let (data, document) = doc::parse::<CharacterFile>(CHARACTER).expect("parses");
    assert!(lint::lint_character(&document, &data).is_empty());
}
