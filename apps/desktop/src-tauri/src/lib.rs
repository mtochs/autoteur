//! Autoteur desktop backend: a thin Tauri command layer over
//! autoteur-core. Every command is a typed gesture that goes through the
//! sync engine's read-modify-write path; the engine's deltas stream to the
//! frontend as `project-delta` events, generation progress as
//! `generation-update`, and export progress as `render-status`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use serde::Serialize;
use tauri::{AppHandle, Emitter, State};
use toml_edit::{Item, Table, Value};

use autoteur_core::doc;
use autoteur_core::error::Error as CoreError;
use autoteur_core::git;
use autoteur_core::id::{ShotId, ShotRef, Slug, TakeId};
use autoteur_core::project::{self, Project, ProjectState};
use autoteur_core::prompt::{self, PromptContext, ResolvedPrompt};
use autoteur_core::provider::{secrets, GenerationRequest, ModelInfo, ProviderRegistry};
use autoteur_core::queue::{GenerationJob, GenerationQueue};
use autoteur_core::render;
use autoteur_core::schema::character::CharacterFile;
use autoteur_core::schema::project::ProjectFormat;
use autoteur_core::schema::shots::ShotStatus;
use autoteur_core::schema::world::{WorldFile, WorldKind};
use autoteur_core::sync::{SyncEngine, SyncOptions};

struct OpenedProject {
    project: Project,
    engine: SyncEngine,
    queue: GenerationQueue,
}

#[derive(Default)]
struct AppState {
    open: Mutex<Option<OpenedProject>>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

type CmdResult<T> = Result<T, String>;

fn err_chain(err: &dyn std::error::Error) -> String {
    let mut message = err.to_string();
    let mut source = err.source();
    while let Some(cause) = source {
        message.push_str(": ");
        message.push_str(&cause.to_string());
        source = cause.source();
    }
    message
}

fn core_err(err: CoreError) -> String {
    err_chain(&err)
}

#[derive(Serialize)]
struct Snapshot {
    root: String,
    state: ProjectState,
}

fn with_open<T>(
    state: &State<'_, AppState>,
    f: impl FnOnce(&OpenedProject) -> CmdResult<T>,
) -> CmdResult<T> {
    let guard = lock(&state.open);
    match guard.as_ref() {
        Some(open) => f(open),
        None => Err("no project is open".to_owned()),
    }
}

fn attach(app: &AppHandle, state: &State<'_, AppState>, project: Project) -> CmdResult<Snapshot> {
    let (engine, events) = SyncEngine::start(&project, SyncOptions::default()).map_err(core_err)?;
    let forward_app = app.clone();
    std::thread::spawn(move || {
        for event in events {
            let _ = forward_app.emit("project-delta", &event);
        }
    });

    let (queue, updates) = GenerationQueue::start(
        project.root().to_owned(),
        ProviderRegistry::default(),
        Arc::new(secrets::get_api_key),
    );
    let updates_app = app.clone();
    std::thread::spawn(move || {
        for update in updates {
            let _ = updates_app.emit("generation-update", &update);
        }
    });

    let snapshot = Snapshot {
        root: project.root().to_string_lossy().into_owned(),
        state: engine.snapshot(),
    };
    let mut guard = lock(&state.open);
    if let Some(previous) = guard.take() {
        previous.engine.stop();
    }
    *guard = Some(OpenedProject {
        project,
        engine,
        queue,
    });
    Ok(snapshot)
}

// ---------------------------------------------------------------------
// Project lifecycle
// ---------------------------------------------------------------------

#[tauri::command]
fn create_project(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
    title: String,
    series: bool,
) -> CmdResult<Snapshot> {
    let format = if series {
        ProjectFormat::Series
    } else {
        ProjectFormat::Feature
    };
    let project = Project::create(Path::new(&path), &title, format).map_err(core_err)?;
    attach(&app, &state, project)
}

#[tauri::command]
fn open_project(app: AppHandle, state: State<'_, AppState>, path: String) -> CmdResult<Snapshot> {
    let project = Project::open(Path::new(&path)).map_err(core_err)?;
    attach(&app, &state, project)
}

#[tauri::command]
fn close_project(state: State<'_, AppState>) -> CmdResult<()> {
    if let Some(open) = lock(&state.open).take() {
        open.engine.stop();
    }
    Ok(())
}

#[tauri::command]
fn get_snapshot(state: State<'_, AppState>) -> CmdResult<Snapshot> {
    with_open(&state, |open| {
        Ok(Snapshot {
            root: open.engine.root().to_string_lossy().into_owned(),
            state: open.engine.snapshot(),
        })
    })
}

#[tauri::command]
fn get_validation(state: State<'_, AppState>) -> CmdResult<Vec<project::ProjectLint>> {
    with_open(&state, |open| {
        Ok(project::validate(&open.engine.snapshot()))
    })
}

// ---------------------------------------------------------------------
// Writers' Room
// ---------------------------------------------------------------------

fn doc_rel(doc: &str) -> CmdResult<PathBuf> {
    match doc {
        "logline" => Ok(PathBuf::from("story/logline.md")),
        "treatment" => Ok(PathBuf::from("story/treatment.md")),
        other => Err(format!("unknown story document `{other}`")),
    }
}

#[tauri::command]
fn read_story_doc(state: State<'_, AppState>, doc: String) -> CmdResult<String> {
    with_open(&state, |open| {
        let path = open.engine.root().join(doc_rel(&doc)?);
        Ok(std::fs::read_to_string(path).unwrap_or_default())
    })
}

#[tauri::command]
fn write_story_doc(state: State<'_, AppState>, doc: String, content: String) -> CmdResult<()> {
    with_open(&state, |open| {
        open.engine
            .write_text_file(&doc_rel(&doc)?, &content)
            .map_err(core_err)
    })
}

// ---------------------------------------------------------------------
// Beat Board
// ---------------------------------------------------------------------

const BEATS_REL: &str = "story/beats.toml";

fn existing_beat_ids(document: &toml_edit::DocumentMut) -> Vec<String> {
    document
        .get("beats")
        .and_then(Item::as_array_of_tables)
        .map(|aot| {
            aot.iter()
                .filter_map(|t| t.get("id").and_then(Item::as_str).map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

fn beat_index(document: &toml_edit::DocumentMut, id: &str) -> CmdResult<usize> {
    existing_beat_ids(document)
        .iter()
        .position(|b| b == id)
        .ok_or_else(|| format!("no beat `{id}`"))
}

#[tauri::command]
fn add_beat(
    state: State<'_, AppState>,
    title: String,
    summary: String,
    episode: Option<String>,
) -> CmdResult<String> {
    with_open(&state, |open| {
        let base = project::slugify(&title).map_err(core_err)?;
        let minted = Arc::new(Mutex::new(String::new()));
        let minted_in = Arc::clone(&minted);
        open.engine
            .edit_document(Path::new(BEATS_REL), move |document| {
                let existing = existing_beat_ids(document);
                let mut slug = base.to_string();
                let mut n = 2;
                while existing.contains(&slug) {
                    slug = format!("{base}-{n}");
                    n += 1;
                }
                let mut table = Table::new();
                table.insert("id", toml_edit::value(slug.clone()));
                table.insert("title", toml_edit::value(title.clone()));
                if let Some(episode) = &episode {
                    table.insert("episode", toml_edit::value(episode.clone()));
                }
                table.insert("summary", toml_edit::value(summary.clone()));
                *lock(&minted_in) = slug;
                doc::append_block(document, "beats", table)
            })
            .map_err(core_err)?;
        let slug = lock(&minted).clone();
        Ok(slug)
    })
}

#[tauri::command]
fn update_beat(
    state: State<'_, AppState>,
    id: String,
    patch: serde_json::Map<String, serde_json::Value>,
) -> CmdResult<()> {
    let allowed = ["title", "summary", "act", "color", "notes", "episode"];
    with_open(&state, |open| {
        open.engine
            .edit_document(Path::new(BEATS_REL), move |document| {
                let index = beat_index(document, &id).map_err(CoreError::Edit)?;
                apply_patch(document, "beats", index, &patch, &allowed)
            })
            .map_err(core_err)
    })
}

#[tauri::command]
fn remove_beat(state: State<'_, AppState>, id: String) -> CmdResult<()> {
    with_open(&state, |open| {
        open.engine
            .edit_document(Path::new(BEATS_REL), move |document| {
                let index = beat_index(document, &id).map_err(CoreError::Edit)?;
                doc::remove_block(document, "beats", index)
            })
            .map_err(core_err)
    })
}

#[tauri::command]
fn move_beat(state: State<'_, AppState>, from: usize, to: usize) -> CmdResult<()> {
    with_open(&state, |open| {
        open.engine.move_beat(from, to).map_err(core_err)
    })
}

// ---------------------------------------------------------------------
// Scenes, casting, world
// ---------------------------------------------------------------------

#[tauri::command]
fn create_scene(state: State<'_, AppState>, title: String) -> CmdResult<String> {
    with_open(&state, |open| {
        let dir = open.project.create_scene(&title).map_err(core_err)?;
        Ok(dir.to_string_lossy().into_owned())
    })
}

#[tauri::command]
fn create_character(state: State<'_, AppState>, name: String) -> CmdResult<String> {
    with_open(&state, |open| {
        let path = open.project.create_character(&name).map_err(core_err)?;
        Ok(path.to_string_lossy().into_owned())
    })
}

#[tauri::command]
fn create_world(state: State<'_, AppState>, name: String, kind: String) -> CmdResult<String> {
    with_open(&state, |open| {
        let path = open
            .project
            .create_world(&name, WorldKind::from(kind.clone()))
            .map_err(core_err)?;
        Ok(path.to_string_lossy().into_owned())
    })
}

#[tauri::command]
fn update_scene(
    state: State<'_, AppState>,
    slug: String,
    patch: serde_json::Map<String, serde_json::Value>,
) -> CmdResult<()> {
    let allowed = [
        "title",
        "synopsis",
        "director_notes",
        "mood",
        "location",
        "int_ext",
        "time",
        "characters",
        "world",
        "beats",
    ];
    with_open(&state, |open| {
        let rel = scene_file_rel(open, &slug)?;
        open.engine
            .edit_document(&rel, move |document| {
                apply_root_patch(document, &patch, &allowed)
            })
            .map_err(core_err)
    })
}

/// Character/world editing: root fields plus [prompt] and [voice] tables.
#[tauri::command]
fn update_entity(
    state: State<'_, AppState>,
    kind: String,
    slug: String,
    patch: serde_json::Map<String, serde_json::Value>,
) -> CmdResult<()> {
    let rel = match kind.as_str() {
        "character" => PathBuf::from(format!("characters/{slug}.toml")),
        "world" => PathBuf::from(format!("world/{slug}.toml")),
        other => return Err(format!("unknown entity kind `{other}`")),
    };
    with_open(&state, |open| {
        open.engine
            .edit_document(&rel, move |document| {
                for (key, value) in &patch {
                    match key.as_str() {
                        "name" | "description" | "kind" => {
                            apply_root_value(document, key, value)?;
                        }
                        "fragment" | "negative" => {
                            apply_subtable_value(document, "prompt", key, value)?;
                        }
                        "voice_provider" => {
                            apply_subtable_value(document, "voice", "provider", value)?;
                        }
                        "voice_id" => {
                            apply_subtable_value(document, "voice", "voice_id", value)?;
                        }
                        "voice_style" => {
                            apply_subtable_value(document, "voice", "style", value)?;
                        }
                        "reference_images" => {
                            apply_subtable_value(document, "visual", "reference_images", value)?;
                        }
                        other => {
                            return Err(CoreError::Edit(format!("unknown field `{other}`")));
                        }
                    }
                }
                Ok(())
            })
            .map_err(core_err)
    })
}

// ---------------------------------------------------------------------
// Shots
// ---------------------------------------------------------------------

fn scene_file_rel(open: &OpenedProject, slug: &str) -> CmdResult<PathBuf> {
    let slug: Slug = slug.parse().map_err(core_err)?;
    let shots = open.engine.shots_rel_path(&slug).map_err(core_err)?;
    Ok(shots
        .parent()
        .map(|p| p.join("scene.toml"))
        .unwrap_or_else(|| PathBuf::from("scene.toml")))
}

fn shot_index_in(document: &toml_edit::DocumentMut, id: &str) -> CmdResult<usize> {
    document
        .get("shots")
        .and_then(Item::as_array_of_tables)
        .and_then(|aot| {
            aot.iter()
                .position(|t| t.get("id").and_then(Item::as_str) == Some(id))
        })
        .ok_or_else(|| format!("no shot `{id}` in this scene"))
}

#[tauri::command]
fn add_shot(
    state: State<'_, AppState>,
    scene: String,
    patch: serde_json::Map<String, serde_json::Value>,
) -> CmdResult<String> {
    with_open(&state, |open| {
        let slug: Slug = scene.parse().map_err(core_err)?;
        let rel = open.engine.shots_rel_path(&slug).map_err(core_err)?;
        let id = project::next_shot_id(&open.engine.snapshot(), &slug);
        let id_str = id.to_string();
        let allowed = shot_fields();
        open.engine
            .edit_document(&rel, move |document| {
                let mut table = Table::new();
                table.insert("id", toml_edit::value(id_str.clone()));
                doc::append_block(document, "shots", table)?;
                let index = document
                    .get("shots")
                    .and_then(Item::as_array_of_tables)
                    .map(|aot| aot.len() - 1)
                    .unwrap_or(0);
                apply_patch(document, "shots", index, &patch, &allowed)
            })
            .map_err(core_err)?;
        Ok(id.to_string())
    })
}

fn shot_fields() -> [&'static str; 12] {
    [
        "framing",
        "camera",
        "action",
        "duration_s",
        "status",
        "notes",
        "prompt",
        "prompt_extra",
        "negative_extra",
        "characters",
        "world",
        "dialogue",
    ]
}

#[tauri::command]
fn update_shot(
    state: State<'_, AppState>,
    scene: String,
    id: String,
    patch: serde_json::Map<String, serde_json::Value>,
) -> CmdResult<()> {
    with_open(&state, |open| {
        let slug: Slug = scene.parse().map_err(core_err)?;
        let rel = open.engine.shots_rel_path(&slug).map_err(core_err)?;
        let allowed = shot_fields();
        open.engine
            .edit_document(&rel, move |document| {
                let index = shot_index_in(document, &id).map_err(CoreError::Edit)?;
                apply_patch(document, "shots", index, &patch, &allowed)
            })
            .map_err(core_err)
    })
}

#[tauri::command]
fn move_shot(state: State<'_, AppState>, scene: String, from: usize, to: usize) -> CmdResult<()> {
    with_open(&state, |open| {
        let slug: Slug = scene.parse().map_err(core_err)?;
        let rel = open.engine.shots_rel_path(&slug).map_err(core_err)?;
        open.engine
            .edit_document(&rel, move |document| {
                doc::move_block(document, "shots", from, to)
            })
            .map_err(core_err)
    })
}

#[tauri::command]
fn circle_take(
    state: State<'_, AppState>,
    scene: String,
    shot: String,
    take: Option<String>,
) -> CmdResult<()> {
    with_open(&state, |open| {
        let scene: Slug = scene.parse().map_err(core_err)?;
        let shot: ShotId = shot.parse().map_err(core_err)?;
        let take: Option<TakeId> = match take {
            Some(t) => Some(t.parse().map_err(core_err)?),
            None => None,
        };
        open.engine
            .circle_take(&scene, &shot, take.as_ref())
            .map_err(core_err)
    })
}

#[tauri::command]
fn set_shot_status(
    state: State<'_, AppState>,
    scene: String,
    shot: String,
    status: String,
) -> CmdResult<()> {
    with_open(&state, |open| {
        let scene: Slug = scene.parse().map_err(core_err)?;
        let shot: ShotId = shot.parse().map_err(core_err)?;
        open.engine
            .set_shot_status(&scene, &shot, ShotStatus::from(status.clone()))
            .map_err(core_err)
    })
}

// ---------------------------------------------------------------------
// Prompt resolution + generation
// ---------------------------------------------------------------------

fn build_maps(state: &ProjectState) -> (BTreeMap<Slug, CharacterFile>, BTreeMap<Slug, WorldFile>) {
    let characters = state
        .characters
        .iter()
        .map(|(k, v)| (k.clone(), v.data.clone()))
        .collect();
    let world = state
        .world
        .iter()
        .map(|(k, v)| (k.clone(), v.data.clone()))
        .collect();
    (characters, world)
}

fn resolve_for(state: &ProjectState, scene: &Slug, shot_id: &ShotId) -> CmdResult<ResolvedPrompt> {
    let defaults = state
        .manifest
        .as_ref()
        .map(|m| m.data.defaults.clone())
        .unwrap_or_default();
    let (characters, world) = build_maps(state);
    let scene_entry = state
        .scenes
        .iter()
        .find(|s| &s.slug == scene)
        .ok_or_else(|| format!("no scene `{scene}`"))?;
    let scene_file = scene_entry
        .scene
        .as_ref()
        .ok_or_else(|| format!("scene `{scene}` has no scene.toml"))?;
    let shot = scene_entry
        .shots
        .as_ref()
        .and_then(|f| f.data.shots.iter().find(|s| &s.id == shot_id))
        .ok_or_else(|| format!("no shot `{scene}/{shot_id}`"))?;
    let ctx = PromptContext {
        defaults: &defaults,
        scene: &scene_file.data,
        characters: &characters,
        world: &world,
    };
    Ok(prompt::resolve(&ctx, shot))
}

#[tauri::command]
fn resolve_shot_prompt(
    state: State<'_, AppState>,
    scene: String,
    shot: String,
) -> CmdResult<ResolvedPrompt> {
    with_open(&state, |open| {
        let scene: Slug = scene.parse().map_err(core_err)?;
        let shot: ShotId = shot.parse().map_err(core_err)?;
        resolve_for(&open.engine.snapshot(), &scene, &shot)
    })
}

#[tauri::command]
fn generate_shots(
    state: State<'_, AppState>,
    refs: Vec<String>,
    provider: Option<String>,
    model: Option<String>,
) -> CmdResult<Vec<u64>> {
    with_open(&state, |open| {
        let snapshot = open.engine.snapshot();
        let defaults = snapshot
            .manifest
            .as_ref()
            .map(|m| m.data.defaults.clone())
            .unwrap_or_default();
        let provider_id = provider
            .or(defaults.provider.clone())
            .unwrap_or_else(|| "replicate".to_owned());
        let model = model.or(defaults.video_model.clone()).ok_or_else(|| {
            "no model set — pick one in Studio Settings (or set video_model in autoteur.toml)"
                .to_owned()
        })?;
        if secrets::get_api_key(&provider_id)
            .map_err(core_err)?
            .is_none()
        {
            return Err(format!(
                "no API key saved for {provider_id} — connect your studio in Studio Settings"
            ));
        }

        let mut ids = Vec::new();
        for reference in &refs {
            let shot_ref: ShotRef = reference.parse().map_err(core_err)?;
            let resolved = resolve_for(&snapshot, &shot_ref.scene, &shot_ref.shot)?;
            let mut inputs = serde_json::json!({ "prompt": resolved.prompt });
            if let Some(negative) = &resolved.negative {
                inputs["negative_prompt"] = serde_json::json!(negative);
            }
            ids.push(open.queue.submit(GenerationJob {
                shot: shot_ref,
                provider: provider_id.clone(),
                request: GenerationRequest {
                    model: model.clone(),
                    inputs,
                },
                resolved_prompt: Some(resolved.prompt.clone()),
                negative_prompt: resolved.negative.clone(),
                seed: None,
            }));
        }
        Ok(ids)
    })
}

// ---------------------------------------------------------------------
// Save points, settings, export
// ---------------------------------------------------------------------

#[tauri::command]
fn save_point(
    state: State<'_, AppState>,
    message: Option<String>,
) -> CmdResult<Vec<git::SavePoint>> {
    with_open(&state, |open| {
        git::save_point(open.engine.root(), message.as_deref()).map_err(core_err)?;
        git::history(open.engine.root(), 30).map_err(core_err)
    })
}

#[tauri::command]
fn history(state: State<'_, AppState>, limit: usize) -> CmdResult<Vec<git::SavePoint>> {
    with_open(&state, |open| {
        git::history(open.engine.root(), limit).map_err(core_err)
    })
}

#[tauri::command]
fn restore_save_point(state: State<'_, AppState>, id: String) -> CmdResult<()> {
    with_open(&state, |open| {
        git::restore(open.engine.root(), &id).map_err(core_err)?;
        open.engine.request_sweep();
        Ok(())
    })
}

#[tauri::command]
fn set_trim(
    state: State<'_, AppState>,
    shot: String,
    in_s: Option<f64>,
    out_s: Option<f64>,
) -> CmdResult<()> {
    with_open(&state, |open| {
        let _valid: ShotRef = shot.parse().map_err(core_err)?;
        open.engine
            .edit_document(Path::new("timeline.toml"), move |document| {
                let found = document
                    .get("entries")
                    .and_then(Item::as_array_of_tables)
                    .and_then(|aot| {
                        aot.iter().position(|t| {
                            t.get("shot").and_then(Item::as_str) == Some(shot.as_str())
                        })
                    });
                let index = match found {
                    Some(index) => index,
                    None => {
                        let mut table = Table::new();
                        table.insert("shot", toml_edit::value(shot.clone()));
                        doc::append_block(document, "entries", table)?;
                        document
                            .get("entries")
                            .and_then(Item::as_array_of_tables)
                            .map(|aot| aot.len() - 1)
                            .unwrap_or(0)
                    }
                };
                match in_s {
                    Some(v) => {
                        doc::set_block_field(document, "entries", index, "in_s", Value::from(v))?
                    }
                    None => {
                        doc::remove_block_field(document, "entries", index, "in_s")?;
                    }
                }
                match out_s {
                    Some(v) => {
                        doc::set_block_field(document, "entries", index, "out_s", Value::from(v))?
                    }
                    None => {
                        doc::remove_block_field(document, "entries", index, "out_s")?;
                    }
                }
                Ok(())
            })
            .map_err(core_err)
    })
}

#[derive(Serialize)]
struct ProviderStatus {
    id: String,
    name: String,
    connected: bool,
}

#[tauri::command]
fn key_status() -> CmdResult<Vec<ProviderStatus>> {
    let registry = ProviderRegistry::default();
    let mut out = Vec::new();
    for provider in registry.all() {
        out.push(ProviderStatus {
            id: provider.id().to_owned(),
            name: provider.display_name().to_owned(),
            connected: secrets::get_api_key(provider.id())
                .map_err(core_err)?
                .is_some(),
        });
    }
    Ok(out)
}

#[tauri::command]
fn key_set(provider: String, key: String) -> CmdResult<()> {
    secrets::set_api_key(&provider, &key).map_err(core_err)
}

#[tauri::command]
fn key_clear(provider: String) -> CmdResult<()> {
    secrets::delete_api_key(&provider).map_err(core_err)
}

#[tauri::command]
fn recommended_models(provider: Option<String>) -> CmdResult<Vec<ModelInfo>> {
    let provider_id = provider.unwrap_or_else(|| "replicate".to_owned());
    let registry = ProviderRegistry::default();
    let provider = registry
        .get(&provider_id)
        .ok_or_else(|| format!("unknown provider `{provider_id}`"))?;
    let key = secrets::get_api_key(&provider_id)
        .map_err(core_err)?
        .ok_or_else(|| format!("connect {provider_id} first in Studio Settings"))?;
    provider.recommended_models(&key).map_err(core_err)
}

#[tauri::command]
fn set_defaults(
    state: State<'_, AppState>,
    patch: serde_json::Map<String, serde_json::Value>,
) -> CmdResult<()> {
    let allowed = ["provider", "video_model", "image_model", "negative"];
    with_open(&state, |open| {
        open.engine
            .edit_document(Path::new("autoteur.toml"), move |document| {
                for (key, value) in &patch {
                    if !allowed.contains(&key.as_str()) {
                        return Err(CoreError::Edit(format!("unknown default `{key}`")));
                    }
                    match json_to_toml_value(value) {
                        Some(v) => doc::set_subtable_field(document, "defaults", key, v),
                        None => {
                            doc::remove_subtable_field(document, "defaults", key);
                        }
                    }
                }
                Ok(())
            })
            .map_err(core_err)
    })
}

#[tauri::command]
fn ffmpeg_status() -> CmdResult<Option<String>> {
    Ok(render::find_ffmpeg().map(|p| p.to_string_lossy().into_owned()))
}

#[derive(Serialize, Clone)]
struct RenderStatus {
    phase: String,
    message: String,
}

#[tauri::command]
fn export_cut(app: AppHandle, state: State<'_, AppState>, output: String) -> CmdResult<()> {
    let (root, snapshot) = with_open(&state, |open| {
        Ok((open.engine.root().to_owned(), open.engine.snapshot()))
    })?;
    std::thread::spawn(move || {
        let emit = |phase: &str, message: String| {
            let _ = app.emit(
                "render-status",
                RenderStatus {
                    phase: phase.to_owned(),
                    message,
                },
            );
        };
        let plan = match render::build_plan(&root, &snapshot) {
            Ok(plan) => plan,
            Err(e) => return emit("error", err_chain(&e)),
        };
        let Some(ffmpeg) = render::find_ffmpeg() else {
            return emit(
                "error",
                "FFmpeg not found — install it or set AUTOTEUR_FFMPEG".to_owned(),
            );
        };
        emit(
            "working",
            format!("Assembling {} shot(s)…", plan.entries.len()),
        );
        match render::render(&plan, &ffmpeg, Path::new(&output)) {
            Ok(()) => emit("done", output.clone()),
            Err(e) => emit("error", err_chain(&e)),
        }
    });
    Ok(())
}

/// Take counts per shot ref, for Dailies badges.
#[tauri::command]
fn take_media(state: State<'_, AppState>) -> CmdResult<serde_json::Value> {
    with_open(&state, |open| {
        let snapshot = open.engine.snapshot();
        let root = open.engine.root();
        let mut by_shot: BTreeMap<String, Vec<serde_json::Value>> = BTreeMap::new();
        for record in snapshot.takes.iter().flat_map(|m| m.data.takes.iter()) {
            let media = record.outputs.first().and_then(|o| o.path.as_ref());
            let abs = media.map(|rel| root.join(rel));
            by_shot
                .entry(record.shot.to_string())
                .or_default()
                .push(serde_json::json!({
                    "id": record.id.to_string(),
                    "kind": record.outputs.first().and_then(|o| o.kind.clone()),
                    "path": abs.as_ref().map(|p| p.to_string_lossy().into_owned()),
                    "exists": abs.as_ref().map(|p| p.exists()).unwrap_or(false),
                    "created_at": record.created_at,
                    "model": record.model,
                    "provider": record.provider,
                    "resolved_prompt": record.resolved_prompt,
                    "cost_usd": record.cost_usd,
                }));
        }
        Ok(serde_json::json!(by_shot))
    })
}

// ---------------------------------------------------------------------
// Patch plumbing: JSON patches -> surgical TOML edits. `null` removes.
// ---------------------------------------------------------------------

fn json_to_toml_value(value: &serde_json::Value) -> Option<Value> {
    match value {
        serde_json::Value::Null => None,
        serde_json::Value::Bool(b) => Some(Value::from(*b)),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Some(Value::from(i))
            } else {
                n.as_f64().map(Value::from)
            }
        }
        serde_json::Value::String(s) => Some(Value::from(s.as_str())),
        serde_json::Value::Array(items) => {
            let mut array = toml_edit::Array::new();
            for item in items {
                match item {
                    serde_json::Value::String(s) => array.push(s.as_str()),
                    serde_json::Value::Object(map) => {
                        // dialogue cues: single-line inline tables
                        let mut inline = toml_edit::InlineTable::new();
                        for (k, v) in map {
                            if let Some(Value::String(s)) = json_to_toml_value(v) {
                                inline.insert(k, Value::String(s));
                            }
                        }
                        array.push(Value::InlineTable(inline));
                    }
                    other => {
                        if let Some(v) = json_to_toml_value(other) {
                            array.push(v);
                        }
                    }
                }
            }
            Some(Value::Array(array))
        }
        serde_json::Value::Object(_) => None,
    }
}

fn apply_patch(
    document: &mut toml_edit::DocumentMut,
    key: &str,
    index: usize,
    patch: &serde_json::Map<String, serde_json::Value>,
    allowed: &[&str],
) -> autoteur_core::Result<()> {
    for (field, value) in patch {
        if !allowed.contains(&field.as_str()) {
            return Err(CoreError::Edit(format!("unknown field `{field}`")));
        }
        match json_to_toml_value(value) {
            Some(v) => doc::set_block_field(document, key, index, field, v)?,
            None => {
                doc::remove_block_field(document, key, index, field)?;
            }
        }
    }
    Ok(())
}

fn apply_root_patch(
    document: &mut toml_edit::DocumentMut,
    patch: &serde_json::Map<String, serde_json::Value>,
    allowed: &[&str],
) -> autoteur_core::Result<()> {
    for (field, value) in patch {
        if !allowed.contains(&field.as_str()) {
            return Err(CoreError::Edit(format!("unknown field `{field}`")));
        }
        apply_root_value(document, field, value)?;
    }
    Ok(())
}

fn apply_root_value(
    document: &mut toml_edit::DocumentMut,
    field: &str,
    value: &serde_json::Value,
) -> autoteur_core::Result<()> {
    match json_to_toml_value(value) {
        Some(v) => doc::set_root_field(document, field, v),
        None => {
            doc::remove_root_field(document, field);
        }
    }
    Ok(())
}

fn apply_subtable_value(
    document: &mut toml_edit::DocumentMut,
    table: &str,
    field: &str,
    value: &serde_json::Value,
) -> autoteur_core::Result<()> {
    match json_to_toml_value(value) {
        Some(v) => doc::set_subtable_field(document, table, field, v),
        None => {
            doc::remove_subtable_field(document, table, field);
        }
    }
    Ok(())
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            create_project,
            open_project,
            close_project,
            get_snapshot,
            get_validation,
            read_story_doc,
            write_story_doc,
            add_beat,
            update_beat,
            remove_beat,
            move_beat,
            create_scene,
            create_character,
            create_world,
            update_scene,
            update_entity,
            add_shot,
            update_shot,
            move_shot,
            circle_take,
            set_shot_status,
            resolve_shot_prompt,
            generate_shots,
            save_point,
            history,
            restore_save_point,
            key_status,
            key_set,
            key_clear,
            recommended_models,
            set_defaults,
            ffmpeg_status,
            export_cut,
            take_media,
            set_trim
        ])
        .run(tauri::generate_context!())
        .expect("error while running Autoteur");
}
