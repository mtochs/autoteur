//! Project scaffolding, scanning, and cross-file validation. `create`
//! writes a complete starter repo (including the generated AGENTS.md that
//! makes any coding agent productive here); `scan` builds the typed state
//! every consumer (CLI, watcher, GUI) reads; `validate` finds dangling
//! references across files.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;
use toml_edit::DocumentMut;

use crate::error::{Error, Result};
use crate::id::{ShotRef, Slug};
use crate::lint::{self, Lint, Severity};
use crate::schema::beats::BeatsFile;
use crate::schema::character::CharacterFile;
use crate::schema::project::{ProjectFile, ProjectFormat};
use crate::schema::scene::{parse_scene_dir_name, SceneFile};
use crate::schema::shots::ShotsFile;
use crate::schema::takes::TakesManifest;
use crate::schema::timeline::TimelineFile;
use crate::schema::world::{WorldFile, WorldKind};
use crate::{doc, git};

const T_AGENTS: &str = include_str!("../templates/AGENTS.md");
const T_MANIFEST: &str = include_str!("../templates/autoteur.toml");
const T_BEATS: &str = include_str!("../templates/beats.toml");
const T_SCENE: &str = include_str!("../templates/scene.toml");
const T_SHOTS: &str = include_str!("../templates/shots.toml");
const T_CHARACTER: &str = include_str!("../templates/character.toml");
const T_WORLD: &str = include_str!("../templates/world.toml");
const T_TAKES: &str = include_str!("../templates/takes.manifest.toml");
const T_TIMELINE: &str = include_str!("../templates/timeline.toml");
const T_GITIGNORE: &str = include_str!("../templates/project.gitignore");
const T_GITATTRIBUTES: &str = include_str!("../templates/project.gitattributes");

pub struct Project {
    root: PathBuf,
}

#[derive(Debug)]
pub struct FileEntry<T> {
    pub path: PathBuf,
    pub data: T,
    pub lints: Vec<Lint>,
}

#[derive(Debug)]
pub struct SceneEntry {
    pub number: u32,
    pub slug: Slug,
    pub dir: PathBuf,
    pub scene: Option<FileEntry<SceneFile>>,
    pub shots: Option<FileEntry<ShotsFile>>,
}

#[derive(Debug, Default)]
pub struct ProjectState {
    pub manifest: Option<FileEntry<ProjectFile>>,
    pub beats: Option<FileEntry<BeatsFile>>,
    /// Ordered by (number, slug) — numeric, not lexicographic.
    pub scenes: Vec<SceneEntry>,
    pub characters: BTreeMap<Slug, FileEntry<CharacterFile>>,
    pub world: BTreeMap<Slug, FileEntry<WorldFile>>,
    pub takes: Option<FileEntry<TakesManifest>>,
    pub timeline: Option<FileEntry<TimelineFile>>,
}

/// A file that could not be read or parsed at all (the state simply omits
/// it; the watcher layer keeps last-good data for display).
#[derive(Debug)]
pub struct FileIssue {
    pub path: PathBuf,
    pub message: String,
}

#[derive(Debug)]
pub struct ProjectScan {
    pub state: ProjectState,
    pub issues: Vec<FileIssue>,
}

/// A cross-file finding, anchored to the file that should change.
#[derive(Debug)]
pub struct ProjectLint {
    pub path: PathBuf,
    pub severity: Severity,
    pub message: String,
}

impl Project {
    /// Scaffold a new project: full directory layout, generated AGENTS.md,
    /// git init, and an initial save point. Refuses a directory that is
    /// already an Autoteur project; existing unrelated files are left alone.
    pub fn create(root: &Path, title: &str, format: ProjectFormat) -> Result<Project> {
        if root.join("autoteur.toml").exists() {
            return Err(Error::Project(format!(
                "{} is already an Autoteur project",
                root.display()
            )));
        }
        fs::create_dir_all(root).map_err(|e| Error::Io {
            path: root.to_owned(),
            source: e,
        })?;

        let format_str: String = format.into();
        let manifest = T_MANIFEST
            .replace("{{TITLE}}", &toml_escape(title))
            .replace("{{FORMAT}}", &format_str);

        write_if_absent(&root.join("autoteur.toml"), &manifest)?;
        write_if_absent(&root.join("AGENTS.md"), T_AGENTS)?;
        write_if_absent(&root.join(".gitignore"), T_GITIGNORE)?;
        write_if_absent(&root.join(".gitattributes"), T_GITATTRIBUTES)?;
        write_if_absent(&root.join("takes.manifest.toml"), T_TAKES)?;
        write_if_absent(&root.join("timeline.toml"), T_TIMELINE)?;
        write_if_absent(
            &root.join("story").join("logline.md"),
            "# Logline\n\n_One sentence: who wants what, and what stands in the way._\n",
        )?;
        write_if_absent(
            &root.join("story").join("treatment.md"),
            &format!("# {title}\n\n_The story, told in prose. Write it like you'd pitch it._\n"),
        )?;
        write_if_absent(&root.join("story").join("beats.toml"), T_BEATS)?;
        for dir in ["scenes", "characters", "world", "takes"] {
            let path = root.join(dir);
            fs::create_dir_all(&path).map_err(|e| Error::Io { path, source: e })?;
        }
        for dir in ["scenes", "characters", "world"] {
            write_if_absent(&root.join(dir).join(".gitkeep"), "")?;
        }

        git::init(root)?;
        git::save_point(root, Some(&format!("Project created: {title}")))?;
        Ok(Project {
            root: root.to_owned(),
        })
    }

    /// Open an existing project (the directory must contain autoteur.toml).
    pub fn open(root: &Path) -> Result<Project> {
        if !root.join("autoteur.toml").exists() {
            return Err(Error::Project(format!(
                "{} is not an Autoteur project (no autoteur.toml)",
                root.display()
            )));
        }
        Ok(Project {
            root: root.to_owned(),
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Create the next scene directory (gap-numbered) with template files.
    /// Returns the new directory path.
    pub fn create_scene(&self, title: &str) -> Result<PathBuf> {
        let scan = self.scan();
        let next_number = scan
            .state
            .scenes
            .iter()
            .map(|s| s.number)
            .max()
            .map(|n| n + 10)
            .unwrap_or(10);
        let base = slugify(title)?;
        let slug = disambiguate(&base, |candidate| {
            scan.state
                .scenes
                .iter()
                .any(|s| s.slug.as_str() == candidate)
        })?;
        let dir_name = format!("{next_number:03}-{slug}");
        let dir = self.root.join("scenes").join(&dir_name);
        fs::create_dir_all(&dir).map_err(|e| Error::Io {
            path: dir.clone(),
            source: e,
        })?;
        write_if_absent(
            &dir.join("scene.toml"),
            &T_SCENE
                .replace("{{DIR}}", &dir_name)
                .replace("{{TITLE}}", &toml_escape(title)),
        )?;
        write_if_absent(
            &dir.join("shots.toml"),
            &T_SHOTS
                .replace("{{DIR}}", &dir_name)
                .replace("{{SLUG}}", slug.as_str()),
        )?;
        Ok(dir)
    }

    /// Create a character file from the template. Errors if the slug exists.
    pub fn create_character(&self, name: &str) -> Result<PathBuf> {
        let slug = slugify(name)?;
        let path = self.root.join("characters").join(format!("{slug}.toml"));
        if path.exists() {
            return Err(Error::Project(format!("character `{slug}` already exists")));
        }
        write_if_absent(
            &path,
            &T_CHARACTER
                .replace("{{SLUG}}", slug.as_str())
                .replace("{{NAME}}", &toml_escape(name)),
        )?;
        Ok(path)
    }

    /// Create a world entry from the template. Errors if the slug exists.
    pub fn create_world(&self, name: &str, kind: WorldKind) -> Result<PathBuf> {
        let slug = slugify(name)?;
        let path = self.root.join("world").join(format!("{slug}.toml"));
        if path.exists() {
            return Err(Error::Project(format!(
                "world entry `{slug}` already exists"
            )));
        }
        let kind_str: String = kind.into();
        write_if_absent(
            &path,
            &T_WORLD
                .replace("{{SLUG}}", slug.as_str())
                .replace("{{NAME}}", &toml_escape(name))
                .replace("{{KIND}}", &kind_str),
        )?;
        Ok(path)
    }

    /// Read every project file into typed state. Unreadable/unparseable
    /// files become issues instead of failures — the show goes on.
    pub fn scan(&self) -> ProjectScan {
        let mut issues = Vec::new();
        let root = &self.root;

        let manifest = load_file(&root.join("autoteur.toml"), &mut issues, |d, t| {
            let mut lints = Vec::new();
            if !d.contains_key("schema_version") {
                lints.push(missing_schema_version());
            }
            let _: &ProjectFile = t;
            lints
        });
        let beats = load_file(
            &root.join("story").join("beats.toml"),
            &mut issues,
            |d, t| lint::lint_beats(d, t),
        );
        let takes = load_file(&root.join("takes.manifest.toml"), &mut issues, |d, _t| {
            let mut lints = Vec::new();
            if !d.contains_key("schema_version") {
                lints.push(missing_schema_version());
            }
            lints
        });
        let timeline = load_file(&root.join("timeline.toml"), &mut issues, |d, _t| {
            let mut lints = Vec::new();
            if !d.contains_key("schema_version") {
                lints.push(missing_schema_version());
            }
            lints
        });

        let mut scenes = Vec::new();
        for entry in read_dir_sorted(&root.join("scenes")) {
            if !entry.is_dir() {
                continue;
            }
            let name = entry
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            if name.starts_with('.') {
                continue;
            }
            let Some((number, slug)) = parse_scene_dir_name(&name) else {
                issues.push(FileIssue {
                    path: entry.clone(),
                    message: format!(
                        "scene directory `{name}` doesn't follow <NNN>-<slug> (e.g. 010-first-scene)"
                    ),
                });
                continue;
            };
            let scene = load_file(&entry.join("scene.toml"), &mut issues, |d, t| {
                lint::lint_scene(d, t)
            });
            let shots = load_file(&entry.join("shots.toml"), &mut issues, |d, t| {
                lint::lint_shots(d, t)
            });
            if scene.is_none() && !entry.join("scene.toml").exists() {
                issues.push(FileIssue {
                    path: entry.join("scene.toml"),
                    message: format!("scene `{slug}` has no scene.toml"),
                });
            }
            scenes.push(SceneEntry {
                number,
                slug,
                dir: entry,
                scene,
                shots,
            });
        }
        scenes.sort_by(|a, b| (a.number, &a.slug).cmp(&(b.number, &b.slug)));

        let mut characters = BTreeMap::new();
        for path in toml_files_in(&root.join("characters"), &mut issues) {
            let (slug, entry) = match slug_of(&path) {
                Ok(slug) => (
                    slug,
                    load_file(&path, &mut issues, |d, t| lint::lint_character(d, t)),
                ),
                Err(message) => {
                    issues.push(FileIssue { path, message });
                    continue;
                }
            };
            if let Some(entry) = entry {
                characters.insert(slug, entry);
            }
        }
        let mut world = BTreeMap::new();
        for path in toml_files_in(&root.join("world"), &mut issues) {
            let (slug, entry) = match slug_of(&path) {
                Ok(slug) => (
                    slug,
                    load_file(&path, &mut issues, |d, t| lint::lint_world(d, t)),
                ),
                Err(message) => {
                    issues.push(FileIssue { path, message });
                    continue;
                }
            };
            if let Some(entry) = entry {
                world.insert(slug, entry);
            }
        }

        ProjectScan {
            state: ProjectState {
                manifest,
                beats,
                scenes,
                characters,
                world,
                takes,
                timeline,
            },
            issues,
        }
    }
}

/// Cross-file reference validation. Dangling references are warnings (the
/// show goes on, with a notice); identity collisions are errors.
pub fn validate(state: &ProjectState) -> Vec<ProjectLint> {
    let mut lints = Vec::new();
    let warn = |lints: &mut Vec<ProjectLint>, path: &Path, message: String| {
        lints.push(ProjectLint {
            path: path.to_owned(),
            severity: Severity::Warning,
            message,
        });
    };
    let error = |lints: &mut Vec<ProjectLint>, path: &Path, message: String| {
        lints.push(ProjectLint {
            path: path.to_owned(),
            severity: Severity::Error,
            message,
        });
    };

    let beat_ids: Vec<&Slug> = state
        .beats
        .iter()
        .flat_map(|b| b.data.beats.iter().map(|beat| &beat.id))
        .collect();
    let episode_ids: Vec<&Slug> = state
        .beats
        .iter()
        .flat_map(|b| b.data.episodes.iter().map(|e| &e.id))
        .collect();

    // Duplicate scene identities are the one collision the filesystem
    // can't prevent (012-vault and 020-vault both claim `vault`).
    let mut seen_slugs: BTreeMap<&Slug, &Path> = BTreeMap::new();
    for scene in &state.scenes {
        if let Some(first) = seen_slugs.insert(&scene.slug, &scene.dir) {
            error(
                &mut lints,
                &scene.dir,
                format!(
                    "two scene directories claim the identity `{}` (also {})",
                    scene.slug,
                    first.display()
                ),
            );
        }
    }

    // Per-scene reference checks.
    for scene_entry in &state.scenes {
        if let Some(scene) = &scene_entry.scene {
            for beat in &scene.data.beats {
                if !beat_ids.contains(&beat) {
                    warn(
                        &mut lints,
                        &scene.path,
                        format!("realizes unknown beat `{beat}`"),
                    );
                }
            }
            for character in &scene.data.characters {
                if !state.characters.contains_key(character) {
                    warn(
                        &mut lints,
                        &scene.path,
                        format!("casts unknown character `{character}`"),
                    );
                }
            }
            for slug in &scene.data.world {
                if !state.world.contains_key(slug) {
                    warn(
                        &mut lints,
                        &scene.path,
                        format!("references unknown world entry `{slug}`"),
                    );
                }
            }
            if let Some(location) = &scene.data.location {
                match state.world.get(location) {
                    None => warn(
                        &mut lints,
                        &scene.path,
                        format!("location `{location}` doesn't exist under world/"),
                    ),
                    Some(entry) if entry.data.kind != WorldKind::Location => warn(
                        &mut lints,
                        &scene.path,
                        format!("location `{location}` is not kind = \"location\""),
                    ),
                    _ => {}
                }
            }
        }

        if let Some(shots) = &scene_entry.shots {
            for shot in &shots.data.shots {
                for cast in shot.characters.iter().flatten() {
                    match state.characters.get(&cast.character) {
                        None => warn(
                            &mut lints,
                            &shots.path,
                            format!(
                                "shot `{}` casts unknown character `{}`",
                                shot.id, cast.character
                            ),
                        ),
                        Some(character) => {
                            if let Some(variant) = &cast.variant {
                                let exists = character
                                    .data
                                    .prompt
                                    .as_ref()
                                    .is_some_and(|p| p.variants.contains_key(variant));
                                if !exists {
                                    warn(
                                        &mut lints,
                                        &shots.path,
                                        format!(
                                            "shot `{}` pins `{}:{variant}` but that variant isn't defined",
                                            shot.id, cast.character
                                        ),
                                    );
                                }
                            }
                        }
                    }
                }
                for slug in shot.world.iter().flatten() {
                    if !state.world.contains_key(slug) {
                        warn(
                            &mut lints,
                            &shots.path,
                            format!("shot `{}` references unknown world entry `{slug}`", shot.id),
                        );
                    }
                }
                for cue in &shot.dialogue {
                    if !state.characters.contains_key(&cue.character) {
                        warn(
                            &mut lints,
                            &shots.path,
                            format!(
                                "shot `{}` has dialogue for unknown character `{}`",
                                shot.id, cue.character
                            ),
                        );
                    }
                }
                if let Some(take) = &shot.selected_take {
                    let expected = ShotRef::new(scene_entry.slug.clone(), shot.id.clone());
                    let record = state
                        .takes
                        .iter()
                        .flat_map(|m| m.data.takes.iter())
                        .find(|t| &t.id == take);
                    match record {
                        None => warn(
                            &mut lints,
                            &shots.path,
                            format!(
                                "shot `{}` circles take `{take}` which isn't in the manifest (media may need regenerating)",
                                shot.id
                            ),
                        ),
                        Some(record) if record.shot != expected => warn(
                            &mut lints,
                            &shots.path,
                            format!(
                                "shot `{}` circles take `{take}` which belongs to `{}`",
                                shot.id, record.shot
                            ),
                        ),
                        _ => {}
                    }
                }
            }
        }
    }

    // Manifest integrity: unique take ids, takes point at real shots.
    if let Some(takes) = &state.takes {
        let mut seen = BTreeMap::new();
        for record in &takes.data.takes {
            if let Some(_first) = seen.insert(&record.id, &record.shot) {
                error(
                    &mut lints,
                    &takes.path,
                    format!("duplicate take id `{}` in the manifest", record.id),
                );
            }
            let found = state.scenes.iter().any(|s| {
                s.slug == record.shot.scene
                    && s.shots
                        .as_ref()
                        .is_some_and(|f| f.data.shots.iter().any(|sh| sh.id == record.shot.shot))
            });
            if !found {
                warn(
                    &mut lints,
                    &takes.path,
                    format!(
                        "takes recorded for `{}` but that shot doesn't exist (was it moved or deleted?)",
                        record.shot
                    ),
                );
            }
        }
    }

    // Timeline references.
    if let Some(timeline) = &state.timeline {
        let mut check_entry = |shot_ref: &ShotRef, lints: &mut Vec<ProjectLint>| {
            let found = state.scenes.iter().any(|s| {
                s.slug == shot_ref.scene
                    && s.shots
                        .as_ref()
                        .is_some_and(|f| f.data.shots.iter().any(|sh| sh.id == shot_ref.shot))
            });
            if !found {
                warn(
                    lints,
                    &timeline.path,
                    format!("the cut references `{shot_ref}` which doesn't exist"),
                );
            }
        };
        for entry in &timeline.data.entries {
            check_entry(&entry.shot, &mut lints);
        }
        for sequence in &timeline.data.sequences {
            if !episode_ids.contains(&&sequence.episode) {
                warn(
                    &mut lints,
                    &timeline.path,
                    format!("sequence for unknown episode `{}`", sequence.episode),
                );
            }
            for entry in &sequence.entries {
                check_entry(&entry.shot, &mut lints);
            }
        }
    }

    // Project defaults.
    if let Some(manifest) = &state.manifest {
        for slug in &manifest.data.defaults.style {
            match state.world.get(slug) {
                None => warn(
                    &mut lints,
                    &manifest.path,
                    format!("[defaults].style lists `{slug}` which doesn't exist under world/"),
                ),
                Some(entry) if entry.data.kind != WorldKind::Style => warn(
                    &mut lints,
                    &manifest.path,
                    format!("[defaults].style lists `{slug}` which is not kind = \"style\""),
                ),
                _ => {}
            }
        }
    }

    lints
}

/// Read project text: UTF-8 required, UTF-8 BOM tolerated, UTF-16 detected
/// with an actionable hint (PowerShell 5.1 `>` redirection writes UTF-16).
pub fn read_text(path: &Path) -> Result<String> {
    let bytes = fs::read(path).map_err(|e| Error::Io {
        path: path.to_owned(),
        source: e,
    })?;
    let looks_utf16 = bytes.starts_with(&[0xFF, 0xFE])
        || bytes.starts_with(&[0xFE, 0xFF])
        || bytes.iter().take(64).any(|b| *b == 0);
    if looks_utf16 {
        return Err(Error::Encoding {
            path: path.to_owned(),
            hint: " (it looks like UTF-16 — rewrite it as UTF-8; avoid PowerShell `>` redirection)"
                .to_owned(),
        });
    }
    let text = String::from_utf8(bytes).map_err(|_| Error::Encoding {
        path: path.to_owned(),
        hint: String::new(),
    })?;
    Ok(match text.strip_prefix('\u{feff}') {
        Some(stripped) => stripped.to_owned(),
        None => text,
    })
}

/// Turn a title into a valid identity slug ("The Vault Job!" → "the-vault-job").
pub fn slugify(input: &str) -> Result<Slug> {
    let mut out = String::new();
    for c in input.to_lowercase().chars() {
        if c.is_ascii_lowercase() || c.is_ascii_digit() {
            out.push(c);
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
    }
    let trimmed = out.trim_end_matches('-');
    Slug::new(if trimmed.is_empty() {
        "untitled"
    } else {
        trimmed
    })
}

fn disambiguate(base: &Slug, taken: impl Fn(&str) -> bool) -> Result<Slug> {
    if !taken(base.as_str()) {
        return Ok(base.clone());
    }
    for n in 2..100 {
        let candidate = format!("{base}-{n}");
        if !taken(&candidate) {
            return Slug::new(candidate);
        }
    }
    Err(Error::Project(format!(
        "couldn't find a free name near `{base}`"
    )))
}

fn toml_escape(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

fn write_if_absent(path: &Path, content: &str) -> Result<()> {
    if path.exists() {
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| Error::Io {
            path: parent.to_owned(),
            source: e,
        })?;
    }
    fs::write(path, content).map_err(|e| Error::Io {
        path: path.to_owned(),
        source: e,
    })
}

fn load_file<T: DeserializeOwned>(
    path: &Path,
    issues: &mut Vec<FileIssue>,
    linter: impl Fn(&DocumentMut, &T) -> Vec<Lint>,
) -> Option<FileEntry<T>> {
    if !path.exists() {
        return None;
    }
    let text = match read_text(path) {
        Ok(text) => text,
        Err(e) => {
            issues.push(FileIssue {
                path: path.to_owned(),
                message: error_chain(&e),
            });
            return None;
        }
    };
    match doc::parse::<T>(&text) {
        Ok((data, document)) => {
            let lints = linter(&document, &data);
            Some(FileEntry {
                path: path.to_owned(),
                data,
                lints,
            })
        }
        Err(e) => {
            issues.push(FileIssue {
                path: path.to_owned(),
                message: error_chain(&e),
            });
            None
        }
    }
}

fn error_chain(err: &dyn std::error::Error) -> String {
    let mut message = err.to_string();
    let mut source = err.source();
    while let Some(cause) = source {
        message.push_str(": ");
        message.push_str(&cause.to_string());
        source = cause.source();
    }
    message
}

fn missing_schema_version() -> Lint {
    Lint {
        severity: Severity::Warning,
        message: "missing schema_version (expected as the first key of the file)".to_owned(),
    }
}

fn read_dir_sorted(dir: &Path) -> Vec<PathBuf> {
    let mut entries: Vec<PathBuf> = fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .collect();
    entries.sort();
    entries
}

fn toml_files_in(dir: &Path, _issues: &mut [FileIssue]) -> Vec<PathBuf> {
    read_dir_sorted(dir)
        .into_iter()
        .filter(|p| p.is_file() && p.extension().is_some_and(|e| e == "toml"))
        .collect()
}

fn slug_of(path: &Path) -> std::result::Result<Slug, String> {
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    Slug::new(stem.clone()).map_err(|_| {
        format!(
            "`{stem}` isn't a valid identity slug (kebab-case: lowercase letters/digits and dashes)"
        )
    })
}
