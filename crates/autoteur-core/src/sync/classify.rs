//! Path classification for the watcher: which domain does a changed path
//! belong to, and which paths are noise to ignore entirely.

use std::path::{Component, Path};

use crate::atomic::TMP_PREFIX;
use crate::id::Slug;
use crate::schema::scene::parse_scene_dir_name;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileKind {
    ProjectManifest,
    Beats,
    Logline,
    Treatment,
    /// scene.toml inside `scenes/<dir>/`.
    Scene {
        dir: String,
        slug: Slug,
    },
    /// shots.toml inside `scenes/<dir>/`.
    Shots {
        dir: String,
        slug: Slug,
    },
    Character {
        slug: Slug,
    },
    World {
        slug: Slug,
    },
    TakesManifest,
    Timeline,
    /// Something under scenes/ that isn't a tracked file — a directory
    /// create/rename/delete. Triggers a scene-list rescan.
    ScenesDirChange,
    Ignored,
}

/// Classify a path RELATIVE to the project root.
pub fn classify(rel: &Path) -> FileKind {
    let components: Vec<&str> = rel
        .components()
        .filter_map(|c| match c {
            Component::Normal(part) => part.to_str(),
            _ => None,
        })
        .collect();

    let Some(first) = components.first() else {
        return FileKind::Ignored;
    };
    if is_noise(&components) {
        return FileKind::Ignored;
    }

    match (*first, components.len()) {
        ("autoteur.toml", 1) => FileKind::ProjectManifest,
        ("takes.manifest.toml", 1) => FileKind::TakesManifest,
        ("timeline.toml", 1) => FileKind::Timeline,
        ("story", 2) => match components[1] {
            "beats.toml" => FileKind::Beats,
            "logline.md" => FileKind::Logline,
            "treatment.md" => FileKind::Treatment,
            _ => FileKind::Ignored,
        },
        ("characters", 2) => toml_slug(components[1])
            .map(|slug| FileKind::Character { slug })
            .unwrap_or(FileKind::Ignored),
        ("world", 2) => toml_slug(components[1])
            .map(|slug| FileKind::World { slug })
            .unwrap_or(FileKind::Ignored),
        ("scenes", 1) => FileKind::ScenesDirChange,
        ("scenes", 2) => FileKind::ScenesDirChange,
        ("scenes", 3) => {
            let dir = components[1];
            let Some((_, slug)) = parse_scene_dir_name(dir) else {
                return FileKind::Ignored;
            };
            match components[2] {
                "scene.toml" => FileKind::Scene {
                    dir: dir.to_owned(),
                    slug,
                },
                "shots.toml" => FileKind::Shots {
                    dir: dir.to_owned(),
                    slug,
                },
                _ => FileKind::Ignored,
            }
        }
        _ => FileKind::Ignored,
    }
}

fn is_noise(components: &[&str]) -> bool {
    let first = components[0];
    if matches!(
        first,
        ".git" | "takes" | ".autoteur" | "target" | "node_modules"
    ) {
        return true;
    }
    components.iter().any(|part| {
        part.starts_with(TMP_PREFIX)
            || part.ends_with(".swp")
            || part.ends_with(".swx")
            || part.ends_with(".tmp")
            || part.ends_with('~')
            || part.starts_with(".#")
    })
}

fn toml_slug(file_name: &str) -> Option<Slug> {
    let stem = file_name.strip_suffix(".toml")?;
    Slug::new(stem).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn kind(path: &str) -> FileKind {
        classify(&PathBuf::from(path))
    }

    #[test]
    fn classifies_every_tracked_file() {
        assert_eq!(kind("autoteur.toml"), FileKind::ProjectManifest);
        assert_eq!(kind("story/beats.toml"), FileKind::Beats);
        assert_eq!(kind("story/treatment.md"), FileKind::Treatment);
        assert!(matches!(
            kind("scenes/012-vault-breach/shots.toml"),
            FileKind::Shots { .. }
        ));
        assert!(matches!(
            kind("characters/mara-chen.toml"),
            FileKind::Character { .. }
        ));
        assert_eq!(kind("takes.manifest.toml"), FileKind::TakesManifest);
        // A scene DIRECTORY event (rename/create) triggers a rescan.
        assert_eq!(kind("scenes/012-vault-breach"), FileKind::ScenesDirChange);
    }

    #[test]
    fn ignores_noise() {
        for noisy in [
            ".git/index",
            "takes/4c/4c9f.mp4",
            ".autoteur/view.json",
            "story/.at-tmp-123-4",
            "story/beats.toml.swp",
            "scenes/012-vault/shots.toml~",
            "story/notes.txt",
            "characters/Mara.toml", // invalid slug stem
        ] {
            assert_eq!(kind(noisy), FileKind::Ignored, "{noisy}");
        }
    }
}
