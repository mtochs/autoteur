//! The generation job queue — "the lab" in Dailies. Jobs run on a worker
//! thread; every stage change streams out as a JobUpdate so the UI can
//! show shots processing and takes arriving. Results land in the
//! content-addressed store + manifest; the live-sync watcher turns that
//! into TakesAdded deltas like any other file change.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::error::Result;
use crate::id::{ShotRef, TakeId};
use crate::provider::{GenerationRequest, ProviderRegistry};
use crate::schema::takes::TakeRecord;
use crate::takes_store;

#[derive(Debug, Clone)]
pub struct GenerationJob {
    pub shot: ShotRef,
    pub provider: String,
    pub request: GenerationRequest,
    /// Snapshot of the resolved prompt, recorded immutably in the manifest.
    pub resolved_prompt: Option<String>,
    pub negative_prompt: Option<String>,
    pub seed: Option<i64>,
}

#[derive(Debug, Clone)]
pub enum JobStage {
    Queued,
    Running,
    Done { take: TakeId, deduplicated: bool },
    Failed { message: String },
}

#[derive(Debug, Clone)]
pub struct JobUpdate {
    pub job: u64,
    pub shot: ShotRef,
    pub stage: JobStage,
}

/// How the queue looks up API keys (the GUI reads the credential store;
/// tests inject a closure).
pub type KeyLookup = Arc<dyn Fn(&str) -> Result<Option<String>> + Send + Sync>;

pub struct GenerationQueue {
    jobs_tx: Option<Sender<(u64, GenerationJob)>>,
    updates_tx: Sender<JobUpdate>,
    next_id: AtomicU64,
    worker: Option<std::thread::JoinHandle<()>>,
}

impl GenerationQueue {
    pub fn start(
        root: PathBuf,
        registry: ProviderRegistry,
        keys: KeyLookup,
    ) -> (Self, Receiver<JobUpdate>) {
        let (jobs_tx, jobs_rx) = mpsc::channel::<(u64, GenerationJob)>();
        let (updates_tx, updates_rx) = mpsc::channel::<JobUpdate>();
        let worker_updates = updates_tx.clone();
        let worker = std::thread::spawn(move || {
            for (id, job) in jobs_rx {
                run_job(&root, &registry, &keys, &worker_updates, id, job);
            }
        });
        (
            Self {
                jobs_tx: Some(jobs_tx),
                updates_tx,
                next_id: AtomicU64::new(1),
                worker: Some(worker),
            },
            updates_rx,
        )
    }

    pub fn submit(&self, job: GenerationJob) -> u64 {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let _ = self.updates_tx.send(JobUpdate {
            job: id,
            shot: job.shot.clone(),
            stage: JobStage::Queued,
        });
        if let Some(tx) = &self.jobs_tx {
            let _ = tx.send((id, job));
        }
        id
    }

    /// Finish queued work and stop the worker.
    pub fn shutdown(mut self) {
        self.jobs_tx = None;
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

impl Drop for GenerationQueue {
    fn drop(&mut self) {
        self.jobs_tx = None;
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn run_job(
    root: &std::path::Path,
    registry: &ProviderRegistry,
    keys: &KeyLookup,
    updates: &Sender<JobUpdate>,
    id: u64,
    job: GenerationJob,
) {
    let send = |stage: JobStage| {
        let _ = updates.send(JobUpdate {
            job: id,
            shot: job.shot.clone(),
            stage,
        });
    };
    send(JobStage::Running);

    let Some(provider) = registry.get(&job.provider) else {
        send(JobStage::Failed {
            message: format!("unknown provider `{}`", job.provider),
        });
        return;
    };
    let key = match keys(&job.provider) {
        Ok(Some(key)) => key,
        Ok(None) => {
            send(JobStage::Failed {
                message: format!(
                    "no API key saved for {} — connect your studio in Studio Settings",
                    provider.display_name()
                ),
            });
            return;
        }
        Err(e) => {
            send(JobStage::Failed {
                message: format!("couldn't read the API key: {e}"),
            });
            return;
        }
    };

    let result = match provider.generate(&key, &job.request) {
        Ok(result) => result,
        Err(e) => {
            send(JobStage::Failed {
                message: error_chain(&e),
            });
            return;
        }
    };
    let stored = match takes_store::store_outputs(root, &result.outputs) {
        Ok(stored) => stored,
        Err(e) => {
            send(JobStage::Failed {
                message: error_chain(&e),
            });
            return;
        }
    };
    let record = TakeRecord {
        id: stored.id.clone(),
        shot: job.shot.clone(),
        provider: job.provider.clone(),
        model: job.request.model.clone(),
        seed: job.seed,
        cost_usd: result.cost_usd,
        created_at: Some(rfc3339_utc_now()),
        resolved_prompt: job.resolved_prompt.clone(),
        negative_prompt: job.negative_prompt.clone(),
        inputs: json_to_toml_table(&job.request.inputs),
        outputs: stored.outputs.clone(),
    };
    match takes_store::append_take(root, &record) {
        Ok(fresh) => send(JobStage::Done {
            take: stored.id,
            deduplicated: !fresh,
        }),
        Err(e) => send(JobStage::Failed {
            message: error_chain(&e),
        }),
    }
}

fn json_to_toml_table(value: &serde_json::Value) -> Option<toml::Table> {
    let cleaned = strip_nulls(value.clone())?;
    match toml::Value::try_from(cleaned) {
        Ok(toml::Value::Table(table)) => Some(table),
        _ => None,
    }
}

/// TOML has no null; drop null values so provider inputs round-trip.
fn strip_nulls(value: serde_json::Value) -> Option<serde_json::Value> {
    match value {
        serde_json::Value::Null => None,
        serde_json::Value::Array(items) => Some(serde_json::Value::Array(
            items.into_iter().filter_map(strip_nulls).collect(),
        )),
        serde_json::Value::Object(map) => Some(serde_json::Value::Object(
            map.into_iter()
                .filter_map(|(k, v)| strip_nulls(v).map(|v| (k, v)))
                .collect(),
        )),
        other => Some(other),
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

/// RFC 3339 UTC timestamp without pulling in a date crate.
fn rfc3339_utc_now() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let days = (seconds / 86_400) as i64;
    let rem = seconds % 86_400;
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}

/// Howard Hinnant's civil-from-days algorithm.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if month <= 2 { year + 1 } else { year }, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamps_are_rfc3339() {
        let ts = rfc3339_utc_now();
        assert_eq!(ts.len(), 20, "{ts}");
        assert!(ts.ends_with('Z'));
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(19_723), (2024, 1, 1));
    }

    #[test]
    fn nulls_are_stripped_from_inputs() {
        let inputs = serde_json::json!({
            "prompt": "a vault",
            "seed": null,
            "nested": { "keep": 1, "drop": null }
        });
        let table = json_to_toml_table(&inputs).expect("table");
        assert!(table.contains_key("prompt"));
        assert!(!table.contains_key("seed"));
        let nested = table["nested"].as_table().expect("nested");
        assert!(nested.contains_key("keep") && !nested.contains_key("drop"));
    }
}
