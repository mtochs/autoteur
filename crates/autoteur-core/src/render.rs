//! Screening Room export: assemble the cut into one MP4 via FFmpeg.
//! Timeline entries reference shots and resolve `selected_take` live (one
//! source of truth with Dailies); with no timeline, the cut is every
//! circled take in story order. v0.1 exports picture only.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::error::{Error, Result};
use crate::id::{ShotRef, TakeId};
use crate::project::ProjectState;
use crate::schema::shots::ShotStatus;

#[derive(Debug, Clone)]
pub struct RenderEntry {
    pub shot: ShotRef,
    pub media: PathBuf,
    pub in_s: Option<f64>,
    pub out_s: Option<f64>,
}

#[derive(Debug, Clone)]
pub struct RenderPlan {
    pub entries: Vec<RenderEntry>,
}

/// Build the cut: timeline order when a timeline exists, otherwise every
/// circled, non-omitted take in story order.
pub fn build_plan(root: &Path, state: &ProjectState) -> Result<RenderPlan> {
    let mut entries = Vec::new();

    let timeline_refs: Vec<(ShotRef, Option<f64>, Option<f64>)> = state
        .timeline
        .iter()
        .flat_map(|t| {
            t.data
                .entries
                .iter()
                .chain(t.data.sequences.iter().flat_map(|s| s.entries.iter()))
                .map(|e| (e.shot.clone(), e.in_s, e.out_s))
        })
        .collect();

    if !timeline_refs.is_empty() {
        for (shot_ref, in_s, out_s) in timeline_refs {
            let take = selected_take_of(state, &shot_ref)?.ok_or_else(|| {
                Error::Project(format!(
                    "`{shot_ref}` is in the cut but has no circled take — pick one in Dailies"
                ))
            })?;
            entries.push(RenderEntry {
                media: media_path(root, state, &shot_ref, &take)?,
                shot: shot_ref,
                in_s,
                out_s,
            });
        }
    } else {
        for scene in &state.scenes {
            let Some(shots) = &scene.shots else { continue };
            for shot in &shots.data.shots {
                if shot.status == ShotStatus::Omitted {
                    continue;
                }
                if let Some(take) = &shot.selected_take {
                    let shot_ref = ShotRef::new(scene.slug.clone(), shot.id.clone());
                    entries.push(RenderEntry {
                        media: media_path(root, state, &shot_ref, take)?,
                        shot: shot_ref,
                        in_s: None,
                        out_s: None,
                    });
                }
            }
        }
    }

    if entries.is_empty() {
        return Err(Error::Project(
            "nothing to render yet — circle takes in Dailies first".to_owned(),
        ));
    }
    Ok(RenderPlan { entries })
}

fn selected_take_of(state: &ProjectState, shot_ref: &ShotRef) -> Result<Option<TakeId>> {
    let scene = state
        .scenes
        .iter()
        .find(|s| s.slug == shot_ref.scene)
        .ok_or_else(|| {
            Error::Project(format!(
                "the cut references unknown scene `{}`",
                shot_ref.scene
            ))
        })?;
    let shot = scene
        .shots
        .as_ref()
        .and_then(|f| f.data.shots.iter().find(|s| s.id == shot_ref.shot))
        .ok_or_else(|| Error::Project(format!("the cut references unknown shot `{shot_ref}`")))?;
    Ok(shot.selected_take.clone())
}

fn media_path(
    root: &Path,
    state: &ProjectState,
    shot_ref: &ShotRef,
    take: &TakeId,
) -> Result<PathBuf> {
    let record = state
        .takes
        .iter()
        .flat_map(|m| m.data.takes.iter())
        .find(|t| &t.id == take)
        .ok_or_else(|| {
            Error::Project(format!(
                "take `{take}` circled on `{shot_ref}` isn't in the manifest"
            ))
        })?;
    let rel = record
        .outputs
        .first()
        .and_then(|o| o.path.clone())
        .ok_or_else(|| Error::Project(format!("take `{take}` has no recorded media path")))?;
    let abs = root.join(&rel);
    if !abs.exists() {
        return Err(Error::Project(format!(
            "media for `{shot_ref}` is missing locally ({rel}) — regenerate it, or sync takes/ from wherever it was made"
        )));
    }
    Ok(abs)
}

/// Locate FFmpeg: `AUTOTEUR_FFMPEG`, then PATH.
pub fn find_ffmpeg() -> Option<PathBuf> {
    if let Ok(configured) = std::env::var("AUTOTEUR_FFMPEG") {
        let path = PathBuf::from(configured);
        if path.is_file() {
            return Some(path);
        }
    }
    let name = if cfg!(windows) {
        "ffmpeg.exe"
    } else {
        "ffmpeg"
    };
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|dir| dir.join(name))
        .find(|candidate| candidate.is_file())
}

/// Two passes: normalize every entry (accurate trim, uniform codec), then
/// concat losslessly. Trims clamp naturally — FFmpeg stops at end of media.
pub fn render(plan: &RenderPlan, ffmpeg: &Path, output: &Path) -> Result<()> {
    let staging = std::env::temp_dir().join(format!("autoteur-render-{}", std::process::id()));
    fs::create_dir_all(&staging).map_err(|e| Error::Io {
        path: staging.clone(),
        source: e,
    })?;

    let result = render_inner(plan, ffmpeg, output, &staging);
    let _ = fs::remove_dir_all(&staging);
    result
}

fn render_inner(plan: &RenderPlan, ffmpeg: &Path, output: &Path, staging: &Path) -> Result<()> {
    let mut list = String::new();
    for (i, entry) in plan.entries.iter().enumerate() {
        let segment = staging.join(format!("seg{i:04}.mp4"));
        let mut cmd = Command::new(ffmpeg);
        cmd.arg("-y").arg("-i").arg(&entry.media);
        if let Some(in_s) = entry.in_s {
            cmd.arg("-ss").arg(format!("{in_s}"));
        }
        if let Some(out_s) = entry.out_s {
            cmd.arg("-to").arg(format!("{out_s}"));
        }
        cmd.args([
            "-an", // v0.1 exports picture only
            "-c:v", "libx264", "-pix_fmt", "yuv420p", "-preset", "veryfast",
        ])
        .arg(&segment);
        run_ffmpeg(cmd, &format!("cutting `{}`", entry.shot))?;
        let quoted = segment
            .to_string_lossy()
            .replace('\\', "/")
            .replace('\'', "'\\''");
        list.push_str(&format!("file '{quoted}'\n"));
    }

    let list_path = staging.join("concat.txt");
    fs::write(&list_path, list).map_err(|e| Error::Io {
        path: list_path.clone(),
        source: e,
    })?;

    let mut cmd = Command::new(ffmpeg);
    cmd.args(["-y", "-f", "concat", "-safe", "0", "-i"])
        .arg(&list_path)
        .args(["-c", "copy"])
        .arg(output);
    run_ffmpeg(cmd, "assembling the cut")
}

fn run_ffmpeg(mut cmd: Command, doing: &str) -> Result<()> {
    let output = cmd
        .output()
        .map_err(|e| Error::Generation(format!("couldn't run FFmpeg while {doing}: {e}")))?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    let tail: String = stderr
        .lines()
        .rev()
        .take(6)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<Vec<_>>()
        .join("\n");
    Err(Error::Generation(format!(
        "FFmpeg failed while {doing}:\n{tail}"
    )))
}
