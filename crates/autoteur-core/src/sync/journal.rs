//! Write journal for echo suppression — of ORIGIN only. When the engine
//! writes a file it records the content hash here; when the watcher later
//! sees that exact content, the change is tagged `local` instead of
//! `external`. A journal hit NEVER skips the differ: a true echo diffs to
//! zero deltas anyway, and skipping on trust is how a stale hash swallows
//! a genuine agent write (e.g. `git restore` reverting to prior bytes).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const ENTRY_TTL: Duration = Duration::from_secs(2);

#[derive(Default)]
pub struct WriteJournal {
    entries: HashMap<PathBuf, Vec<(blake3::Hash, Instant)>>,
}

impl WriteJournal {
    /// Record a write the engine itself performed.
    pub fn record(&mut self, path: &Path, content: &[u8]) {
        self.entries
            .entry(path.to_owned())
            .or_default()
            .push((blake3::hash(content), Instant::now()));
    }

    /// True when `content` matches a fresh entry for `path` (consumed).
    pub fn consume(&mut self, path: &Path, content: &[u8]) -> bool {
        let hash = blake3::hash(content);
        let Some(list) = self.entries.get_mut(path) else {
            return false;
        };
        let now = Instant::now();
        let before = list.len();
        list.retain(|(_, at)| now.duration_since(*at) < ENTRY_TTL);
        let _expired = before - list.len();
        if let Some(pos) = list.iter().position(|(h, _)| *h == hash) {
            list.remove(pos);
            if list.is_empty() {
                self.entries.remove(path);
            }
            true
        } else {
            false
        }
    }

    /// Drop expired entries (called periodically by the engine tick).
    pub fn prune(&mut self) {
        let now = Instant::now();
        self.entries.retain(|_, list| {
            list.retain(|(_, at)| now.duration_since(*at) < ENTRY_TTL);
            !list.is_empty()
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_only_exact_fresh_content() {
        let mut journal = WriteJournal::default();
        let path = Path::new("story/beats.toml");
        journal.record(path, b"hello");
        assert!(
            !journal.consume(path, b"different"),
            "other content is external"
        );
        assert!(journal.consume(path, b"hello"), "own write is local");
        assert!(!journal.consume(path, b"hello"), "entries are single-use");
    }
}
