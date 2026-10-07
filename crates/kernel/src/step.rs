use monstertruck_io::step::save;

use crate::body::Body;
use crate::{KernelError, Result, guard};

/// STEP (ISO 10303-21, AP203-style geometry) text for the given bodies.
pub fn step_export(bodies: &[&Body], system: &str) -> Result<String> {
    if bodies.is_empty() {
        return Err(KernelError::Invalid("nothing to export".into()));
    }
    guard("step export", || {
        let cs: Vec<_> = bodies.iter().map(|b| b.solid.compress()).collect();
        let models = save::StepModels::from_iter(cs.iter());
        let header = save::StepHeaderDescriptor { organization_system: system.to_owned(), ..Default::default() };
        let out = save::CompleteStepDisplay::new(models, header).to_string();
        Ok(out)
    })
}
