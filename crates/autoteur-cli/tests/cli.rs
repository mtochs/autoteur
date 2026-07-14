//! The full loop through the real binary: create → scene → shot →
//! dry-run generate → save points — and a real FFmpeg render when
//! FFmpeg is installed.

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use autoteur_core::id::TakeId;
use autoteur_core::schema::takes::{TakeOutput, TakeRecord};
use autoteur_core::takes_store;

fn autoteur(project: &Path) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_autoteur"));
    cmd.arg("--project").arg(project);
    cmd
}

fn run_ok(cmd: &mut Command) -> Output {
    let output = cmd.output().expect("binary runs");
    assert!(
        output.status.success(),
        "command failed\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

#[test]
fn full_loop_create_to_dry_run_generate() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path().join("cold-signal");

    // create
    let output = run_ok(
        Command::new(env!("CARGO_BIN_EXE_autoteur"))
            .arg("new")
            .arg(&root)
            .args(["--title", "Cold Signal"]),
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("Project created"));
    assert!(root.join("AGENTS.md").exists());

    // scaffold story pieces
    run_ok(autoteur(&root).args(["scene", "new", "The Vault Job"]));
    run_ok(autoteur(&root).args(["character", "new", "Mara Chen"]));
    run_ok(autoteur(&root).args(["world", "new", "Neon Noir", "--kind", "style"]));

    // an agent appends a shot
    let shots_path = root.join("scenes/010-the-vault-job/shots.toml");
    let mut shots = fs::read_to_string(&shots_path).expect("shots");
    shots.push_str(
        "\n[[shots]]\nid = \"a\"\nframing = \"wide\"\naction = \"Mara at the vault door.\"\ncharacters = [\"mara-chen\"]\nstatus = \"ready\"\n",
    );
    fs::write(&shots_path, shots).expect("write shots");

    // give mara a fragment so injection is visible
    let character_path = root.join("characters/mara-chen.toml");
    let character = fs::read_to_string(&character_path)
        .expect("character")
        .replace(
            "fragment = \"\"",
            "fragment = \"Mara Chen, gray streak, utility jacket\"",
        );
    fs::write(&character_path, character).expect("write character");

    // validate: clean exit
    run_ok(autoteur(&root).arg("validate"));

    // status --json is machine-readable and sees the shot
    let output = run_ok(autoteur(&root).args(["status", "--json"]));
    let status: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("status is valid JSON");
    assert_eq!(status["title"], "Cold Signal");
    assert_eq!(status["scenes"][0]["shots"][0]["ref"], "the-vault-job/a");
    assert_eq!(status["scenes"][0]["shots"][0]["status"], "ready");

    // dry-run generation shows the fully resolved prompt
    let output = run_ok(autoteur(&root).args([
        "generate",
        "the-vault-job/a",
        "--model",
        "acme/dream",
        "--dry-run",
    ]));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("wide shot"), "framing expands: {stdout}");
    assert!(
        stdout.contains("Mara Chen, gray streak"),
        "character fragment injects: {stdout}"
    );

    // save point + history
    fs::write(
        root.join("story/logline.md"),
        "# Logline\n\nA vault, a lie.\n",
    )
    .expect("logline");
    run_ok(autoteur(&root).args(["save", "-m", "Locked the logline"]));
    let output = run_ok(autoteur(&root).arg("history"));
    let history = String::from_utf8_lossy(&output.stdout);
    assert!(history.contains("Locked the logline"));
    assert!(history.contains("Project created"));

    // validate --json shape
    let output = run_ok(autoteur(&root).args(["validate", "--json"]));
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).expect("json");
    assert_eq!(report["ok"], true);
}

#[test]
fn validate_flags_dangling_references_without_failing_hard() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path().join("p");
    run_ok(
        Command::new(env!("CARGO_BIN_EXE_autoteur"))
            .arg("new")
            .arg(&root)
            .args(["--title", "P"]),
    );
    run_ok(autoteur(&root).args(["scene", "new", "Vault"]));
    let shots_path = root.join("scenes/010-vault/shots.toml");
    let mut shots = fs::read_to_string(&shots_path).expect("shots");
    shots.push_str("\n[[shots]]\nid = \"a\"\ncharacters = [\"ghost\"]\n");
    fs::write(&shots_path, shots).expect("write");

    let output = autoteur(&root)
        .args(["validate", "--json"])
        .output()
        .expect("runs");
    assert!(output.status.success(), "warnings alone must exit 0");
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).expect("json");
    let findings = report["findings"].as_array().expect("findings");
    assert!(
        findings
            .iter()
            .any(|f| f["message"].as_str().unwrap_or("").contains("ghost")),
        "{report}"
    );
}

#[test]
fn render_assembles_circled_takes_with_real_ffmpeg() {
    let Some(ffmpeg) = autoteur_core::render::find_ffmpeg() else {
        eprintln!("skipping: FFmpeg not installed");
        return;
    };

    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path().join("p");
    run_ok(
        Command::new(env!("CARGO_BIN_EXE_autoteur"))
            .arg("new")
            .arg(&root)
            .args(["--title", "P"]),
    );
    run_ok(autoteur(&root).args(["scene", "new", "Vault"]));

    // Two tiny clips as if a provider had generated them.
    let mut takes = Vec::new();
    for color in ["red", "blue"] {
        let clip = dir.path().join(format!("{color}.mp4"));
        let status = Command::new(&ffmpeg)
            .args(["-y", "-f", "lavfi", "-i"])
            .arg(format!("color=c={color}:size=64x64:duration=0.4:rate=12"))
            .args(["-pix_fmt", "yuv420p"])
            .arg(&clip)
            .output()
            .expect("ffmpeg runs");
        assert!(status.status.success());
        let bytes = fs::read(&clip).expect("clip bytes");
        let id = TakeId::from_media_bytes(&bytes);
        let hash = blake3::hash(&bytes).to_hex().to_string();
        let rel = format!("takes/{}/{hash}.mp4", &hash[..2]);
        let abs = root.join(&rel);
        fs::create_dir_all(abs.parent().expect("parent")).expect("mkdir");
        fs::write(&abs, &bytes).expect("place take");
        takes.push((id, hash, rel));
    }

    // Manifest records + circled takes on two shots.
    for (i, (id, hash, rel)) in takes.iter().enumerate() {
        let shot_id = if i == 0 { "a" } else { "b" };
        takes_store::append_take(
            &root,
            &TakeRecord {
                id: id.clone(),
                shot: format!("vault/{shot_id}").parse().expect("ref"),
                provider: "test".to_owned(),
                model: "test/clip".to_owned(),
                seed: None,
                cost_usd: None,
                created_at: None,
                resolved_prompt: None,
                negative_prompt: None,
                inputs: None,
                outputs: vec![TakeOutput {
                    hash: hash.clone(),
                    kind: Some("video".to_owned()),
                    path: Some(rel.clone()),
                    duration_s: None,
                }],
            },
        )
        .expect("append");
    }
    let shots_path = root.join("scenes/010-vault/shots.toml");
    let mut shots = fs::read_to_string(&shots_path).expect("shots");
    shots.push_str(&format!(
        "\n[[shots]]\nid = \"a\"\nselected_take = \"{}\"\n\n[[shots]]\nid = \"b\"\nselected_take = \"{}\"\n",
        takes[0].0, takes[1].0
    ));
    fs::write(&shots_path, shots).expect("write shots");

    run_ok(autoteur(&root).arg("validate"));
    let out_file = dir.path().join("screening.mp4");
    run_ok(autoteur(&root).arg("render").arg("-o").arg(&out_file));
    let size = fs::metadata(&out_file).expect("output exists").len();
    assert!(size > 500, "render produced a real file ({size} bytes)");
}
