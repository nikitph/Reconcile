use pyo3::prelude::*;

mod py_types;
mod py_system;

use py_types::*;
use py_system::PyReconcileSystem;

#[pymodule]
fn _native(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add("__version__", "0.1.0")?;
    m.add_class::<PyReconcileSystem>()?;
    m.add_class::<PyResource>()?;
    m.add_class::<PyEvent>()?;
    m.add_class::<PyAuditRecord>()?;
    m.add_class::<PyTransitionResult>()?;
    m.add_class::<PyPolicyResult>()?;
    m.add_class::<PyInvariantResult>()?;
    m.add_class::<PyAuthorityLevel>()?;
    m.add_class::<PyQueryContext>()?;
    Ok(())
}
