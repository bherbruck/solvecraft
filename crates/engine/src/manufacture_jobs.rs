//! Per-session, transient CAM job registry.
//!
//! Mirrors the structure of [`crate::render_jobs`]: captured STEP snapshots remain immutable
//! when the CAD model continues to change; callers receive the captured `scene_generation` and
//! must never present it as a more recent result.
//!
//! The registry never touches the document, the undo stack, or any evaluated geometry.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};

use solidcraft_manufacture::job::{
    CamBackendDescriptor, CamErrorCode, CamHandle, CamHost, CamJobError, CamJobId, CamJobResult, CamJobStatus, CamPhase, CamResult,
};

static NEXT_OWNER: AtomicU64 = AtomicU64::new(1);

fn next_owner() -> u64 {
    let mut n = NEXT_OWNER.load(Ordering::Relaxed);
    loop {
        let Some(next) = n.checked_add(1) else { return 0 };
        match NEXT_OWNER.compare_exchange_weak(n, next, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => return n,
            Err(current) => n = current,
        }
    }
}

pub(crate) struct Jobs {
    owner: u64,
    serial: u64,
    generation: u64,
    pub(crate) host: Option<CamHost>,
    live: Vec<CamHandle>,
    history: VecDeque<CamJobStatus>,
    last_good: Option<CamHandle>,
}

impl Default for Jobs {
    fn default() -> Self {
        Self { owner: next_owner(), serial: 0, generation: 0, host: None, live: Vec::new(), history: VecDeque::new(), last_good: None }
    }
}

impl Drop for Jobs {
    fn drop(&mut self) {
        for job in &self.live {
            let _ = job.cancel();
        }
    }
}

/// Maximum concurrent live jobs.
const QUEUE_LIMIT: usize = 2;
/// History entries retained.
const HISTORY_LIMIT: usize = 4;

impl Jobs {
    /// A scratch copy that carries the host but resets all identity counters.
    pub(crate) fn scratch(&self) -> Self {
        let mut j = Self::default();
        j.host = self.host.clone();
        j
    }

    /// Collect terminal live jobs into history; keep at most `HISTORY_LIMIT` entries.
    pub(crate) fn collect(&mut self) -> CamResult<()> {
        let mut i = 0;
        while i < self.live.len() {
            let status = self.live.get(i).ok_or_else(|| CamJobError::new(CamErrorCode::Internal, "missing live cam job"))?.status()?;
            if !status.progress.phase.terminal() {
                i += 1;
                continue;
            }
            let handle = self.live.remove(i);
            let completed = status.progress.phase == CamPhase::Completed;
            let serial = handle.id().serial;
            if completed && self.last_good.as_ref().is_none_or(|old| old.id().serial < serial) {
                self.last_good = Some(handle);
            }
            self.history.push_back(status);
            while self.history.len() > HISTORY_LIMIT {
                self.history.pop_front();
            }
        }
        Ok(())
    }

    pub(crate) fn submit(&mut self, input: solidcraft_manufacture::job::CamJobInput, replace: bool) -> CamResult<CamJobStatus> {
        self.collect()?;
        let host = self.host.as_ref().ok_or_else(|| CamJobError::new(CamErrorCode::HostUnavailable, "no CAM host is installed"))?.clone();
        if !host.is_available() {
            return Err(CamJobError::new(CamErrorCode::HostUnavailable, "CAM host is unavailable"));
        }
        if replace {
            for job in self.live.drain(..) {
                let mut status = job.cancel()?;
                if !status.progress.phase.terminal() {
                    status.progress.phase = CamPhase::Cancelled;
                }
                self.history.push_back(status);
            }
            while self.history.len() > HISTORY_LIMIT {
                self.history.pop_front();
            }
        }
        if self.live.len() >= QUEUE_LIMIT {
            return Err(CamJobError::new(CamErrorCode::QueueFull, "this document already has an active and pending CAM job"));
        }
        if self.owner == 0 {
            return Err(CamJobError::new(CamErrorCode::LimitExceeded, "CAM session identity exhausted"));
        }
        let serial = self.serial.checked_add(1).ok_or_else(|| CamJobError::new(CamErrorCode::LimitExceeded, "CAM job serial exhausted"))?;
        let generation = self.generation.checked_add(1).ok_or_else(|| CamJobError::new(CamErrorCode::LimitExceeded, "CAM generation exhausted"))?;
        let id = CamJobId { session: self.owner, serial, generation };
        let handle = host.submit(id, input)?;
        let status = handle.status()?;
        self.serial = serial;
        self.generation = generation;
        self.live.push(handle);
        Ok(status)
    }

    fn check_owner(&self, id: CamJobId) -> CamResult<()> {
        if id.session != self.owner {
            return Err(CamJobError::new(CamErrorCode::WrongOwner, "CAM job belongs to another document session"));
        }
        Ok(())
    }

    pub(crate) fn handle(&self, id: CamJobId) -> CamResult<CamHandle> {
        self.check_owner(id)?;
        self.live
            .iter()
            .chain(self.last_good.iter())
            .find(|j| j.id() == id)
            .cloned()
            .ok_or_else(|| CamJobError::new(CamErrorCode::UnknownJob, "CAM job is no longer retained"))
    }

    pub(crate) fn status(&mut self, id: CamJobId) -> CamResult<CamJobStatus> {
        self.check_owner(id)?;
        self.collect()?;
        if let Ok(handle) = self.handle(id) {
            return handle.status();
        }
        self.history.iter().find(|s| s.job == id).cloned().ok_or_else(|| CamJobError::new(CamErrorCode::UnknownJob, "CAM job is no longer retained"))
    }

    pub(crate) fn result(&mut self, id: CamJobId) -> CamResult<std::sync::Arc<CamJobResult>> {
        self.collect()?;
        self.handle(id)?.result()?.ok_or_else(|| CamJobError::new(CamErrorCode::NotCompleted, "CAM job has not completed"))
    }

    pub(crate) fn descriptor(&self) -> Option<CamBackendDescriptor> {
        self.host.as_ref().map(CamHost::descriptor)
    }
}

impl crate::Session {
    /// Install or remove the CAM host.  Called by the application; never by a command.
    pub fn set_manufacture_job_host(&mut self, host: Option<CamHost>) {
        let mut jobs = Jobs::default();
        jobs.host = host;
        self.manufacture_jobs = jobs;
    }

    pub fn manufacture_job_host(&self) -> Option<&CamHost> {
        self.manufacture_jobs.host.as_ref()
    }
}

pub(crate) fn job_error(error: solidcraft_manufacture::job::CamJobError) -> crate::EngineError {
    crate::EngineError::Other(error.to_string())
}
