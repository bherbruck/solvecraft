use truck_stepio::out;

use crate::body::Body;
use crate::{KernelError, Result, guard};

/// STEP (ISO 10303-21) text for the given bodies.
pub fn step_export(bodies: &[&Body], system: &str) -> Result<String> {
    if bodies.is_empty() {
        return Err(KernelError::Invalid("nothing to export".into()));
    }
    guard("step export", || {
        let cs: Vec<_> = bodies.iter().map(|b| b.solid.compress()).collect();
        let models: out::StepModels<_, _, _> = cs.iter().collect();
        let header = out::StepHeaderDescriptor { organization_system: system.to_owned(), ..Default::default() };
        Ok(out::CompleteStepDisplay::new(models, header).to_string())
    })
}
