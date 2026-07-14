//! Headless live-sync proof: an agent writing files next to a running
//! engine produces granular, origin-tagged deltas within the debounce
//! window — and the engine's own writes come back tagged local.

use std::fs;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use autoteur_core::id::{ShotId, Slug, TakeId};
use autoteur_core::project::Project;
use autoteur_core::schema::project::ProjectFormat;
use autoteur_core::sync::{Delta, Origin, SyncEngine, SyncEvent, SyncOptions};

const SHOTS_THREE: &str = "schema_version = 1\n\n[[shots]]\nid = \"a\"\naction = \"one\"\n\n[[shots]]\nid = \"b\"\naction = \"two\"\n\n[[shots]]\nid = \"c\"\naction = \"three\"\n";

fn slug(s: &str) -> Slug {
    Slug::new(s).expect("slug")
}

struct Harness {
    _dir: tempfile::TempDir,
    root: std::path::PathBuf,
    engine: SyncEngine,
    rx: Receiver<SyncEvent>,
}

impl Harness {
    fn new() -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let project =
            Project::create(dir.path(), "Live Sync", ProjectFormat::Feature).expect("create");
        // A scene with three shots to play with.
        let scene_dir = dir.path().join("scenes/010-vault");
        fs::create_dir_all(&scene_dir).expect("scene dir");
        fs::write(
            scene_dir.join("scene.toml"),
            "schema_version = 1\ntitle = \"Vault\"\n",
        )
        .expect("scene");
        fs::write(scene_dir.join("shots.toml"), SHOTS_THREE).expect("shots");

        let project = Project::open(project.root()).expect("open");
        let (engine, rx) = SyncEngine::start(&project, SyncOptions::default()).expect("start");
        let root = dir.path().to_owned();
        let harness = Harness {
            _dir: dir,
            root,
            engine,
            rx,
        };
        // First event is always the startup snapshot.
        let startup = harness.next_event(Duration::from_secs(5)).expect("startup");
        assert_eq!(startup.origin, Origin::Startup);
        harness
    }

    fn next_event(&self, timeout: Duration) -> Option<SyncEvent> {
        self.rx.recv_timeout(timeout).ok()
    }

    /// Collect events until `pred` matches one of their deltas (returning
    /// the event) or the deadline passes.
    fn wait_for(&self, timeout: Duration, pred: impl Fn(&Delta) -> bool) -> Option<SyncEvent> {
        let deadline = Instant::now() + timeout;
        loop {
            let remaining = deadline.checked_duration_since(Instant::now())?;
            let event = self.rx.recv_timeout(remaining).ok()?;
            if event.deltas.iter().any(&pred) {
                return Some(event);
            }
        }
    }

    /// Drain events for `window`, returning every delta seen.
    fn drain(&self, window: Duration) -> Vec<Delta> {
        let deadline = Instant::now() + window;
        let mut all = Vec::new();
        while let Some(remaining) = deadline.checked_duration_since(Instant::now()) {
            match self.rx.recv_timeout(remaining) {
                Ok(event) => all.extend(event.deltas),
                Err(_) => break,
            }
        }
        all
    }
}

#[test]
fn external_beat_write_glides_in_within_a_second() {
    let h = Harness::new();
    let beats = h.root.join("story/beats.toml");
    let mut text = fs::read_to_string(&beats).expect("read");
    text.push_str("\n[[beats]]\nid = \"cold-open\"\ntitle = \"Cold open\"\n");
    let written_at = Instant::now();
    fs::write(&beats, text).expect("write");

    let event = h
        .wait_for(
            Duration::from_secs(5),
            |d| matches!(d, Delta::BeatAdded { beat, .. } if beat.id.as_str() == "cold-open"),
        )
        .expect("BeatAdded arrives");
    assert_eq!(event.origin, Origin::External);
    // The product promise: on screen within about a second.
    let budget = if std::env::var_os("CI").is_some() {
        Duration::from_secs(10) // shared runners stall; locally we hold the 1s promise
    } else {
        Duration::from_secs(2)
    };
    assert!(
        written_at.elapsed() < budget,
        "took {:?}",
        written_at.elapsed()
    );

    // Canonical state caught up too.
    let state = h.engine.snapshot();
    assert!(state
        .beats
        .expect("beats")
        .data
        .beats
        .iter()
        .any(|b| b.id.as_str() == "cold-open"));
}

#[test]
fn engine_writes_come_back_as_local_and_are_minimal() {
    let h = Harness::new();
    let take = TakeId::new("tk_aaaaaaaaaaaa").expect("take");
    let before = fs::read_to_string(h.root.join("scenes/010-vault/shots.toml")).expect("read");

    h.engine
        .circle_take(&slug("vault"), &ShotId::new("b").expect("id"), Some(&take))
        .expect("circle");

    let event = h
        .wait_for(Duration::from_secs(5), |d| {
            matches!(d, Delta::SelectedTakeChanged { take: Some(_), .. })
        })
        .expect("SelectedTakeChanged arrives");
    assert_eq!(
        event.origin,
        Origin::Local,
        "own write must be tagged local"
    );

    let after = fs::read_to_string(h.root.join("scenes/010-vault/shots.toml")).expect("read");
    assert_eq!(
        after.lines().count(),
        before.lines().count() + 1,
        "circling adds exactly one line"
    );
    assert!(after.contains("selected_take = \"tk_aaaaaaaaaaaa\""));
}

#[test]
fn malformed_file_keeps_last_good_state_and_recovers() {
    let h = Harness::new();
    let shots = h.root.join("scenes/010-vault/shots.toml");

    fs::write(&shots, "[[shots]\nid = broken").expect("break it");
    let event = h
        .wait_for(Duration::from_secs(5), |d| {
            matches!(d, Delta::FileProblem { .. })
        })
        .expect("FileProblem arrives after grace");
    assert!(event
        .deltas
        .iter()
        .any(|d| matches!(d, Delta::FileProblem { path, .. } if path.ends_with("shots.toml"))));

    // Last-good state survives: still three shots in canonical.
    let state = h.engine.snapshot();
    let vault = state
        .scenes
        .iter()
        .find(|s| s.slug.as_str() == "vault")
        .expect("scene");
    assert_eq!(vault.shots.as_ref().expect("shots").data.shots.len(), 3);

    // Fix the file: problem clears, and the shot list diffs from last-good.
    fs::write(&shots, SHOTS_THREE.replace("three", "three, fixed")).expect("fix");
    let event = h
        .wait_for(Duration::from_secs(5), |d| {
            matches!(d, Delta::FileProblemCleared { .. })
        })
        .expect("FileProblemCleared arrives");
    assert!(
        event
            .deltas
            .iter()
            .any(|d| matches!(d, Delta::ShotUpdated { .. })),
        "the fix's content change rides the same event: {:?}",
        event.deltas
    );
}

#[test]
fn truncation_is_quarantined_but_real_removal_lands() {
    let h = Harness::new();
    let shots = h.root.join("scenes/010-vault/shots.toml");

    // Truncated-but-valid prefix (one shot), restored quickly — the kind of
    // intermediate state a non-atomic writer produces.
    let truncated = "schema_version = 1\n\n[[shots]]\nid = \"a\"\naction = \"one\"\n";
    fs::write(&shots, truncated).expect("truncate");
    std::thread::sleep(Duration::from_millis(250));
    fs::write(&shots, SHOTS_THREE).expect("restore");

    let seen = h.drain(Duration::from_secs(2));
    assert!(
        !seen.iter().any(|d| matches!(d, Delta::ShotRemoved { .. })),
        "no cards may vanish during a transient truncation: {seen:?}"
    );

    // A removal that persists is believed after the hold.
    let two = "schema_version = 1\n\n[[shots]]\nid = \"a\"\naction = \"one\"\n\n[[shots]]\nid = \"b\"\naction = \"two\"\n";
    fs::write(&shots, two).expect("remove c");
    // Generous ceiling: shared CI runners stall well past the ~700ms this
    // takes (debounce + quarantine hold) on an idle machine.
    let event = h
        .wait_for(
            Duration::from_secs(20),
            |d| matches!(d, Delta::ShotRemoved { id, .. } if id.as_str() == "c"),
        )
        .expect("persistent removal arrives");
    assert_eq!(event.origin, Origin::External);
    let state = h.engine.snapshot();
    let vault = state
        .scenes
        .iter()
        .find(|s| s.slug.as_str() == "vault")
        .expect("scene");
    assert_eq!(vault.shots.as_ref().expect("shots").data.shots.len(), 2);
}

#[test]
fn drag_reorder_via_engine_reports_moves() {
    let h = Harness::new();
    let beats = h.root.join("story/beats.toml");
    let mut text = fs::read_to_string(&beats).expect("read");
    text.push_str("\n[[beats]]\nid = \"one\"\ntitle = \"One\"\n\n[[beats]]\nid = \"two\"\ntitle = \"Two\"\n\n[[beats]]\nid = \"three\"\ntitle = \"Three\"\n");
    fs::write(&beats, text).expect("seed beats");
    h.wait_for(
        Duration::from_secs(5),
        |d| matches!(d, Delta::BeatAdded { beat, .. } if beat.id.as_str() == "three"),
    )
    .expect("seeded");

    h.engine.move_beat(2, 0).expect("move");
    let event = h
        .wait_for(
            Duration::from_secs(5),
            |d| matches!(d, Delta::BeatMoved { id, to: 0, .. } if id.as_str() == "three"),
        )
        .expect("BeatMoved arrives");
    assert_eq!(event.origin, Origin::Local);

    let state = h.engine.snapshot();
    let ids: Vec<String> = state
        .beats
        .expect("beats")
        .data
        .beats
        .iter()
        .map(|b| b.id.to_string())
        .collect();
    assert_eq!(ids, ["three", "one", "two"]);
}

#[test]
fn new_scene_directory_is_discovered() {
    let h = Harness::new();
    let dir = h.root.join("scenes/020-rooftop");
    fs::create_dir_all(&dir).expect("dir");
    fs::write(
        dir.join("scene.toml"),
        "schema_version = 1\ntitle = \"Rooftop\"\n",
    )
    .expect("scene");

    h.wait_for(
        Duration::from_secs(5),
        |d| matches!(d, Delta::SceneAdded { slug, number: 20 } if slug.as_str() == "rooftop"),
    )
    .expect("SceneAdded arrives");
    assert!(h
        .engine
        .snapshot()
        .scenes
        .iter()
        .any(|s| s.slug.as_str() == "rooftop"));
}
