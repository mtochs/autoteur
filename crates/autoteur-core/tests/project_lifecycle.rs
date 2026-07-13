//! Project lifecycle: scaffold → populate → scan → validate → save points.

use std::fs;

use autoteur_core::error::Error;
use autoteur_core::git;
use autoteur_core::lint::Severity;
use autoteur_core::project::{self, Project};
use autoteur_core::schema::project::ProjectFormat;
use autoteur_core::schema::world::WorldKind;

const BEATS: &str = include_str!("fixtures/beats.toml");
const SCENE: &str = include_str!("fixtures/scene.toml");
const SHOTS: &str = include_str!("fixtures/shots.toml");
const CHARACTER: &str = include_str!("fixtures/character.toml");
const WORLD: &str = include_str!("fixtures/world.toml");

fn fresh_project(title: &str, format: ProjectFormat) -> (tempfile::TempDir, Project) {
    let dir = tempfile::tempdir().expect("tempdir");
    let project = Project::create(dir.path(), title, format).expect("create project");
    (dir, project)
}

#[test]
fn create_scaffolds_a_complete_agent_ready_project() {
    let (dir, project) = fresh_project("Cold Signal", ProjectFormat::Series);
    let root = dir.path();

    for file in [
        "autoteur.toml",
        "AGENTS.md",
        ".gitignore",
        ".gitattributes",
        "takes.manifest.toml",
        "timeline.toml",
        "story/logline.md",
        "story/treatment.md",
        "story/beats.toml",
    ] {
        assert!(root.join(file).exists(), "{file} missing from scaffold");
    }
    for dir_name in ["scenes", "characters", "world", "takes"] {
        assert!(root.join(dir_name).is_dir(), "{dir_name}/ missing");
    }

    // AGENTS.md carries the load-bearing rules.
    let agents = fs::read_to_string(root.join("AGENTS.md")).expect("read AGENTS.md");
    for rule in [
        "File order IS the order",
        "selected_take",
        "schema_version",
        "autoteur validate",
        "UTF-8",
    ] {
        assert!(agents.contains(rule), "AGENTS.md must mention {rule:?}");
    }

    // The manifest parses with the requested title/format.
    let scan = project.scan();
    assert!(scan.issues.is_empty(), "{:?}", scan.issues);
    let manifest = scan.state.manifest.expect("manifest parsed");
    assert_eq!(manifest.data.title, "Cold Signal");
    assert_eq!(manifest.data.format, ProjectFormat::Series);
    assert!(manifest.lints.is_empty());

    // git is initialized with an initial save point.
    let history = git::history(root, 10).expect("history");
    assert_eq!(history.len(), 1);
    assert!(history[0].summary.contains("Cold Signal"));

    // The takes manifest merges as a union across branches.
    let attributes = fs::read_to_string(root.join(".gitattributes")).expect("attributes");
    assert!(attributes.contains("takes.manifest.toml merge=union"));
}

#[test]
fn scan_reads_a_populated_project_in_order() {
    let (dir, project) = fresh_project("Cold Signal", ProjectFormat::Series);
    let root = dir.path();

    fs::write(root.join("story/beats.toml"), BEATS).expect("beats");
    let scene_dir = root.join("scenes/012-vault-breach");
    fs::create_dir_all(&scene_dir).expect("scene dir");
    fs::write(scene_dir.join("scene.toml"), SCENE).expect("scene");
    fs::write(scene_dir.join("shots.toml"), SHOTS).expect("shots");
    let later = root.join("scenes/002-cold-open");
    fs::create_dir_all(&later).expect("scene dir 2");
    fs::write(
        later.join("scene.toml"),
        "schema_version = 1\ntitle = \"Cold open\"\n",
    )
    .expect("scene 2");
    fs::write(root.join("characters/mara-chen.toml"), CHARACTER).expect("character");
    fs::write(root.join("world/halcyon-vault.toml"), WORLD).expect("world");

    let scan = project.scan();
    assert!(scan.issues.is_empty(), "{:?}", scan.issues);
    let state = scan.state;

    // Numeric order: 002 before 012 (and 1000 would sort after 990).
    let slugs: Vec<&str> = state.scenes.iter().map(|s| s.slug.as_str()).collect();
    assert_eq!(slugs, ["cold-open", "vault-breach"]);
    assert_eq!(state.beats.as_ref().expect("beats").data.beats.len(), 3);
    assert!(state.characters.keys().any(|k| k.as_str() == "mara-chen"));
    assert!(state.world.keys().any(|k| k.as_str() == "halcyon-vault"));

    let vault = &state.scenes[1];
    let shots = vault.shots.as_ref().expect("shots parsed");
    assert_eq!(shots.data.shots.len(), 3);
    assert!(shots.lints.is_empty());
}

#[test]
fn validate_finds_dangling_references_and_collisions() {
    let (dir, project) = fresh_project("Cold Signal", ProjectFormat::Series);
    let root = dir.path();

    fs::write(root.join("story/beats.toml"), BEATS).expect("beats");
    let scene_dir = root.join("scenes/012-vault-breach");
    fs::create_dir_all(&scene_dir).expect("scene dir");
    fs::write(scene_dir.join("scene.toml"), SCENE).expect("scene");
    fs::write(scene_dir.join("shots.toml"), SHOTS).expect("shots");
    fs::write(root.join("characters/mara-chen.toml"), CHARACTER).expect("character");
    fs::write(root.join("world/halcyon-vault.toml"), WORLD).expect("world");

    let lints = project::validate(&project.scan().state);

    // The fixture casts june-park (not created) and circles a take that is
    // not in the manifest; both must surface as warnings, not errors.
    assert!(
        lints
            .iter()
            .any(|l| l.severity == Severity::Warning && l.message.contains("june-park")),
        "{lints:?}"
    );
    assert!(
        lints
            .iter()
            .any(|l| l.message.contains("tk_3f9c2a8b41de") && l.message.contains("manifest")),
        "{lints:?}"
    );
    // crew-drill-rig world entry was never created either.
    assert!(lints.iter().any(|l| l.message.contains("crew-drill-rig")));
    assert!(!lints.iter().any(|l| l.severity == Severity::Error));

    // A second directory claiming the same slug is an identity collision.
    let clash = root.join("scenes/030-vault-breach");
    fs::create_dir_all(&clash).expect("clash dir");
    fs::write(
        clash.join("scene.toml"),
        "schema_version = 1\ntitle = \"Clash\"\n",
    )
    .expect("clash scene");
    let lints = project::validate(&project.scan().state);
    assert!(
        lints
            .iter()
            .any(|l| l.severity == Severity::Error && l.message.contains("vault-breach")),
        "{lints:?}"
    );
}

#[test]
fn scene_and_entity_scaffolds_mint_valid_slugs() {
    let (dir, project) = fresh_project("Untitled", ProjectFormat::Feature);
    let root = dir.path();

    let first = project.create_scene("The Vault Job!").expect("scene 1");
    assert!(first.ends_with("010-the-vault-job"), "{first:?}");
    let second = project.create_scene("The Vault Job!").expect("scene 2");
    assert!(second.ends_with("020-the-vault-job-2"), "{second:?}");

    let character = project.create_character("Mara Chen").expect("character");
    assert!(character.ends_with("mara-chen.toml"));
    assert!(matches!(
        project.create_character("Mara Chen"),
        Err(Error::Project(_))
    ));
    let world = project
        .create_world("The Halcyon Vault", WorldKind::Location)
        .expect("world");
    assert!(world.ends_with("the-halcyon-vault.toml"));

    // Everything scaffolded parses clean.
    let scan = project.scan();
    assert!(scan.issues.is_empty(), "{:?}", scan.issues);
    assert_eq!(scan.state.scenes.len(), 2);
    let mut all_lints = Vec::new();
    for scene in &scan.state.scenes {
        if let Some(file) = &scene.scene {
            all_lints.extend(file.lints.iter().cloned());
        }
        if let Some(file) = &scene.shots {
            all_lints.extend(file.lints.iter().cloned());
        }
    }
    assert!(all_lints.is_empty(), "{all_lints:?}");
    assert!(root.join("scenes/010-the-vault-job/shots.toml").exists());
}

#[test]
fn save_points_summarize_restore_and_never_lose_history() {
    let (dir, project) = fresh_project("Cold Signal", ProjectFormat::Feature);
    let root = dir.path();

    fs::write(
        root.join("story/treatment.md"),
        "# Cold Signal\n\nAct one begins in the rain.\n",
    )
    .expect("treatment");
    let first = git::save_point(root, None).expect("save point");
    let history = git::history(root, 10).expect("history");
    assert_eq!(history.len(), 2);
    assert!(
        history[0].summary.contains("Treatment updated"),
        "auto message should read plainly: {}",
        history[0].summary
    );

    // Nothing changed → same save point comes back, no empty commit.
    let again = git::save_point(root, None).expect("noop save");
    assert_eq!(again, first);

    // Change again, then restore the earlier state as a NEW save point.
    fs::write(root.join("story/treatment.md"), "# Rewritten\n").expect("rewrite");
    git::save_point(root, Some("Rewrote everything")).expect("save 3");
    git::restore(root, &first).expect("restore");
    let text = fs::read_to_string(root.join("story/treatment.md")).expect("read");
    assert!(text.contains("Act one begins in the rain."));
    let history = git::history(root, 10).expect("history");
    assert_eq!(history.len(), 4, "restore adds history, never rewrites it");
    assert!(history[0].summary.starts_with("Restored"));

    let _ = project;
}

#[test]
fn utf16_files_get_an_actionable_issue() {
    let (dir, project) = fresh_project("Cold Signal", ProjectFormat::Feature);
    let root = dir.path();

    // What PowerShell 5.1 `>` redirection produces: UTF-16LE with BOM.
    let mut bytes = vec![0xFF, 0xFE];
    for unit in "schema_version = 1\n".encode_utf16() {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    fs::write(root.join("story/beats.toml"), bytes).expect("write utf16");

    let scan = project.scan();
    let issue = scan
        .issues
        .iter()
        .find(|i| i.path.ends_with("beats.toml"))
        .expect("issue for the UTF-16 file");
    assert!(
        issue.message.contains("UTF-16"),
        "hint must name the real problem: {}",
        issue.message
    );
    assert!(scan.state.beats.is_none());
}
