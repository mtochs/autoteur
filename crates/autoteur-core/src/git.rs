//! Save points: git under the hood, plain language on top. The director
//! sees "save points" with human summaries; agents see ordinary commits.

use std::collections::BTreeSet;
use std::path::Path;

use git2::{IndexAddOption, Repository, Signature};

use crate::error::Result;

#[derive(Debug, Clone, serde::Serialize)]
pub struct SavePoint {
    pub id: String,
    pub summary: String,
    pub seconds_since_epoch: i64,
}

/// Initialize a repository at `root` if one doesn't already exist.
pub fn init(root: &Path) -> Result<()> {
    if Repository::open(root).is_err() {
        Repository::init(root)?;
    }
    Ok(())
}

/// Stage everything and commit. Without an explicit message, a plain-language
/// summary is derived from the changed paths. Returns the commit id; if
/// nothing changed, returns the current HEAD without creating a commit.
pub fn save_point(root: &Path, message: Option<&str>) -> Result<String> {
    let repo = Repository::open(root)?;
    let mut index = repo.index()?;
    index.add_all(["*"].iter(), IndexAddOption::DEFAULT, None)?;
    index.write()?;
    let tree_id = index.write_tree()?;
    let tree = repo.find_tree(tree_id)?;

    let parent = match repo.head() {
        Ok(head) => Some(head.peel_to_commit()?),
        Err(_) => None,
    };
    if let Some(parent) = &parent {
        if parent.tree_id() == tree_id {
            return Ok(parent.id().to_string());
        }
    }

    let auto;
    let message = match message {
        Some(m) => m,
        None => {
            auto = auto_message(&repo, parent.as_ref(), &tree);
            &auto
        }
    };
    let signature = repo
        .signature()
        .or_else(|_| Signature::now("Autoteur", "autoteur@localhost"))?;
    let parents: Vec<&git2::Commit> = parent.iter().collect();
    let oid = repo.commit(
        Some("HEAD"),
        &signature,
        &signature,
        message,
        &tree,
        &parents,
    )?;
    Ok(oid.to_string())
}

/// The save-point timeline, newest first.
pub fn history(root: &Path, limit: usize) -> Result<Vec<SavePoint>> {
    let repo = Repository::open(root)?;
    let mut walk = match repo.revwalk() {
        Ok(w) => w,
        Err(_) => return Ok(Vec::new()),
    };
    if walk.push_head().is_err() {
        return Ok(Vec::new()); // no commits yet
    }
    let mut out = Vec::new();
    for oid in walk.take(limit) {
        let oid = oid?;
        let commit = repo.find_commit(oid)?;
        out.push(SavePoint {
            id: oid.to_string(),
            summary: commit.summary().unwrap_or("").to_owned(),
            seconds_since_epoch: commit.time().seconds(),
        });
    }
    Ok(out)
}

/// Restore the working tree to an earlier save point, recorded as a NEW
/// save point — history is never rewritten or lost.
pub fn restore(root: &Path, commit_id: &str) -> Result<String> {
    let repo = Repository::open(root)?;
    let oid = git2::Oid::from_str(commit_id)?;
    let commit = repo.find_commit(oid)?;
    let mut checkout = git2::build::CheckoutBuilder::new();
    checkout.force().remove_untracked(true);
    repo.checkout_tree(commit.tree()?.as_object(), Some(&mut checkout))?;
    drop(commit);
    let short = &commit_id[..commit_id.len().min(7)];
    save_point(root, Some(&format!("Restored the save point from {short}")))
}

fn auto_message(repo: &Repository, parent: Option<&git2::Commit>, tree: &git2::Tree) -> String {
    let parent_tree = parent.and_then(|p| p.tree().ok());
    let mut paths: Vec<String> = Vec::new();
    if let Ok(diff) = repo.diff_tree_to_tree(parent_tree.as_ref(), Some(tree), None) {
        for delta in diff.deltas() {
            if let Some(path) = delta.new_file().path().or_else(|| delta.old_file().path()) {
                paths.push(path.to_string_lossy().replace('\\', "/"));
            }
        }
    }
    summarize_paths(&paths)
}

/// Turn changed paths into a director-readable summary, e.g.
/// "Treatment updated; scene vault-breach changed; 2 cast members changed".
pub fn summarize_paths(paths: &[String]) -> String {
    let mut phrases: Vec<String> = Vec::new();
    let mut scenes: BTreeSet<&str> = BTreeSet::new();
    let mut characters = 0usize;
    let mut world = 0usize;
    let mut other = 0usize;
    let (mut beats, mut logline, mut treatment, mut takes, mut timeline) =
        (false, false, false, false, false);

    for path in paths {
        if path == "story/beats.toml" {
            beats = true;
        } else if path == "story/logline.md" {
            logline = true;
        } else if path == "story/treatment.md" {
            treatment = true;
        } else if path == "takes.manifest.toml" {
            takes = true;
        } else if path == "timeline.toml" {
            timeline = true;
        } else if let Some(rest) = path.strip_prefix("scenes/") {
            if let Some(dir) = rest.split('/').next() {
                scenes.insert(dir);
            }
        } else if path.starts_with("characters/") {
            characters += 1;
        } else if path.starts_with("world/") {
            world += 1;
        } else {
            other += 1;
        }
    }

    if treatment {
        phrases.push("treatment updated".to_owned());
    }
    if logline {
        phrases.push("logline updated".to_owned());
    }
    if beats {
        phrases.push("beat board updated".to_owned());
    }
    match scenes.len() {
        0 => {}
        1 => {
            let dir = scenes.iter().next().copied().unwrap_or_default();
            let slug = dir.split_once('-').map(|(_, s)| s).unwrap_or(dir);
            phrases.push(format!("scene {slug} changed"));
        }
        n => phrases.push(format!("{n} scenes changed")),
    }
    match characters {
        0 => {}
        1 => phrases.push("a cast member changed".to_owned()),
        n => phrases.push(format!("{n} cast members changed")),
    }
    match world {
        0 => {}
        1 => phrases.push("a location or prop changed".to_owned()),
        n => phrases.push(format!("{n} locations or props changed")),
    }
    if takes {
        phrases.push("new takes recorded".to_owned());
    }
    if timeline {
        phrases.push("the cut changed".to_owned());
    }
    if phrases.is_empty() && other > 0 {
        phrases.push(format!("{other} files changed"));
    }

    if phrases.is_empty() {
        "Save point".to_owned()
    } else {
        let mut joined = phrases.join("; ");
        if let Some(first) = joined.get_mut(0..1) {
            first.make_ascii_uppercase();
        }
        format!("Save point: {joined}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summaries_read_like_a_person_wrote_them() {
        let paths = vec![
            "story/treatment.md".to_owned(),
            "scenes/012-vault-breach/shots.toml".to_owned(),
            "characters/mara-chen.toml".to_owned(),
        ];
        assert_eq!(
            summarize_paths(&paths),
            "Save point: Treatment updated; scene vault-breach changed; a cast member changed"
        );
        assert_eq!(summarize_paths(&[]), "Save point");
    }
}
