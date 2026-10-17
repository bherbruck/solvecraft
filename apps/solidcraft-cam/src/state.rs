//! Shared application state threaded through axum handlers.

use std::sync::Arc;

use crate::{config::CamConfig, jobs::JobStore};

/// Cloneable state injected into every handler via axum's `State` extractor.
#[derive(Clone)]
pub struct AppState {
    pub config: Arc<CamConfig>,
    pub jobs: Arc<JobStore>,
}

impl AppState {
    pub fn new(config: CamConfig) -> Self {
        Self { config: Arc::new(config), jobs: Arc::new(JobStore::new()) }
    }
}
