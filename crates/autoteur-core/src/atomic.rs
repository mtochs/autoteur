//! Atomic file writes: temp file in the same directory, fsync, rename over
//! the target. Rename is atomic on NTFS; editors/AV/sync clients holding
//! the target cause transient sharing violations, so the rename retries
//! with backoff (Defender hold times regularly exceed 300ms).

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use crate::error::{Error, Result};

static COUNTER: AtomicU64 = AtomicU64::new(0);

const RETRY_DELAYS_MS: [u64; 7] = [10, 25, 50, 100, 200, 400, 800];

/// Temp-file prefix; the watcher hard-ignores these.
pub const TMP_PREFIX: &str = ".at-tmp-";

pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path.parent().unwrap_or(Path::new("."));
    let tmp: PathBuf = parent.join(format!(
        "{TMP_PREFIX}{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));

    let io_err = |p: &Path, e: std::io::Error| Error::Io {
        path: p.to_owned(),
        source: e,
    };

    let mut file = fs::File::create(&tmp).map_err(|e| io_err(&tmp, e))?;
    file.write_all(bytes).map_err(|e| io_err(&tmp, e))?;
    file.sync_all().map_err(|e| io_err(&tmp, e))?;
    drop(file);

    let mut last_err = None;
    for (attempt, delay) in RETRY_DELAYS_MS.iter().enumerate() {
        match fs::rename(&tmp, path) {
            Ok(()) => return Ok(()),
            Err(e) => {
                last_err = Some(e);
                if attempt + 1 < RETRY_DELAYS_MS.len() {
                    std::thread::sleep(Duration::from_millis(*delay));
                }
            }
        }
    }
    let _ = fs::remove_file(&tmp);
    Err(io_err(
        path,
        last_err.unwrap_or_else(|| std::io::Error::other("rename failed")),
    ))
}
