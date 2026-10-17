//! Job state machine: submission, status, cancellation, and result retrieval.
//!
//! # State transitions
//! ```text
//! (submit) → Pending → Running → Done { result }
//!                              → Failed { message }
//!            Pending → Cancelled   (DELETE before task starts)
//!            Running → Cancelled   (DELETE while task runs)
//! ```
//!
//! Cancellation is advisory: a cancel signal is sent over a oneshot channel.
//! The subprocess task honours it by killing the child process. If the task
//! completes before the cancel signal arrives the `Done`/`Failed` state is kept.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Instant;

use serde::Serialize;
use solidcraft_manufacture::PathSegment;
use uuid::Uuid;

use crate::config::MAX_STORED_JOBS;

// ── JobResult ─────────────────────────────────────────────────────────────────

/// The output of a successful CAM job.
#[derive(Debug, Clone, Serialize)]
pub struct JobResult {
    /// Normalised toolpath segments in WCS millimetres.
    pub segments: Vec<PathSegment>,
    /// Optional post-processed G-code string from the adapter.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub post_output: Option<String>,
}

// ── JobPhase ──────────────────────────────────────────────────────────────────

/// Observable state of a single job.
#[derive(Debug)]
pub enum JobPhase {
    Pending,
    Running,
    Done { result: JobResult },
    Failed { message: String },
    Cancelled,
}

impl JobPhase {
    pub fn status_str(&self) -> &'static str {
        match self {
            JobPhase::Pending => "pending",
            JobPhase::Running => "running",
            JobPhase::Done { .. } => "done",
            JobPhase::Failed { .. } => "failed",
            JobPhase::Cancelled => "cancelled",
        }
    }
}

// ── Job ───────────────────────────────────────────────────────────────────────

/// A single submitted job stored in [`JobStore`].
pub struct Job {
    pub id: Uuid,
    pub submitted_at: Instant,
    pub phase: JobPhase,
    /// Alive only while the job is Pending or Running; taken and fired by DELETE.
    pub cancel_tx: Option<tokio::sync::oneshot::Sender<()>>,
}

// ── JobStore ──────────────────────────────────────────────────────────────────

/// Thread-safe map of all in-memory jobs.
pub struct JobStore {
    inner: Mutex<HashMap<Uuid, Job>>,
}

impl Default for JobStore {
    fn default() -> Self {
        Self::new()
    }
}

impl JobStore {
    pub fn new() -> Self {
        Self { inner: Mutex::new(HashMap::new()) }
    }

    /// Insert a new Pending job. Returns `Err` when the store is at capacity.
    pub fn insert(&self, id: Uuid, cancel_tx: tokio::sync::oneshot::Sender<()>) -> Result<(), &'static str> {
        let mut map = lock(&self.inner);
        if map.len() >= MAX_STORED_JOBS {
            return Err("job store full");
        }
        map.insert(id, Job { id, submitted_at: Instant::now(), phase: JobPhase::Pending, cancel_tx: Some(cancel_tx) });
        Ok(())
    }

    /// Transition an existing job to `Running`.
    pub fn set_running(&self, id: Uuid) {
        let mut map = lock(&self.inner);
        if let Some(job) = map.get_mut(&id)
            && matches!(job.phase, JobPhase::Pending)
        {
            job.phase = JobPhase::Running;
        }
    }

    /// Finalise a job with the result of the subprocess.
    pub fn finish(&self, id: Uuid, outcome: Result<JobResult, String>) {
        let mut map = lock(&self.inner);
        // Only transition from Running (guard against a race with cancel).
        if let Some(job) = map.get_mut(&id)
            && matches!(job.phase, JobPhase::Running)
        {
            job.phase = match outcome {
                Ok(r) => JobPhase::Done { result: r },
                Err(msg) => JobPhase::Failed { message: msg },
            };
            job.cancel_tx = None;
        }
    }

    /// Attempt to cancel a job. Returns:
    /// - `Ok(true)` when the cancel signal was sent.
    /// - `Ok(false)` when the job was already terminal (Done/Failed/Cancelled).
    /// - `Err` when the job ID is unknown.
    pub fn cancel(&self, id: Uuid) -> Result<bool, &'static str> {
        let mut map = lock(&self.inner);
        let job = map.get_mut(&id).ok_or("not found")?;
        match job.phase {
            JobPhase::Done { .. } | JobPhase::Failed { .. } | JobPhase::Cancelled => Ok(false),
            JobPhase::Pending | JobPhase::Running => {
                // Take the sender; the task's select! branch fires on drop.
                if let Some(tx) = job.cancel_tx.take() {
                    let _ = tx.send(());
                }
                job.phase = JobPhase::Cancelled;
                Ok(true)
            }
        }
    }

    /// Return the status string of a job, or `None` if not found.
    pub fn status(&self, id: Uuid) -> Option<&'static str> {
        let map = lock(&self.inner);
        map.get(&id).map(|j| j.phase.status_str())
    }

    /// Clone the result of a Done job.
    /// Returns `None` if the job does not exist or is not done.
    pub fn result(&self, id: Uuid) -> Option<JobResult> {
        let map = lock(&self.inner);
        match map.get(&id) {
            Some(Job { phase: JobPhase::Done { result }, .. }) => Some(result.clone()),
            _ => None,
        }
    }

    /// Returns the phase, optional error message, and optional result for a job.
    pub fn phase_and_result(&self, id: Uuid) -> Option<(&'static str, Option<String>, Option<JobResult>)> {
        let map = lock(&self.inner);
        map.get(&id).map(|j| match &j.phase {
            JobPhase::Done { result } => ("done", None, Some(result.clone())),
            JobPhase::Failed { message } => ("failed", Some(message.clone()), None),
            phase => (phase.status_str(), None, None),
        })
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_store() -> (JobStore, Uuid, tokio::sync::oneshot::Receiver<()>) {
        let store = JobStore::new();
        let id = Uuid::new_v4();
        let (tx, rx) = tokio::sync::oneshot::channel();
        store.insert(id, tx).unwrap();
        (store, id, rx)
    }

    #[test]
    fn initial_phase_is_pending() {
        let (store, id, _rx) = make_store();
        assert_eq!(store.status(id), Some("pending"));
    }

    #[test]
    fn set_running_transitions_from_pending() {
        let (store, id, _rx) = make_store();
        store.set_running(id);
        assert_eq!(store.status(id), Some("running"));
    }

    #[test]
    fn finish_ok_transitions_to_done() {
        let (store, id, _rx) = make_store();
        store.set_running(id);
        store.finish(id, Ok(JobResult { segments: vec![], post_output: None }));
        assert_eq!(store.status(id), Some("done"));
        assert!(store.result(id).is_some());
    }

    #[test]
    fn finish_err_transitions_to_failed() {
        let (store, id, _rx) = make_store();
        store.set_running(id);
        store.finish(id, Err("subprocess died".into()));
        assert_eq!(store.status(id), Some("failed"));
    }

    #[test]
    fn cancel_pending_job() {
        let (store, id, _rx) = make_store();
        let was_cancellable = store.cancel(id).unwrap();
        assert!(was_cancellable);
        assert_eq!(store.status(id), Some("cancelled"));
    }

    #[test]
    fn cancel_running_job_sends_signal() {
        let (store, id, mut rx) = make_store();
        store.set_running(id);
        let was_cancellable = store.cancel(id).unwrap();
        assert!(was_cancellable);
        assert_eq!(store.status(id), Some("cancelled"));
        // The receiver should have gotten the signal.
        assert!(rx.try_recv().is_ok());
    }

    #[test]
    fn cancel_done_job_returns_false() {
        let (store, id, _rx) = make_store();
        store.set_running(id);
        store.finish(id, Ok(JobResult { segments: vec![], post_output: None }));
        let was_cancellable = store.cancel(id).unwrap();
        assert!(!was_cancellable);
        // Phase must NOT have been overwritten.
        assert_eq!(store.status(id), Some("done"));
    }

    #[test]
    fn cancel_unknown_id_returns_err() {
        let store = JobStore::new();
        assert!(store.cancel(Uuid::new_v4()).is_err());
    }

    #[test]
    fn status_unknown_id_returns_none() {
        let store = JobStore::new();
        assert!(store.status(Uuid::new_v4()).is_none());
    }

    #[test]
    fn store_rejects_when_full() {
        let store = JobStore::new();
        for _ in 0..MAX_STORED_JOBS {
            let (tx, _rx) = tokio::sync::oneshot::channel();
            store.insert(Uuid::new_v4(), tx).unwrap();
        }
        let (tx, _rx) = tokio::sync::oneshot::channel();
        assert!(store.insert(Uuid::new_v4(), tx).is_err());
    }
}
