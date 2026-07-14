//! Live sync: the bidirectional lens. A watcher thread turns file changes
//! into granular, origin-tagged deltas within a debounce window; typed
//! commands write back through read-modify-write + atomic rename, with a
//! content-hash journal tagging their echoes as `local`.
//!
//! Hardening rules proven by the design review, all load-bearing here:
//! - A journal hit tags origin only; the differ ALWAYS runs.
//! - Removal deltas are quarantined until they survive a re-parse after a
//!   hold window (truncated-but-valid TOML must not vaporize cards).
//! - Parse failures get a grace period, then surface as FileProblem while
//!   canonical state keeps the last good data.
//! - Commands re-read the file at edit time and verify it is unchanged
//!   before renaming over it (no stale in-memory serialization).

pub mod classify;
pub mod delta;
pub mod journal;

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use notify::{RecursiveMode, Watcher};
use toml_edit::{DocumentMut, Value};

use crate::error::{Error, Result};
use crate::id::{ShotId, Slug, TakeId};
use crate::lint::{self, has_errors};
use crate::project::{self, FileEntry, Project, ProjectState};
use crate::schema::beats::BeatsFile;
use crate::schema::character::CharacterFile;
use crate::schema::project::ProjectFile;
use crate::schema::scene::SceneFile;
use crate::schema::shots::{ShotStatus, ShotsFile};
use crate::schema::takes::TakesManifest;
use crate::schema::timeline::TimelineFile;
use crate::schema::world::WorldFile;
use crate::{atomic, doc};

pub use classify::FileKind;
pub use delta::{Delta, StoryDoc};
pub use journal::WriteJournal;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Origin {
    Startup,
    Local,
    External,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct SyncEvent {
    pub rev: u64,
    pub origin: Origin,
    pub deltas: Vec<Delta>,
}

#[derive(Debug, Clone)]
pub struct SyncOptions {
    /// Per-path quiet window before a change is processed.
    pub quiet: Duration,
    /// A hot file still flushes at this cadence under continuous writes.
    pub max_latency: Duration,
    /// How long removals are held before they are believed.
    pub removal_hold: Duration,
    /// How long a parse failure stays silent before the problem banner.
    pub problem_grace: Duration,
}

impl Default for SyncOptions {
    fn default() -> Self {
        Self {
            quiet: Duration::from_millis(150),
            max_latency: Duration::from_millis(500),
            removal_hold: Duration::from_millis(500),
            problem_grace: Duration::from_millis(800),
        }
    }
}

pub struct SyncEngine {
    inner: Arc<Inner>,
}

struct Inner {
    root: PathBuf,
    options: SyncOptions,
    state: Mutex<ProjectState>,
    journal: Mutex<WriteJournal>,
    doc_hashes: Mutex<HashMap<PathBuf, blake3::Hash>>,
    problems: Mutex<HashMap<PathBuf, String>>,
    rev: AtomicU64,
    stop: AtomicBool,
    tx: Sender<SyncEvent>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

enum RawMsg {
    Touch(PathBuf),
    Sweep,
}

impl SyncEngine {
    /// Scan the project, emit a startup event, and start watching.
    pub fn start(
        project: &Project,
        options: SyncOptions,
    ) -> Result<(SyncEngine, Receiver<SyncEvent>)> {
        let root = project.root().to_owned();
        let (tx, rx) = mpsc::channel();
        let scan = project.scan();

        let inner = Arc::new(Inner {
            root: root.clone(),
            options,
            state: Mutex::new(scan.state),
            journal: Mutex::new(WriteJournal::default()),
            doc_hashes: Mutex::new(HashMap::new()),
            problems: Mutex::new(HashMap::new()),
            rev: AtomicU64::new(0),
            stop: AtomicBool::new(false),
            tx,
        });

        for doc_rel in ["story/logline.md", "story/treatment.md"] {
            let abs = root.join(doc_rel);
            if let Ok(bytes) = fs::read(&abs) {
                lock(&inner.doc_hashes).insert(PathBuf::from(doc_rel), blake3::hash(&bytes));
            }
        }

        let mut startup_deltas = Vec::new();
        for issue in &scan.issues {
            lock(&inner.problems).insert(issue.path.clone(), issue.message.clone());
            startup_deltas.push(Delta::FileProblem {
                path: issue.path.clone(),
                message: issue.message.clone(),
            });
        }
        inner.emit(Origin::Startup, startup_deltas, true);

        let (raw_tx, raw_rx) = mpsc::channel::<RawMsg>();
        let error_tx = raw_tx.clone();
        let mut watcher = notify::recommended_watcher(
            move |result: notify::Result<notify::Event>| match result {
                Ok(event) => {
                    if event.need_rescan() {
                        let _ = error_tx.send(RawMsg::Sweep);
                    }
                    for path in event.paths {
                        let _ = error_tx.send(RawMsg::Touch(path));
                    }
                }
                Err(_) => {
                    let _ = error_tx.send(RawMsg::Sweep);
                }
            },
        )
        .map_err(|e| Error::Watch(e.to_string()))?;
        watcher
            .watch(&root, RecursiveMode::Recursive)
            .map_err(|e| Error::Watch(e.to_string()))?;

        let worker_inner = Arc::clone(&inner);
        std::thread::spawn(move || {
            // The watcher must live as long as the worker.
            let _watcher = watcher;
            worker_loop(&worker_inner, &raw_rx);
        });

        Ok((SyncEngine { inner }, rx))
    }

    pub fn root(&self) -> &Path {
        &self.inner.root
    }

    /// A clone of the canonical state (last good data for every file).
    pub fn snapshot(&self) -> ProjectState {
        lock(&self.inner.state).clone()
    }

    pub fn stop(&self) {
        self.inner.stop.store(true, Ordering::Relaxed);
    }

    /// Ask the engine to verify everything against disk (used after
    /// resume-from-sleep or window refocus; also exercised by tests).
    pub fn request_sweep(&self) {
        // The worker also sweeps on watcher errors; here we just touch all
        // tracked files by writing nothing — the next tick picks them up
        // via the integrity sweep message channel is owned by the worker,
        // so we simulate by processing directly on the caller thread.
        let paths = tracked_files(&self.inner.root);
        let mut holds = HashMap::new();
        let mut pending_problems = HashMap::new();
        for rel in paths {
            process_path(&self.inner, &rel, false, &mut holds, &mut pending_problems);
        }
    }

    // ------------------------------------------------------------------
    // Typed commands: the GUI/CLI write path. Read fresh, edit surgically,
    // verify unchanged, write atomically, journal the echo.
    // ------------------------------------------------------------------

    /// Circle (or un-circle with `None`) a take on a shot.
    pub fn circle_take(&self, scene: &Slug, shot: &ShotId, take: Option<&TakeId>) -> Result<()> {
        let rel = self.shots_rel_path(scene)?;
        let shot = shot.clone();
        let take = take.cloned();
        self.edit_toml(&rel, move |document| {
            let index = shot_index(document, &shot)?;
            match &take {
                Some(take) => doc::set_block_field(
                    document,
                    "shots",
                    index,
                    "selected_take",
                    Value::from(take.as_str()),
                ),
                None => {
                    doc::remove_block_field(document, "shots", index, "selected_take").map(|_| ())
                }
            }
        })
    }

    pub fn set_shot_status(&self, scene: &Slug, shot: &ShotId, status: ShotStatus) -> Result<()> {
        let rel = self.shots_rel_path(scene)?;
        let shot = shot.clone();
        let status: String = status.into();
        self.edit_toml(&rel, move |document| {
            let index = shot_index(document, &shot)?;
            doc::set_block_field(
                document,
                "shots",
                index,
                "status",
                Value::from(status.as_str()),
            )
        })
    }

    /// Drag-reorder a beat card.
    pub fn move_beat(&self, from: usize, to: usize) -> Result<()> {
        self.edit_toml(Path::new("story/beats.toml"), move |document| {
            doc::move_block(document, "beats", from, to)
        })
    }

    pub fn shots_rel_path(&self, scene: &Slug) -> Result<PathBuf> {
        let state = lock(&self.inner.state);
        let entry = state
            .scenes
            .iter()
            .find(|s| &s.slug == scene)
            .ok_or_else(|| Error::Project(format!("no scene `{scene}`")))?;
        let rel = entry
            .dir
            .strip_prefix(&self.inner.root)
            .map_err(|_| Error::Project(format!("scene `{scene}` is outside the project")))?;
        Ok(rel.join("shots.toml"))
    }

    /// Surgical read-modify-write on any project TOML file: read fresh,
    /// apply the edit, verify the file is unchanged, write atomically, and
    /// journal the echo so the watcher tags it local. This is THE write
    /// path for every GUI gesture.
    pub fn edit_document(
        &self,
        rel: &Path,
        op: impl Fn(&mut DocumentMut) -> Result<()>,
    ) -> Result<()> {
        self.edit_toml(rel, op)
    }

    /// Replace a text file (markdown docs) atomically, journaled as local.
    pub fn write_text_file(&self, rel: &Path, content: &str) -> Result<()> {
        let abs = self.inner.root.join(rel);
        if let Some(parent) = abs.parent() {
            fs::create_dir_all(parent).map_err(|e| Error::Io {
                path: parent.to_owned(),
                source: e,
            })?;
        }
        lock(&self.inner.journal).record(&abs, content.as_bytes());
        atomic::write_atomic(&abs, content.as_bytes())
    }

    fn edit_toml(&self, rel: &Path, op: impl Fn(&mut DocumentMut) -> Result<()>) -> Result<()> {
        let abs = self.inner.root.join(rel);
        for _attempt in 0..3 {
            let bytes = fs::read(&abs).map_err(|e| Error::Io {
                path: abs.clone(),
                source: e,
            })?;
            let text = project::text_from_bytes(&abs, &bytes)?;
            let crlf = doc::detect_crlf(&text);
            let mut document: DocumentMut = text.parse().map_err(|e| Error::Syntax(Box::new(e)))?;
            op(&mut document)?;
            let output = doc::serialize(&document, crlf);

            // Compare-and-swap: if the file changed while we edited, retry
            // against the fresh bytes rather than clobbering them.
            let current = fs::read(&abs).map_err(|e| Error::Io {
                path: abs.clone(),
                source: e,
            })?;
            if current != bytes {
                continue;
            }
            lock(&self.inner.journal).record(&abs, output.as_bytes());
            atomic::write_atomic(&abs, output.as_bytes())?;
            return Ok(());
        }
        Err(Error::Edit(format!(
            "{} kept changing while editing; try again",
            rel.display()
        )))
    }
}

impl Inner {
    fn emit(&self, origin: Origin, deltas: Vec<Delta>, force: bool) {
        if deltas.is_empty() && !force {
            return;
        }
        let rev = self.rev.fetch_add(1, Ordering::Relaxed) + 1;
        let _ = self.tx.send(SyncEvent {
            rev,
            origin,
            deltas,
        });
    }
}

fn shot_index(document: &DocumentMut, shot: &ShotId) -> Result<usize> {
    document
        .get("shots")
        .and_then(toml_edit::Item::as_array_of_tables)
        .and_then(|aot| {
            aot.iter().position(|table| {
                table
                    .get("id")
                    .and_then(toml_edit::Item::as_str)
                    .is_some_and(|id| id == shot.as_str())
            })
        })
        .ok_or_else(|| Error::Edit(format!("no shot `{shot}` in this scene")))
}

// ----------------------------------------------------------------------
// Worker: debounce, quarantine, grace, process.
// ----------------------------------------------------------------------

struct PendingTouch {
    first: Instant,
    last: Instant,
}

fn worker_loop(inner: &Arc<Inner>, raw_rx: &Receiver<RawMsg>) {
    let mut pending: HashMap<PathBuf, PendingTouch> = HashMap::new();
    let mut holds: HashMap<PathBuf, Instant> = HashMap::new();
    let mut pending_problems: HashMap<PathBuf, Instant> = HashMap::new();
    let mut scenes_dirty: Option<Instant> = None;
    let mut last_prune = Instant::now();

    loop {
        if inner.stop.load(Ordering::Relaxed) {
            break;
        }
        match raw_rx.recv_timeout(Duration::from_millis(25)) {
            Ok(RawMsg::Touch(abs)) => {
                let Ok(rel) = abs.strip_prefix(&inner.root) else {
                    continue;
                };
                let rel = rel.to_owned();
                match classify::classify(&rel) {
                    FileKind::Ignored => {}
                    FileKind::ScenesDirChange => {
                        scenes_dirty.get_or_insert_with(Instant::now);
                    }
                    _ => {
                        let now = Instant::now();
                        pending
                            .entry(rel)
                            .and_modify(|p| p.last = now)
                            .or_insert(PendingTouch {
                                first: now,
                                last: now,
                            });
                    }
                }
            }
            Ok(RawMsg::Sweep) => {
                let now = Instant::now();
                for rel in tracked_files(&inner.root) {
                    pending.entry(rel).or_insert(PendingTouch {
                        first: now,
                        last: now,
                    });
                }
                scenes_dirty.get_or_insert(now);
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }

        let now = Instant::now();
        let due: Vec<PathBuf> = pending
            .iter()
            .filter(|(_, t)| {
                now.duration_since(t.last) >= inner.options.quiet
                    || now.duration_since(t.first) >= inner.options.max_latency
            })
            .map(|(path, _)| path.clone())
            .collect();
        for rel in due {
            pending.remove(&rel);
            process_path(inner, &rel, false, &mut holds, &mut pending_problems);
        }

        if scenes_dirty.is_some_and(|since| now.duration_since(since) >= inner.options.quiet) {
            scenes_dirty = None;
            rescan_scenes(inner, false, &mut holds);
        }

        let due_holds: Vec<PathBuf> = holds
            .iter()
            .filter(|(_, deadline)| now >= **deadline)
            .map(|(path, _)| path.clone())
            .collect();
        for rel in due_holds {
            holds.remove(&rel);
            if rel == Path::new("::scenes::") {
                rescan_scenes(inner, true, &mut holds);
            } else {
                process_path(inner, &rel, true, &mut holds, &mut pending_problems);
            }
        }

        let due_problems: Vec<PathBuf> = pending_problems
            .iter()
            .filter(|(_, deadline)| now >= **deadline)
            .map(|(path, _)| path.clone())
            .collect();
        for rel in due_problems {
            pending_problems.remove(&rel);
            confirm_problem(inner, &rel);
        }

        if now.duration_since(last_prune) > Duration::from_secs(5) {
            lock(&inner.journal).prune();
            last_prune = now;
        }
    }
}

/// All files the engine tracks, as paths relative to the root.
fn tracked_files(root: &Path) -> Vec<PathBuf> {
    let mut out = vec![
        PathBuf::from("autoteur.toml"),
        PathBuf::from("takes.manifest.toml"),
        PathBuf::from("timeline.toml"),
        PathBuf::from("story/beats.toml"),
        PathBuf::from("story/logline.md"),
        PathBuf::from("story/treatment.md"),
    ];
    for dir in ["characters", "world"] {
        if let Ok(entries) = fs::read_dir(root.join(dir)) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().is_some_and(|e| e == "toml") {
                    if let Ok(rel) = path.strip_prefix(root) {
                        out.push(rel.to_owned());
                    }
                }
            }
        }
    }
    if let Ok(entries) = fs::read_dir(root.join("scenes")) {
        for entry in entries.flatten() {
            let dir = entry.path();
            if dir.is_dir() {
                for file in ["scene.toml", "shots.toml"] {
                    let path = dir.join(file);
                    if path.exists() {
                        if let Ok(rel) = path.strip_prefix(root) {
                            out.push(rel.to_owned());
                        }
                    }
                }
            }
        }
    }
    out
}

fn process_path(
    inner: &Arc<Inner>,
    rel: &Path,
    allow_removals: bool,
    holds: &mut HashMap<PathBuf, Instant>,
    pending_problems: &mut HashMap<PathBuf, Instant>,
) {
    let kind = classify::classify(rel);
    let abs = inner.root.join(rel);
    let bytes = fs::read(&abs).ok();
    let origin = match &bytes {
        Some(bytes) if lock(&inner.journal).consume(&abs, bytes) => Origin::Local,
        _ => Origin::External,
    };

    let outcome = match kind {
        FileKind::Beats => process_beats(inner, rel, bytes.as_deref()),
        FileKind::Shots { slug, .. } => process_shots(inner, rel, &slug, bytes.as_deref()),
        FileKind::Scene { slug, .. } => process_scene(inner, rel, &slug, bytes.as_deref()),
        FileKind::Character { slug } => {
            process_library::<CharacterFile>(inner, rel, &slug, bytes.as_deref(), true)
        }
        FileKind::World { slug } => {
            process_library::<WorldFile>(inner, rel, &slug, bytes.as_deref(), false)
        }
        FileKind::TakesManifest => process_takes(inner, rel, bytes.as_deref()),
        FileKind::Timeline => {
            process_simple::<TimelineFile>(inner, rel, bytes.as_deref(), Delta::TimelineChanged)
        }
        FileKind::ProjectManifest => {
            process_simple::<ProjectFile>(inner, rel, bytes.as_deref(), Delta::ProjectChanged)
        }
        FileKind::Logline => process_doc(inner, rel, bytes.as_deref(), StoryDoc::Logline),
        FileKind::Treatment => process_doc(inner, rel, bytes.as_deref(), StoryDoc::Treatment),
        FileKind::ScenesDirChange | FileKind::Ignored => Outcome::Nothing,
    };

    match outcome {
        Outcome::Nothing => {}
        Outcome::Deltas(deltas, apply) => {
            let has_removals = deltas.iter().any(Delta::is_removal);
            if has_removals && !allow_removals {
                holds.insert(rel.to_owned(), Instant::now() + inner.options.removal_hold);
                return;
            }
            apply(inner);
            let mut all = clear_problem(inner, &abs);
            all.extend(deltas);
            inner.emit(origin, all, false);
        }
        Outcome::Problem(message) => {
            let mut problems = lock(&inner.problems);
            if let Some(existing) = problems.get_mut(&abs) {
                // Still broken — refresh the detail, don't re-announce.
                *existing = message;
                return;
            }
            drop(problems);
            if pending_problems.contains_key(rel) {
                return; // grace timer already running
            }
            pending_problems.insert(rel.to_owned(), Instant::now() + inner.options.problem_grace);
        }
    }
}

/// After the grace window, re-check the file; if it is still broken,
/// surface the problem (the UI keeps showing last-good state behind it).
fn confirm_problem(inner: &Arc<Inner>, rel: &Path) {
    let abs = inner.root.join(rel);
    let bytes = fs::read(&abs).ok();
    let message = match probe_parse(rel, bytes.as_deref()) {
        Some(message) => message,
        None => return, // healed during the grace window
    };
    lock(&inner.problems).insert(abs.clone(), message.clone());
    inner.emit(
        Origin::External,
        vec![Delta::FileProblem { path: abs, message }],
        false,
    );
}

/// Parse-check a file for problem confirmation without touching state.
fn probe_parse(rel: &Path, bytes: Option<&[u8]>) -> Option<String> {
    let bytes = bytes?;
    let text = match project::text_from_bytes(rel, bytes) {
        Ok(text) => text,
        Err(e) => return Some(error_chain(&e)),
    };
    let kind = classify::classify(rel);
    let result: Option<String> = match kind {
        FileKind::Beats => parse_and_lint::<BeatsFile>(rel, &text, lint::lint_beats).err(),
        FileKind::Shots { .. } => parse_and_lint::<ShotsFile>(rel, &text, lint::lint_shots).err(),
        FileKind::Scene { .. } => parse_and_lint::<SceneFile>(rel, &text, lint::lint_scene).err(),
        FileKind::Character { .. } => {
            parse_and_lint::<CharacterFile>(rel, &text, lint::lint_character).err()
        }
        FileKind::World { .. } => parse_and_lint::<WorldFile>(rel, &text, lint::lint_world).err(),
        FileKind::TakesManifest => doc::parse::<TakesManifest>(&text)
            .err()
            .map(|e| error_chain(&e)),
        FileKind::Timeline => doc::parse::<TimelineFile>(&text)
            .err()
            .map(|e| error_chain(&e)),
        FileKind::ProjectManifest => doc::parse::<ProjectFile>(&text)
            .err()
            .map(|e| error_chain(&e)),
        _ => None,
    };
    result
}

type Apply = Box<dyn FnOnce(&Arc<Inner>) + Send>;

enum Outcome {
    Nothing,
    /// Deltas plus the state mutation to apply once accepted.
    Deltas(Vec<Delta>, Apply),
    Problem(String),
}

fn parse_and_lint<T: serde::de::DeserializeOwned>(
    _rel: &Path,
    text: &str,
    linter: impl Fn(&DocumentMut, &T) -> Vec<crate::lint::Lint>,
) -> std::result::Result<(T, Vec<crate::lint::Lint>), String> {
    match doc::parse::<T>(text) {
        Ok((data, document)) => {
            let lints = linter(&document, &data);
            if has_errors(&lints) {
                let messages: Vec<String> = lints
                    .iter()
                    .filter(|l| l.severity == crate::lint::Severity::Error)
                    .map(|l| l.message.clone())
                    .collect();
                Err(messages.join("; "))
            } else {
                Ok((data, lints))
            }
        }
        Err(e) => Err(error_chain(&e)),
    }
}

fn decode(rel: &Path, bytes: Option<&[u8]>) -> std::result::Result<Option<String>, String> {
    match bytes {
        None => Ok(None),
        Some(bytes) => project::text_from_bytes(rel, bytes)
            .map(Some)
            .map_err(|e| error_chain(&e)),
    }
}

fn process_beats(inner: &Arc<Inner>, rel: &Path, bytes: Option<&[u8]>) -> Outcome {
    let text = match decode(rel, bytes) {
        Ok(text) => text,
        Err(message) => return Outcome::Problem(message),
    };
    let parsed = match text {
        None => None,
        Some(text) => match parse_and_lint::<BeatsFile>(rel, &text, lint::lint_beats) {
            Ok((data, lints)) => Some((data, lints)),
            Err(message) => return Outcome::Problem(message),
        },
    };
    let state = lock(&inner.state);
    let old = state.beats.as_ref().map(|entry| &entry.data);
    let empty = BeatsFile {
        schema_version: 1,
        episodes: vec![],
        beats: vec![],
    };
    let new_data = parsed.as_ref().map(|(d, _)| d).unwrap_or(&empty);
    let deltas = delta::diff_beats(old, new_data);
    drop(state);
    if deltas.is_empty() {
        return Outcome::Nothing;
    }
    let abs = inner.root.join(rel);
    let apply: Apply = Box::new(move |inner: &Arc<Inner>| {
        let mut state = lock(&inner.state);
        state.beats = parsed.map(|(data, lints)| FileEntry {
            path: abs,
            data,
            lints,
        });
    });
    Outcome::Deltas(deltas, apply)
}

fn process_shots(inner: &Arc<Inner>, rel: &Path, slug: &Slug, bytes: Option<&[u8]>) -> Outcome {
    let text = match decode(rel, bytes) {
        Ok(text) => text,
        Err(message) => return Outcome::Problem(message),
    };
    let parsed = match text {
        None => None,
        Some(text) => match parse_and_lint::<ShotsFile>(rel, &text, lint::lint_shots) {
            Ok((data, lints)) => Some((data, lints)),
            Err(message) => return Outcome::Problem(message),
        },
    };
    let state = lock(&inner.state);
    let Some(index) = state.scenes.iter().position(|s| &s.slug == slug) else {
        drop(state);
        // A shots file for a scene we don't know yet: the directory just
        // appeared; rescan will pick everything up.
        return Outcome::Nothing;
    };
    let old = state.scenes[index].shots.as_ref().map(|entry| &entry.data);
    let empty = ShotsFile {
        schema_version: 1,
        shots: vec![],
    };
    let new_data = parsed.as_ref().map(|(d, _)| d).unwrap_or(&empty);
    let deltas = delta::diff_shots(slug, old, new_data);
    drop(state);
    if deltas.is_empty() {
        return Outcome::Nothing;
    }
    let abs = inner.root.join(rel);
    let slug = slug.clone();
    let apply: Apply = Box::new(move |inner: &Arc<Inner>| {
        let mut state = lock(&inner.state);
        if let Some(entry) = state.scenes.iter_mut().find(|s| s.slug == slug) {
            entry.shots = parsed.map(|(data, lints)| FileEntry {
                path: abs,
                data,
                lints,
            });
        }
    });
    Outcome::Deltas(deltas, apply)
}

fn process_scene(inner: &Arc<Inner>, rel: &Path, slug: &Slug, bytes: Option<&[u8]>) -> Outcome {
    let text = match decode(rel, bytes) {
        Ok(text) => text,
        Err(message) => return Outcome::Problem(message),
    };
    let parsed = match text {
        None => None,
        Some(text) => match parse_and_lint::<SceneFile>(rel, &text, lint::lint_scene) {
            Ok((data, lints)) => Some((data, lints)),
            Err(message) => return Outcome::Problem(message),
        },
    };
    let state = lock(&inner.state);
    let known = state.scenes.iter().any(|s| &s.slug == slug);
    let changed = state
        .scenes
        .iter()
        .find(|s| &s.slug == slug)
        .is_none_or(|s| s.scene.as_ref().map(|e| &e.data) != parsed.as_ref().map(|(d, _)| d));
    drop(state);
    if !known || !changed {
        return Outcome::Nothing;
    }
    let deltas = vec![Delta::SceneUpdated { slug: slug.clone() }];
    let abs = inner.root.join(rel);
    let slug = slug.clone();
    let apply: Apply = Box::new(move |inner: &Arc<Inner>| {
        let mut state = lock(&inner.state);
        if let Some(entry) = state.scenes.iter_mut().find(|s| s.slug == slug) {
            entry.scene = parsed.map(|(data, lints)| FileEntry {
                path: abs,
                data,
                lints,
            });
        }
    });
    Outcome::Deltas(deltas, apply)
}

fn process_library<T>(
    inner: &Arc<Inner>,
    rel: &Path,
    slug: &Slug,
    bytes: Option<&[u8]>,
    is_character: bool,
) -> Outcome
where
    T: serde::de::DeserializeOwned + PartialEq + Send + 'static,
    T: LibraryFile,
{
    let text = match decode(rel, bytes) {
        Ok(text) => text,
        Err(message) => return Outcome::Problem(message),
    };
    let parsed = match text {
        None => None,
        Some(text) => match T::parse_and_lint(rel, &text) {
            Ok(pair) => Some(pair),
            Err(message) => return Outcome::Problem(message),
        },
    };
    let state = lock(&inner.state);
    let old = T::get(&state, slug).map(|entry| &entry.data);
    let deltas = match (&old, &parsed) {
        (None, None) => vec![],
        (Some(old), Some((new, _))) if *old == new => vec![],
        (_, Some(_)) => vec![T::changed_delta(slug, is_character)],
        (Some(_), None) => vec![T::removed_delta(slug, is_character)],
    };
    drop(state);
    if deltas.is_empty() {
        return Outcome::Nothing;
    }
    let abs = inner.root.join(rel);
    let slug = slug.clone();
    let apply: Apply = Box::new(move |inner: &Arc<Inner>| {
        let mut state = lock(&inner.state);
        T::set(
            &mut state,
            &slug,
            parsed.map(|(data, lints)| FileEntry {
                path: abs,
                data,
                lints,
            }),
        );
    });
    Outcome::Deltas(deltas, apply)
}

trait LibraryFile: Sized {
    fn parse_and_lint(
        rel: &Path,
        text: &str,
    ) -> std::result::Result<(Self, Vec<crate::lint::Lint>), String>;
    fn get<'a>(state: &'a ProjectState, slug: &Slug) -> Option<&'a FileEntry<Self>>;
    fn set(state: &mut ProjectState, slug: &Slug, entry: Option<FileEntry<Self>>);
    fn changed_delta(slug: &Slug, is_character: bool) -> Delta;
    fn removed_delta(slug: &Slug, is_character: bool) -> Delta;
}

impl LibraryFile for CharacterFile {
    fn parse_and_lint(
        rel: &Path,
        text: &str,
    ) -> std::result::Result<(Self, Vec<crate::lint::Lint>), String> {
        parse_and_lint::<CharacterFile>(rel, text, lint::lint_character)
    }
    fn get<'a>(state: &'a ProjectState, slug: &Slug) -> Option<&'a FileEntry<Self>> {
        state.characters.get(slug)
    }
    fn set(state: &mut ProjectState, slug: &Slug, entry: Option<FileEntry<Self>>) {
        match entry {
            Some(entry) => {
                state.characters.insert(slug.clone(), entry);
            }
            None => {
                state.characters.remove(slug);
            }
        }
    }
    fn changed_delta(slug: &Slug, _is_character: bool) -> Delta {
        Delta::CharacterChanged { slug: slug.clone() }
    }
    fn removed_delta(slug: &Slug, _is_character: bool) -> Delta {
        Delta::CharacterRemoved { slug: slug.clone() }
    }
}

impl LibraryFile for WorldFile {
    fn parse_and_lint(
        rel: &Path,
        text: &str,
    ) -> std::result::Result<(Self, Vec<crate::lint::Lint>), String> {
        parse_and_lint::<WorldFile>(rel, text, lint::lint_world)
    }
    fn get<'a>(state: &'a ProjectState, slug: &Slug) -> Option<&'a FileEntry<Self>> {
        state.world.get(slug)
    }
    fn set(state: &mut ProjectState, slug: &Slug, entry: Option<FileEntry<Self>>) {
        match entry {
            Some(entry) => {
                state.world.insert(slug.clone(), entry);
            }
            None => {
                state.world.remove(slug);
            }
        }
    }
    fn changed_delta(slug: &Slug, _is_character: bool) -> Delta {
        Delta::WorldChanged { slug: slug.clone() }
    }
    fn removed_delta(slug: &Slug, _is_character: bool) -> Delta {
        Delta::WorldRemoved { slug: slug.clone() }
    }
}

fn process_takes(inner: &Arc<Inner>, rel: &Path, bytes: Option<&[u8]>) -> Outcome {
    let text = match decode(rel, bytes) {
        Ok(text) => text,
        Err(message) => return Outcome::Problem(message),
    };
    let parsed = match text {
        None => None,
        Some(text) => match doc::parse::<TakesManifest>(&text) {
            Ok((data, _)) => Some(data),
            Err(e) => return Outcome::Problem(error_chain(&e)),
        },
    };
    let state = lock(&inner.state);
    let old = state.takes.as_ref().map(|entry| &entry.data);
    let empty = TakesManifest {
        schema_version: 1,
        takes: vec![],
    };
    let new_data = parsed.as_ref().unwrap_or(&empty);
    let deltas = if old == Some(new_data) || (old.is_none() && parsed.is_none()) {
        vec![]
    } else {
        delta::diff_takes(old, new_data)
    };
    drop(state);
    if deltas.is_empty() {
        return Outcome::Nothing;
    }
    let abs = inner.root.join(rel);
    let apply: Apply = Box::new(move |inner: &Arc<Inner>| {
        let mut state = lock(&inner.state);
        state.takes = parsed.map(|data| FileEntry {
            path: abs,
            data,
            lints: vec![],
        });
    });
    Outcome::Deltas(deltas, apply)
}

fn process_simple<T>(inner: &Arc<Inner>, rel: &Path, bytes: Option<&[u8]>, delta: Delta) -> Outcome
where
    T: serde::de::DeserializeOwned + PartialEq + Send + 'static,
    T: SimpleSlot,
{
    let text = match decode(rel, bytes) {
        Ok(text) => text,
        Err(message) => return Outcome::Problem(message),
    };
    let parsed = match text {
        None => None,
        Some(text) => match doc::parse::<T>(&text) {
            Ok((data, _)) => Some(data),
            Err(e) => return Outcome::Problem(error_chain(&e)),
        },
    };
    let state = lock(&inner.state);
    let unchanged = T::get(&state).map(|e| &e.data) == parsed.as_ref();
    drop(state);
    if unchanged {
        return Outcome::Nothing;
    }
    let abs = inner.root.join(rel);
    let apply: Apply = Box::new(move |inner: &Arc<Inner>| {
        let mut state = lock(&inner.state);
        T::set(
            &mut state,
            parsed.map(|data| FileEntry {
                path: abs,
                data,
                lints: vec![],
            }),
        );
    });
    Outcome::Deltas(vec![delta], apply)
}

trait SimpleSlot: Sized {
    fn get(state: &ProjectState) -> Option<&FileEntry<Self>>;
    fn set(state: &mut ProjectState, entry: Option<FileEntry<Self>>);
}

impl SimpleSlot for TimelineFile {
    fn get(state: &ProjectState) -> Option<&FileEntry<Self>> {
        state.timeline.as_ref()
    }
    fn set(state: &mut ProjectState, entry: Option<FileEntry<Self>>) {
        state.timeline = entry;
    }
}

impl SimpleSlot for ProjectFile {
    fn get(state: &ProjectState) -> Option<&FileEntry<Self>> {
        state.manifest.as_ref()
    }
    fn set(state: &mut ProjectState, entry: Option<FileEntry<Self>>) {
        state.manifest = entry;
    }
}

fn process_doc(inner: &Arc<Inner>, rel: &Path, bytes: Option<&[u8]>, which: StoryDoc) -> Outcome {
    let Some(bytes) = bytes else {
        return Outcome::Nothing;
    };
    let hash = blake3::hash(bytes);
    let mut hashes = lock(&inner.doc_hashes);
    if hashes.get(rel) == Some(&hash) {
        return Outcome::Nothing;
    }
    hashes.insert(rel.to_owned(), hash);
    drop(hashes);
    Outcome::Deltas(
        vec![Delta::StoryDocChanged { doc: which }],
        Box::new(|_| {}),
    )
}

/// Re-list the scenes directory and reconcile adds/renumbers/removals.
fn rescan_scenes(inner: &Arc<Inner>, allow_removals: bool, holds: &mut HashMap<PathBuf, Instant>) {
    let Ok(project) = Project::open(&inner.root) else {
        return;
    };
    let fresh = project.scan().state.scenes;
    let state = lock(&inner.state);
    let mut deltas = Vec::new();
    for scene in &fresh {
        match state.scenes.iter().find(|s| s.slug == scene.slug) {
            None => deltas.push(Delta::SceneAdded {
                slug: scene.slug.clone(),
                number: scene.number,
            }),
            Some(old) if old.number != scene.number => deltas.push(Delta::SceneRenumbered {
                slug: scene.slug.clone(),
                number: scene.number,
            }),
            _ => {}
        }
    }
    for old in &state.scenes {
        if !fresh.iter().any(|s| s.slug == old.slug) {
            deltas.push(Delta::SceneRemoved {
                slug: old.slug.clone(),
            });
        }
    }
    drop(state);
    if deltas.is_empty() {
        return;
    }
    if deltas.iter().any(Delta::is_removal) && !allow_removals {
        holds.insert(
            PathBuf::from("::scenes::"),
            Instant::now() + inner.options.removal_hold,
        );
        return;
    }
    lock(&inner.state).scenes = fresh;
    inner.emit(Origin::External, deltas, false);
}

fn clear_problem(inner: &Arc<Inner>, abs: &Path) -> Vec<Delta> {
    if lock(&inner.problems).remove(abs).is_some() {
        vec![Delta::FileProblemCleared {
            path: abs.to_owned(),
        }]
    } else {
        vec![]
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
