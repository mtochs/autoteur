//! Granular state deltas and the diff functions that produce them. Deltas
//! are keyed by stable ids with order read from array position, so one
//! card can glide instead of a board repainting — and so the Activity feed
//! can say "3 new takes for Shot 12B" instead of "a file changed".

use std::collections::BTreeSet;
use std::path::PathBuf;

use crate::id::{ShotId, Slug, TakeId};
use crate::schema::beats::{Beat, BeatsFile, Episode};
use crate::schema::shots::{Shot, ShotsFile};
use crate::schema::takes::{TakeRecord, TakesManifest};

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum StoryDoc {
    Logline,
    Treatment,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(tag = "type")]
pub enum Delta {
    // Beat Board
    BeatAdded {
        beat: Beat,
        index: usize,
    },
    BeatUpdated {
        beat: Beat,
        index: usize,
    },
    BeatRemoved {
        id: Slug,
    },
    BeatMoved {
        id: Slug,
        from: usize,
        to: usize,
    },
    EpisodesChanged {
        episodes: Vec<Episode>,
    },
    // Scenes
    SceneAdded {
        slug: Slug,
        number: u32,
    },
    SceneUpdated {
        slug: Slug,
    },
    SceneRenumbered {
        slug: Slug,
        number: u32,
    },
    SceneRemoved {
        slug: Slug,
    },
    // Shot List / Dailies
    ShotAdded {
        scene: Slug,
        shot: Shot,
        index: usize,
    },
    ShotUpdated {
        scene: Slug,
        shot: Shot,
        index: usize,
    },
    ShotRemoved {
        scene: Slug,
        id: ShotId,
    },
    ShotMoved {
        scene: Slug,
        id: ShotId,
        from: usize,
        to: usize,
    },
    SelectedTakeChanged {
        scene: Slug,
        id: ShotId,
        take: Option<TakeId>,
    },
    // Casting & World
    CharacterChanged {
        slug: Slug,
    },
    CharacterRemoved {
        slug: Slug,
    },
    WorldChanged {
        slug: Slug,
    },
    WorldRemoved {
        slug: Slug,
    },
    // Machine files
    TakesAdded {
        takes: Vec<TakeRecord>,
    },
    TakesChanged,
    TimelineChanged,
    ProjectChanged,
    StoryDocChanged {
        doc: StoryDoc,
    },
    // File health (last-good state stays on screen behind these)
    FileProblem {
        path: PathBuf,
        message: String,
    },
    FileProblemCleared {
        path: PathBuf,
    },
}

impl Delta {
    /// True for deltas that remove an entity — these are quarantined until
    /// a removal survives a second clean parse, because a truncated-but-
    /// valid TOML prefix from a non-atomic writer must not vaporize cards.
    pub fn is_removal(&self) -> bool {
        matches!(
            self,
            Delta::BeatRemoved { .. }
                | Delta::SceneRemoved { .. }
                | Delta::ShotRemoved { .. }
                | Delta::CharacterRemoved { .. }
                | Delta::WorldRemoved { .. }
        )
    }
}

pub fn diff_beats(old: Option<&BeatsFile>, new: &BeatsFile) -> Vec<Delta> {
    let empty = BeatsFile {
        schema_version: 1,
        episodes: vec![],
        beats: vec![],
    };
    let old = old.unwrap_or(&empty);
    let mut deltas = Vec::new();

    if old.episodes != new.episodes {
        deltas.push(Delta::EpisodesChanged {
            episodes: new.episodes.clone(),
        });
    }

    let ordered = diff_ordered(&old.beats, &new.beats, |b: &Beat| b.id.clone());
    for (index, beat) in ordered.added {
        deltas.push(Delta::BeatAdded {
            beat: beat.clone(),
            index,
        });
    }
    for id in ordered.removed {
        deltas.push(Delta::BeatRemoved { id });
    }
    for (index, beat) in ordered.updated {
        deltas.push(Delta::BeatUpdated {
            beat: beat.clone(),
            index,
        });
    }
    for (id, from, to) in ordered.moved {
        deltas.push(Delta::BeatMoved { id, from, to });
    }
    deltas
}

pub fn diff_shots(scene: &Slug, old: Option<&ShotsFile>, new: &ShotsFile) -> Vec<Delta> {
    let empty = ShotsFile {
        schema_version: 1,
        shots: vec![],
    };
    let old = old.unwrap_or(&empty);
    let mut deltas = Vec::new();

    let ordered = diff_ordered(&old.shots, &new.shots, |s: &Shot| s.id.clone());
    for (index, shot) in ordered.added {
        deltas.push(Delta::ShotAdded {
            scene: scene.clone(),
            shot: shot.clone(),
            index,
        });
    }
    for id in ordered.removed {
        deltas.push(Delta::ShotRemoved {
            scene: scene.clone(),
            id,
        });
    }
    for (index, shot) in ordered.updated {
        // Circling is the highest-signal gesture in the app; give it its
        // own delta when it is the only change.
        let old_shot = old.shots.iter().find(|s| s.id == shot.id);
        let only_take_changed = old_shot.is_some_and(|o| {
            o.selected_take != shot.selected_take && {
                let mut normalized = shot.clone();
                normalized.selected_take = o.selected_take.clone();
                normalized == *o
            }
        });
        if only_take_changed {
            deltas.push(Delta::SelectedTakeChanged {
                scene: scene.clone(),
                id: shot.id.clone(),
                take: shot.selected_take.clone(),
            });
        } else {
            deltas.push(Delta::ShotUpdated {
                scene: scene.clone(),
                shot: shot.clone(),
                index,
            });
        }
    }
    for (id, from, to) in ordered.moved {
        deltas.push(Delta::ShotMoved {
            scene: scene.clone(),
            id,
            from,
            to,
        });
    }
    deltas
}

pub fn diff_takes(old: Option<&TakesManifest>, new: &TakesManifest) -> Vec<Delta> {
    let old_ids: BTreeSet<&TakeId> = old
        .map(|m| m.takes.iter().map(|t| &t.id).collect())
        .unwrap_or_default();
    let added: Vec<TakeRecord> = new
        .takes
        .iter()
        .filter(|t| !old_ids.contains(&t.id))
        .cloned()
        .collect();
    if !added.is_empty() {
        vec![Delta::TakesAdded { takes: added }]
    } else if old.is_none_or(|o| o != new) {
        vec![Delta::TakesChanged]
    } else {
        vec![]
    }
}

struct OrderedDiff<'a, T, Id> {
    added: Vec<(usize, &'a T)>,
    removed: Vec<Id>,
    updated: Vec<(usize, &'a T)>,
    moved: Vec<(Id, usize, usize)>,
}

fn diff_ordered<'a, T: PartialEq, Id: Ord + Clone>(
    old: &'a [T],
    new: &'a [T],
    id_of: impl Fn(&T) -> Id,
) -> OrderedDiff<'a, T, Id> {
    let old_ids: Vec<Id> = old.iter().map(&id_of).collect();
    let new_ids: Vec<Id> = new.iter().map(&id_of).collect();
    let old_set: BTreeSet<&Id> = old_ids.iter().collect();
    let new_set: BTreeSet<&Id> = new_ids.iter().collect();

    let mut diff = OrderedDiff {
        added: Vec::new(),
        removed: Vec::new(),
        updated: Vec::new(),
        moved: Vec::new(),
    };

    for (id, item) in old_ids.iter().zip(old) {
        let _ = item;
        if !new_set.contains(id) {
            diff.removed.push(id.clone());
        }
    }
    for (index, (id, item)) in new_ids.iter().zip(new).enumerate() {
        if !old_set.contains(id) {
            diff.added.push((index, item));
        } else {
            let old_item = old_ids
                .iter()
                .position(|o| o == id)
                .and_then(|i| old.get(i));
            if old_item.is_some_and(|o| o != item) {
                diff.updated.push((index, item));
            }
        }
    }

    // Moves: compare the order of ids common to both sides. When the
    // common subsequence order changed, report each displaced id with its
    // absolute old/new indices (what a UI animates).
    let old_common: Vec<&Id> = old_ids.iter().filter(|id| new_set.contains(id)).collect();
    let new_common: Vec<&Id> = new_ids.iter().filter(|id| old_set.contains(id)).collect();
    for (new_pos, id) in new_common.iter().enumerate() {
        let old_pos = old_common.iter().position(|o| o == id);
        if old_pos.is_some_and(|p| p != new_pos) {
            let from = old_ids.iter().position(|o| &o == id).unwrap_or(0);
            let to = new_ids.iter().position(|n| &n == id).unwrap_or(0);
            diff.moved.push(((*id).clone(), from, to));
        }
    }
    diff
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::beats::Beat;

    fn beat(id: &str, title: &str) -> Beat {
        Beat {
            id: Slug::new(id).expect("slug"),
            title: title.to_owned(),
            summary: None,
            episode: None,
            act: None,
            color: None,
            notes: None,
        }
    }

    fn beats_file(beats: Vec<Beat>) -> BeatsFile {
        BeatsFile {
            schema_version: 1,
            episodes: vec![],
            beats,
        }
    }

    #[test]
    fn add_update_remove_and_move_are_distinguished() {
        let old = beats_file(vec![beat("a", "A"), beat("b", "B"), beat("c", "C")]);

        // Pure append.
        let new = beats_file(vec![
            beat("a", "A"),
            beat("b", "B"),
            beat("c", "C"),
            beat("d", "D"),
        ]);
        let deltas = diff_beats(Some(&old), &new);
        assert_eq!(deltas.len(), 1);
        assert!(matches!(&deltas[0], Delta::BeatAdded { index: 3, .. }));

        // Retitle only.
        let new = beats_file(vec![beat("a", "A!"), beat("b", "B"), beat("c", "C")]);
        let deltas = diff_beats(Some(&old), &new);
        assert_eq!(deltas.len(), 1);
        assert!(matches!(&deltas[0], Delta::BeatUpdated { index: 0, .. }));

        // Drag c to the front: every displaced card reports a move, no
        // adds/updates/removes.
        let new = beats_file(vec![beat("c", "C"), beat("a", "A"), beat("b", "B")]);
        let deltas = diff_beats(Some(&old), &new);
        assert!(deltas.iter().all(|d| matches!(d, Delta::BeatMoved { .. })));
        assert!(deltas
            .iter()
            .any(|d| matches!(d, Delta::BeatMoved { from: 2, to: 0, .. })));

        // An insert does NOT report moves for merely shifted cards.
        let new = beats_file(vec![
            beat("a", "A"),
            beat("x", "X"),
            beat("b", "B"),
            beat("c", "C"),
        ]);
        let deltas = diff_beats(Some(&old), &new);
        assert_eq!(deltas.len(), 1, "{deltas:?}");
        assert!(matches!(&deltas[0], Delta::BeatAdded { index: 1, .. }));
    }

    #[test]
    fn circling_gets_its_own_delta() {
        use crate::schema::shots::{Shot, ShotStatus};
        let scene = Slug::new("vault-breach").expect("slug");
        let base = Shot {
            id: crate::id::ShotId::new("a").expect("id"),
            framing: None,
            camera: None,
            action: None,
            characters: None,
            world: None,
            dialogue: vec![],
            duration_s: None,
            status: ShotStatus::Ready,
            selected_take: None,
            prompt: None,
            prompt_extra: None,
            negative_extra: None,
            notes: None,
        };
        let old = ShotsFile {
            schema_version: 1,
            shots: vec![base.clone()],
        };
        let mut circled = base.clone();
        circled.selected_take = Some(crate::id::TakeId::new("tk_aaaaaaaaaaaa").expect("id"));
        let new = ShotsFile {
            schema_version: 1,
            shots: vec![circled],
        };
        let deltas = diff_shots(&scene, Some(&old), &new);
        assert_eq!(deltas.len(), 1);
        assert!(matches!(
            &deltas[0],
            Delta::SelectedTakeChanged { take: Some(_), .. }
        ));
    }
}
