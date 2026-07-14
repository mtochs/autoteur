//! `autoteur` — the command line for Autoteur projects. Every capability
//! the GUI has exists here first: the app is a lens, never a requirement.
//! Output is human text by default; `--json` gives machines the same view.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{anyhow, bail, Context, Result};
use clap::{Parser, Subcommand};

use autoteur_core::git;
use autoteur_core::id::{ShotRef, Slug};
use autoteur_core::lint::Severity;
use autoteur_core::project::{self, Project, ProjectState};
use autoteur_core::prompt::{self, PromptContext};
use autoteur_core::provider::{secrets, GenerationRequest, ProviderRegistry};
use autoteur_core::queue::{GenerationJob, GenerationQueue, JobStage};
use autoteur_core::render;
use autoteur_core::schema::character::CharacterFile;
use autoteur_core::schema::project::{Defaults, ProjectFormat};
use autoteur_core::schema::shots::ShotStatus;
use autoteur_core::schema::world::{WorldFile, WorldKind};

#[derive(Parser)]
#[command(
    name = "autoteur",
    version,
    about = "The director's chair, from the terminal. An Autoteur project is a plain git repo of TOML + Markdown; this CLI and any coding agent share it with the app."
)]
struct Cli {
    /// Project directory (defaults to the current directory).
    #[arg(long, global = true, default_value = ".")]
    project: PathBuf,
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Create a new project (git init + full scaffold + AGENTS.md)
    New {
        path: PathBuf,
        /// Project title; defaults to the directory name.
        #[arg(long)]
        title: Option<String>,
        /// A series (episodes) rather than a feature film.
        #[arg(long)]
        series: bool,
    },
    /// Parse and lint every file; report dangling references. Exit 2 on errors.
    Validate {
        #[arg(long)]
        json: bool,
    },
    /// Project summary: scenes, shots, takes, problems.
    Status {
        #[arg(long)]
        json: bool,
    },
    /// Scene tools.
    Scene {
        #[command(subcommand)]
        command: SceneCmd,
    },
    /// Casting tools.
    Character {
        #[command(subcommand)]
        command: CharacterCmd,
    },
    /// Locations & props tools.
    World {
        #[command(subcommand)]
        command: WorldCmd,
    },
    /// Queue generation for one shot (scene-slug/shot-id) or a whole scene.
    Generate {
        /// A shot reference like `vault-breach/a`.
        target: Option<String>,
        /// Generate every `ready` shot in this scene instead.
        #[arg(long)]
        scene: Option<String>,
        /// Provider id (default: [defaults].provider, else replicate).
        #[arg(long)]
        provider: Option<String>,
        /// Model `owner/name[:version]` (default: [defaults].video_model).
        #[arg(long)]
        model: Option<String>,
        /// Print the resolved prompts without generating.
        #[arg(long)]
        dry_run: bool,
    },
    /// Export the cut to an MP4 via FFmpeg.
    Render {
        #[arg(short, long, default_value = "screening.mp4")]
        output: PathBuf,
    },
    /// Create a save point (a git commit with a plain-language summary).
    Save {
        #[arg(short, long)]
        message: Option<String>,
    },
    /// List save points, newest first.
    History {
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
    /// API keys, stored in the OS credential manager — never in files.
    Key {
        #[command(subcommand)]
        command: KeyCmd,
    },
    /// Currently recommended generation models (fetched live).
    Models {
        #[arg(long)]
        provider: Option<String>,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
enum SceneCmd {
    /// Create the next scene directory with template files.
    New { title: String },
}

#[derive(Subcommand)]
enum CharacterCmd {
    /// Create a character file from the template.
    New { name: String },
}

#[derive(Subcommand)]
enum WorldCmd {
    /// Create a world entry (location, prop, vehicle, or style bible).
    New {
        name: String,
        #[arg(long, default_value = "location")]
        kind: String,
    },
}

#[derive(Subcommand)]
enum KeyCmd {
    /// Save an API key ("connect your studio").
    Set {
        provider: String,
        #[arg(long)]
        key: String,
    },
    /// Remove a stored API key.
    Clear { provider: String },
    /// Which providers are connected.
    Status,
}

fn main() {
    let cli = Cli::parse();
    match run(cli) {
        Ok(code) => std::process::exit(code),
        Err(e) => {
            eprintln!("error: {e:#}");
            std::process::exit(1);
        }
    }
}

fn run(cli: Cli) -> Result<i32> {
    match cli.command {
        Cmd::New {
            path,
            title,
            series,
        } => {
            let title = title.unwrap_or_else(|| {
                path.file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "Untitled".to_owned())
            });
            let format = if series {
                ProjectFormat::Series
            } else {
                ProjectFormat::Feature
            };
            Project::create(&path, &title, format)
                .with_context(|| format!("couldn't create a project in {}", path.display()))?;
            println!("Project created: {} ({})", path.display(), title);
            println!("Next: open it in Autoteur, or point your agent at AGENTS.md.");
            Ok(0)
        }
        Cmd::Validate { json } => cmd_validate(&cli.project, json),
        Cmd::Status { json } => cmd_status(&cli.project, json),
        Cmd::Scene {
            command: SceneCmd::New { title },
        } => {
            let dir = Project::open(&cli.project)?.create_scene(&title)?;
            println!("Scene created: {}", dir.display());
            Ok(0)
        }
        Cmd::Character {
            command: CharacterCmd::New { name },
        } => {
            let path = Project::open(&cli.project)?.create_character(&name)?;
            println!("Character created: {}", path.display());
            Ok(0)
        }
        Cmd::World {
            command: WorldCmd::New { name, kind },
        } => {
            let kind = WorldKind::from(kind);
            let path = Project::open(&cli.project)?.create_world(&name, kind)?;
            println!("World entry created: {}", path.display());
            Ok(0)
        }
        Cmd::Generate {
            target,
            scene,
            provider,
            model,
            dry_run,
        } => cmd_generate(&cli.project, target, scene, provider, model, dry_run),
        Cmd::Render { output } => cmd_render(&cli.project, &output),
        Cmd::Save { message } => {
            let id = git::save_point(&cli.project, message.as_deref())?;
            let history = git::history(&cli.project, 1)?;
            let summary = history
                .first()
                .map(|s| s.summary.clone())
                .unwrap_or_default();
            println!("{} {}", &id[..id.len().min(7)], summary);
            Ok(0)
        }
        Cmd::History { limit } => {
            for point in git::history(&cli.project, limit)? {
                println!("{}  {}", &point.id[..point.id.len().min(7)], point.summary);
            }
            Ok(0)
        }
        Cmd::Key { command } => cmd_key(command),
        Cmd::Models { provider, json } => cmd_models(provider, json),
    }
}

fn cmd_validate(root: &Path, json: bool) -> Result<i32> {
    let project = Project::open(root)?;
    let scan = project.scan();
    let cross = project::validate(&scan.state);

    let mut findings: Vec<(String, String, String)> = Vec::new(); // (severity, path, message)
    for issue in &scan.issues {
        findings.push((
            "error".to_owned(),
            display_rel(root, &issue.path),
            issue.message.clone(),
        ));
    }
    for (path, lints) in per_file_lints(&scan.state) {
        for lint in lints {
            findings.push((
                severity_str(lint.severity).to_owned(),
                display_rel(root, &path),
                lint.message.clone(),
            ));
        }
    }
    for lint in &cross {
        findings.push((
            severity_str(lint.severity).to_owned(),
            display_rel(root, &lint.path),
            lint.message.clone(),
        ));
    }

    let errors = findings.iter().filter(|(s, _, _)| s == "error").count();
    if json {
        let items: Vec<serde_json::Value> = findings
            .iter()
            .map(|(severity, path, message)| {
                serde_json::json!({ "severity": severity, "path": path, "message": message })
            })
            .collect();
        println!(
            "{}",
            serde_json::json!({ "ok": errors == 0, "errors": errors, "findings": items })
        );
    } else if findings.is_empty() {
        println!("All clear — every file parses and every reference resolves.");
    } else {
        for (severity, path, message) in &findings {
            println!("{severity:>7}  {path}: {message}");
        }
        println!("\n{} finding(s), {} error(s).", findings.len(), errors);
    }
    Ok(if errors > 0 { 2 } else { 0 })
}

fn per_file_lints(state: &ProjectState) -> Vec<(PathBuf, Vec<autoteur_core::lint::Lint>)> {
    let mut out = Vec::new();
    if let Some(entry) = &state.manifest {
        out.push((entry.path.clone(), entry.lints.clone()));
    }
    if let Some(entry) = &state.beats {
        out.push((entry.path.clone(), entry.lints.clone()));
    }
    for scene in &state.scenes {
        if let Some(entry) = &scene.scene {
            out.push((entry.path.clone(), entry.lints.clone()));
        }
        if let Some(entry) = &scene.shots {
            out.push((entry.path.clone(), entry.lints.clone()));
        }
    }
    for entry in state.characters.values() {
        out.push((entry.path.clone(), entry.lints.clone()));
    }
    for entry in state.world.values() {
        out.push((entry.path.clone(), entry.lints.clone()));
    }
    out.retain(|(_, lints)| !lints.is_empty());
    out
}

fn cmd_status(root: &Path, json: bool) -> Result<i32> {
    let project = Project::open(root)?;
    let scan = project.scan();
    let state = &scan.state;

    let take_counts: BTreeMap<String, usize> = {
        let mut counts = BTreeMap::new();
        for record in state.takes.iter().flat_map(|m| m.data.takes.iter()) {
            *counts.entry(record.shot.to_string()).or_insert(0) += 1;
        }
        counts
    };

    if json {
        let scenes: Vec<serde_json::Value> = state
            .scenes
            .iter()
            .map(|scene| {
                let shots: Vec<serde_json::Value> = scene
                    .shots
                    .iter()
                    .flat_map(|f| f.data.shots.iter())
                    .map(|shot| {
                        let shot_ref = format!("{}/{}", scene.slug, shot.id);
                        serde_json::json!({
                            "id": shot.id.to_string(),
                            "ref": shot_ref,
                            "status": String::from(shot.status.clone()),
                            "selected_take": shot.selected_take.as_ref().map(|t| t.to_string()),
                            "takes": take_counts.get(&shot_ref).copied().unwrap_or(0),
                        })
                    })
                    .collect();
                serde_json::json!({
                    "slug": scene.slug.to_string(),
                    "number": scene.number,
                    "title": scene.scene.as_ref().map(|s| s.data.title.clone()),
                    "shots": shots,
                })
            })
            .collect();
        let status = serde_json::json!({
            "title": state.manifest.as_ref().map(|m| m.data.title.clone()),
            "format": state.manifest.as_ref().map(|m| String::from(m.data.format.clone())),
            "beats": state.beats.as_ref().map(|b| b.data.beats.len()).unwrap_or(0),
            "episodes": state.beats.as_ref().map(|b| b.data.episodes.len()).unwrap_or(0),
            "characters": state.characters.len(),
            "world": state.world.len(),
            "takes": state.takes.as_ref().map(|t| t.data.takes.len()).unwrap_or(0),
            "scenes": scenes,
            "problems": scan.issues.iter().map(|i| serde_json::json!({
                "path": display_rel(root, &i.path),
                "message": i.message,
            })).collect::<Vec<_>>(),
        });
        println!("{status}");
        return Ok(0);
    }

    let title = state
        .manifest
        .as_ref()
        .map(|m| m.data.title.as_str())
        .unwrap_or("(untitled)");
    println!("{title}");
    println!(
        "  {} beats · {} scenes · {} cast · {} world entries · {} takes",
        state
            .beats
            .as_ref()
            .map(|b| b.data.beats.len())
            .unwrap_or(0),
        state.scenes.len(),
        state.characters.len(),
        state.world.len(),
        state
            .takes
            .as_ref()
            .map(|t| t.data.takes.len())
            .unwrap_or(0),
    );
    for scene in &state.scenes {
        let scene_title = scene
            .scene
            .as_ref()
            .map(|s| s.data.title.as_str())
            .unwrap_or("(no scene.toml)");
        println!("  {:>4}  {}  — {}", scene.number, scene.slug, scene_title);
        for shot in scene.shots.iter().flat_map(|f| f.data.shots.iter()) {
            let shot_ref = format!("{}/{}", scene.slug, shot.id);
            let takes = take_counts.get(&shot_ref).copied().unwrap_or(0);
            let circled = shot
                .selected_take
                .as_ref()
                .map(|t| format!("  ⊙ {t}"))
                .unwrap_or_default();
            println!(
                "        {}  [{}]  {} take(s){}",
                shot.id,
                String::from(shot.status.clone()),
                takes,
                circled
            );
        }
    }
    for issue in &scan.issues {
        println!("  ⚠ {}: {}", display_rel(root, &issue.path), issue.message);
    }
    Ok(0)
}

fn cmd_generate(
    root: &Path,
    target: Option<String>,
    scene: Option<String>,
    provider: Option<String>,
    model: Option<String>,
    dry_run: bool,
) -> Result<i32> {
    let project = Project::open(root)?;
    let scan = project.scan();
    let state = &scan.state;

    let defaults = state
        .manifest
        .as_ref()
        .map(|m| m.data.defaults.clone())
        .unwrap_or_default();
    let characters: BTreeMap<Slug, CharacterFile> = state
        .characters
        .iter()
        .map(|(k, v)| (k.clone(), v.data.clone()))
        .collect();
    let world: BTreeMap<Slug, WorldFile> = state
        .world
        .iter()
        .map(|(k, v)| (k.clone(), v.data.clone()))
        .collect();

    // Which shots?
    let mut selected: Vec<(Slug, autoteur_core::schema::shots::Shot, Defaults)> = Vec::new();
    match (&target, &scene) {
        (Some(target), None) => {
            let shot_ref: ShotRef = target
                .parse()
                .map_err(|e| anyhow!("{e} — expected something like vault-breach/a"))?;
            let scene_entry = state
                .scenes
                .iter()
                .find(|s| s.slug == shot_ref.scene)
                .ok_or_else(|| anyhow!("no scene `{}`", shot_ref.scene))?;
            let shot = scene_entry
                .shots
                .as_ref()
                .and_then(|f| f.data.shots.iter().find(|s| s.id == shot_ref.shot))
                .ok_or_else(|| anyhow!("no shot `{target}`"))?;
            match shot.status {
                ShotStatus::Locked => bail!(
                    "`{target}` is locked — the circled take is final. Set status = \"ready\" to allow retakes."
                ),
                ShotStatus::Omitted => bail!("`{target}` is omitted from the picture."),
                _ => {}
            }
            selected.push((shot_ref.scene.clone(), shot.clone(), defaults.clone()));
        }
        (None, Some(scene_slug)) => {
            let slug: Slug = scene_slug.parse().map_err(|e| anyhow!("{e}"))?;
            let scene_entry = state
                .scenes
                .iter()
                .find(|s| s.slug == slug)
                .ok_or_else(|| anyhow!("no scene `{scene_slug}`"))?;
            for shot in scene_entry.shots.iter().flat_map(|f| f.data.shots.iter()) {
                if shot.status == ShotStatus::Ready {
                    selected.push((slug.clone(), shot.clone(), defaults.clone()));
                }
            }
            if selected.is_empty() {
                bail!(
                    "no `ready` shots in `{scene_slug}` — mark shots ready (status = \"ready\") first"
                );
            }
        }
        _ => bail!("pass a shot like `vault-breach/a`, or --scene <slug> for every ready shot"),
    }

    let provider_id = provider
        .or(defaults.provider.clone())
        .unwrap_or_else(|| "replicate".to_owned());
    let model = model
        .or(defaults.video_model.clone())
        .ok_or_else(|| {
            anyhow!(
                "no model set — pass --model owner/name[:version] or set video_model under [defaults] in autoteur.toml"
            )
        })?;

    // Resolve every prompt first; a dry run stops here.
    let mut jobs = Vec::new();
    for (scene_slug, shot, defaults) in &selected {
        let scene_entry = state
            .scenes
            .iter()
            .find(|s| &s.slug == scene_slug)
            .and_then(|s| s.scene.as_ref())
            .ok_or_else(|| anyhow!("scene `{scene_slug}` has no scene.toml"))?;
        let ctx = PromptContext {
            defaults,
            scene: &scene_entry.data,
            characters: &characters,
            world: &world,
        };
        let resolved = prompt::resolve(&ctx, shot);
        let shot_ref = ShotRef::new(scene_slug.clone(), shot.id.clone());
        for warning in &resolved.warnings {
            eprintln!("  note ({shot_ref}): {warning}");
        }
        let mut inputs = serde_json::json!({ "prompt": resolved.prompt });
        if let Some(negative) = &resolved.negative {
            inputs["negative_prompt"] = serde_json::json!(negative);
        }
        if dry_run {
            println!("─── {shot_ref}  ({provider_id} · {model})");
            println!("{}", resolved.prompt);
            if let Some(negative) = &resolved.negative {
                println!("  negative: {negative}");
            }
            if !resolved.reference_images.is_empty() || !resolved.adapters.is_empty() {
                println!(
                    "  identity: {} reference image(s), {} adapter(s)",
                    resolved.reference_images.len(),
                    resolved.adapters.len()
                );
            }
            continue;
        }
        jobs.push(GenerationJob {
            shot: shot_ref,
            provider: provider_id.clone(),
            request: GenerationRequest {
                model: model.clone(),
                inputs,
            },
            resolved_prompt: Some(resolved.prompt.clone()),
            negative_prompt: resolved.negative.clone(),
            seed: None,
        });
    }
    if dry_run {
        return Ok(0);
    }

    if secrets::get_api_key(&provider_id)?.is_none() {
        bail!(
            "no API key saved for `{provider_id}` — connect your studio: autoteur key set {provider_id} --key <KEY>"
        );
    }

    let total = jobs.len();
    let (queue, updates) = GenerationQueue::start(
        root.to_owned(),
        ProviderRegistry::default(),
        Arc::new(secrets::get_api_key),
    );
    for job in jobs {
        queue.submit(job);
    }

    let mut finished = 0usize;
    let mut failures = 0usize;
    while finished < total {
        let update = updates
            .recv()
            .context("the generation worker stopped unexpectedly")?;
        match update.stage {
            JobStage::Queued => {}
            JobStage::Running => {
                println!("▶ {} — generating…", update.shot);
                let _ = std::io::stdout().flush();
            }
            JobStage::Done { take, deduplicated } => {
                finished += 1;
                if deduplicated {
                    println!("✓ {} → {take} (identical to an existing take)", update.shot);
                } else {
                    println!("✓ {} → {take}", update.shot);
                }
            }
            JobStage::Failed { message } => {
                finished += 1;
                failures += 1;
                println!("✗ {} — {message}", update.shot);
            }
        }
    }
    queue.shutdown();
    Ok(if failures > 0 { 1 } else { 0 })
}

fn cmd_render(root: &Path, output: &Path) -> Result<i32> {
    let project = Project::open(root)?;
    let scan = project.scan();
    let plan = render::build_plan(root, &scan.state)?;
    let ffmpeg = render::find_ffmpeg().ok_or_else(|| {
        anyhow!("FFmpeg not found — install it on PATH or set AUTOTEUR_FFMPEG to the executable")
    })?;
    println!("Assembling {} shot(s)…", plan.entries.len());
    render::render(&plan, &ffmpeg, output)?;
    println!("Screening copy ready: {}", output.display());
    Ok(0)
}

fn cmd_key(command: KeyCmd) -> Result<i32> {
    match command {
        KeyCmd::Set { provider, key } => {
            secrets::set_api_key(&provider, &key)?;
            println!("{provider} connected (key stored in the OS credential manager).");
        }
        KeyCmd::Clear { provider } => {
            secrets::delete_api_key(&provider)?;
            println!("{provider} disconnected.");
        }
        KeyCmd::Status => {
            for provider in ProviderRegistry::default().all() {
                let connected = secrets::get_api_key(provider.id())?.is_some();
                let mark = if connected {
                    "✓ connected"
                } else {
                    "· not connected"
                };
                println!("{:>12}  {mark}", provider.display_name());
            }
        }
    }
    Ok(0)
}

fn cmd_models(provider: Option<String>, json: bool) -> Result<i32> {
    let provider_id = provider.unwrap_or_else(|| "replicate".to_owned());
    let registry = ProviderRegistry::default();
    let provider = registry
        .get(&provider_id)
        .ok_or_else(|| anyhow!("unknown provider `{provider_id}`"))?;
    let key = secrets::get_api_key(&provider_id)?.ok_or_else(|| {
        anyhow!("connect your studio first: autoteur key set {provider_id} --key <KEY>")
    })?;
    let models = provider.recommended_models(&key)?;
    if json {
        let items: Vec<serde_json::Value> = models
            .iter()
            .map(|m| {
                serde_json::json!({
                    "slug": m.slug,
                    "version": m.version,
                    "kind": m.kind.as_str(),
                    "name": m.display_name,
                    "description": m.description,
                })
            })
            .collect();
        println!("{}", serde_json::json!(items));
    } else {
        for model in &models {
            println!(
                "{:>6}  {}  {}",
                model.kind.as_str(),
                model.slug,
                model.description.as_deref().unwrap_or("")
            );
        }
        if models.is_empty() {
            println!("(the provider returned no recommendations right now)");
        }
    }
    Ok(0)
}

fn severity_str(severity: Severity) -> &'static str {
    match severity {
        Severity::Error => "error",
        Severity::Warning => "warning",
    }
}

fn display_rel(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}
