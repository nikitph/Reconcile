use pyo3::prelude::*;
use reconcile_core::types;

// ---------------------------------------------------------------------------
// AuthorityLevel enum
// ---------------------------------------------------------------------------

#[pyclass(name = "AuthorityLevel", eq)]
#[derive(Clone, PartialEq)]
pub struct PyAuthorityLevel {
    pub inner: types::AuthorityLevel,
}

#[pymethods]
impl PyAuthorityLevel {
    #[classattr]
    const HUMAN: &'static str = "HUMAN";
    #[classattr]
    const CONTROLLER: &'static str = "CONTROLLER";
    #[classattr]
    const AGENT: &'static str = "AGENT";
    #[classattr]
    const SYSTEM: &'static str = "SYSTEM";

    fn __repr__(&self) -> String {
        format!("AuthorityLevel.{}", self.inner)
    }

    fn __str__(&self) -> String {
        self.inner.to_string()
    }
}

// ---------------------------------------------------------------------------
// Resource
// ---------------------------------------------------------------------------

#[pyclass(name = "Resource")]
#[derive(Clone)]
pub struct PyResource {
    pub inner: types::Resource,
}

#[pymethods]
impl PyResource {
    #[getter]
    fn id(&self) -> String {
        self.inner.id.to_string()
    }

    #[getter]
    fn resource_type(&self) -> &str {
        &self.inner.resource_type
    }

    #[getter]
    fn state(&self) -> &str {
        &self.inner.state
    }

    #[getter]
    fn desired_state(&self) -> Option<&str> {
        self.inner.desired_state.as_deref()
    }

    #[getter]
    fn data(&self, py: Python<'_>) -> PyResult<PyObject> {
        json_to_py(py, &self.inner.data)
    }

    #[getter]
    fn version(&self) -> u64 {
        self.inner.version
    }

    fn __repr__(&self) -> String {
        format!(
            "Resource(id={}, type={}, state={}, version={})",
            self.inner.id, self.inner.resource_type, self.inner.state, self.inner.version
        )
    }
}

// ---------------------------------------------------------------------------
// Event
// ---------------------------------------------------------------------------

#[pyclass(name = "Event")]
#[derive(Clone)]
pub struct PyEvent {
    pub inner: types::Event,
}

#[pymethods]
impl PyEvent {
    #[getter]
    fn id(&self) -> String {
        self.inner.id.to_string()
    }

    #[getter]
    fn offset(&self) -> u64 {
        self.inner.offset
    }

    #[getter]
    fn event_type(&self) -> &str {
        &self.inner.event_type
    }

    #[getter]
    fn resource_id(&self) -> String {
        self.inner.resource_id.to_string()
    }

    #[getter]
    fn payload(&self, py: Python<'_>) -> PyResult<PyObject> {
        json_to_py(py, &self.inner.payload)
    }

    #[getter]
    fn actor(&self) -> &str {
        &self.inner.actor
    }

    #[getter]
    fn authority_level(&self) -> String {
        self.inner.authority_level.to_string()
    }

    fn __repr__(&self) -> String {
        format!(
            "Event(type={}, resource_id={}, offset={})",
            self.inner.event_type, self.inner.resource_id, self.inner.offset
        )
    }
}

// ---------------------------------------------------------------------------
// AuditRecord
// ---------------------------------------------------------------------------

#[pyclass(name = "AuditRecord")]
#[derive(Clone)]
pub struct PyAuditRecord {
    pub inner: types::AuditRecord,
}

#[pymethods]
impl PyAuditRecord {
    #[getter]
    fn id(&self) -> String {
        self.inner.id.to_string()
    }

    #[getter]
    fn resource_type(&self) -> &str {
        &self.inner.resource_type
    }

    #[getter]
    fn resource_id(&self) -> String {
        self.inner.resource_id.to_string()
    }

    #[getter]
    fn actor(&self) -> &str {
        &self.inner.actor
    }

    #[getter]
    fn role(&self) -> &str {
        &self.inner.role
    }

    #[getter]
    fn authority_level(&self) -> String {
        self.inner.authority_level.to_string()
    }

    #[getter]
    fn previous_state(&self) -> &str {
        &self.inner.previous_state
    }

    #[getter]
    fn new_state(&self) -> &str {
        &self.inner.new_state
    }

    #[getter]
    fn policies_evaluated(&self) -> Vec<(String, bool, String)> {
        self.inner
            .policies_evaluated
            .iter()
            .map(|p| (p.name.clone(), p.passed, p.message.clone()))
            .collect()
    }

    #[getter]
    fn invariants_checked(&self) -> Vec<(String, bool, String)> {
        self.inner
            .invariants_checked
            .iter()
            .map(|i| {
                (
                    i.name.clone(),
                    i.holds,
                    i.violation.clone().unwrap_or_default(),
                )
            })
            .collect()
    }

    fn __repr__(&self) -> String {
        format!(
            "AuditRecord(resource={}, {} -> {}, actor={})",
            self.inner.resource_id,
            self.inner.previous_state,
            self.inner.new_state,
            self.inner.actor,
        )
    }
}

// ---------------------------------------------------------------------------
// TransitionResult
// ---------------------------------------------------------------------------

#[pyclass(name = "TransitionResult")]
#[derive(Clone)]
pub struct PyTransitionResult {
    pub success: bool,
    pub resource: Option<PyResource>,
    pub events: Vec<PyEvent>,
    pub rejected_step: Option<String>,
    pub rejected_reason: Option<String>,
}

#[pymethods]
impl PyTransitionResult {
    #[getter]
    fn success(&self) -> bool {
        self.success
    }

    #[getter]
    fn resource(&self) -> Option<PyResource> {
        self.resource.clone()
    }

    #[getter]
    fn events(&self) -> Vec<PyEvent> {
        self.events.clone()
    }

    #[getter]
    fn rejected_step(&self) -> Option<String> {
        self.rejected_step.clone()
    }

    #[getter]
    fn rejected_reason(&self) -> Option<String> {
        self.rejected_reason.clone()
    }

    fn __repr__(&self) -> String {
        if self.success {
            "TransitionResult(success=True)".into()
        } else {
            format!(
                "TransitionResult(success=False, step={}, reason={})",
                self.rejected_step.as_deref().unwrap_or("?"),
                self.rejected_reason.as_deref().unwrap_or("?"),
            )
        }
    }
}

impl PyTransitionResult {
    pub fn from_outcome(outcome: types::TransitionOutcome) -> Self {
        match outcome {
            types::TransitionOutcome::Success { resource, events } => Self {
                success: true,
                resource: Some(PyResource { inner: resource }),
                events: events.into_iter().map(|e| PyEvent { inner: e }).collect(),
                rejected_step: None,
                rejected_reason: None,
            },
            types::TransitionOutcome::Rejected { step, reason, .. } => Self {
                success: false,
                resource: None,
                events: vec![],
                rejected_step: Some(step),
                rejected_reason: Some(reason),
            },
        }
    }
}

// ---------------------------------------------------------------------------
// PolicyResult + InvariantResult
// ---------------------------------------------------------------------------

#[pyclass(name = "PolicyResult")]
#[derive(Clone)]
pub struct PyPolicyResult {
    pub inner: types::PolicyResult,
}

#[pymethods]
impl PyPolicyResult {
    #[new]
    #[pyo3(signature = (passed, message="".to_string()))]
    fn new(passed: bool, message: String) -> Self {
        Self {
            inner: types::PolicyResult {
                passed,
                message,
                details: serde_json::Value::Null,
            },
        }
    }

    #[getter]
    fn passed(&self) -> bool {
        self.inner.passed
    }

    #[getter]
    fn message(&self) -> &str {
        &self.inner.message
    }

    #[staticmethod]
    fn allow() -> Self {
        Self {
            inner: types::PolicyResult::allow(),
        }
    }

    #[staticmethod]
    fn deny(message: String) -> Self {
        Self {
            inner: types::PolicyResult::deny(message),
        }
    }
}

#[pyclass(name = "InvariantResult")]
#[derive(Clone)]
pub struct PyInvariantResult {
    pub inner: types::InvariantResult,
}

#[pymethods]
impl PyInvariantResult {
    #[new]
    #[pyo3(signature = (holds, violation=None))]
    fn new(holds: bool, violation: Option<String>) -> Self {
        Self {
            inner: types::InvariantResult {
                holds,
                violation,
                details: serde_json::Value::Null,
            },
        }
    }

    #[getter]
    fn holds(&self) -> bool {
        self.inner.holds
    }

    #[getter]
    fn violation(&self) -> Option<String> {
        self.inner.violation.clone()
    }

    #[staticmethod]
    fn ok() -> Self {
        Self {
            inner: types::InvariantResult::ok(),
        }
    }

    #[staticmethod]
    fn violated(message: String) -> Self {
        Self {
            inner: types::InvariantResult::violated(message),
        }
    }
}

// ---------------------------------------------------------------------------
// JSON <-> Python conversion helpers
// ---------------------------------------------------------------------------

pub fn json_to_py(py: Python<'_>, value: &serde_json::Value) -> PyResult<PyObject> {
    match value {
        serde_json::Value::Null => Ok(py.None()),
        serde_json::Value::Bool(b) => Ok(b.into_pyobject(py)?.to_owned().into_any().unbind()),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Ok(i.into_pyobject(py)?.into_any().unbind())
            } else if let Some(f) = n.as_f64() {
                Ok(f.into_pyobject(py)?.into_any().unbind())
            } else {
                Ok(py.None())
            }
        }
        serde_json::Value::String(s) => Ok(s.into_pyobject(py)?.into_any().unbind()),
        serde_json::Value::Array(arr) => {
            let list = pyo3::types::PyList::empty(py);
            for item in arr {
                list.append(json_to_py(py, item)?)?;
            }
            Ok(list.into_any().unbind())
        }
        serde_json::Value::Object(map) => {
            let dict = pyo3::types::PyDict::new(py);
            for (k, v) in map {
                dict.set_item(k, json_to_py(py, v)?)?;
            }
            Ok(dict.into_any().unbind())
        }
    }
}

pub fn py_to_json(py: Python<'_>, obj: &Bound<'_, pyo3::PyAny>) -> PyResult<serde_json::Value> {
    if obj.is_none() {
        Ok(serde_json::Value::Null)
    } else if let Ok(b) = obj.extract::<bool>() {
        Ok(serde_json::Value::Bool(b))
    } else if let Ok(i) = obj.extract::<i64>() {
        Ok(serde_json::json!(i))
    } else if let Ok(f) = obj.extract::<f64>() {
        Ok(serde_json::json!(f))
    } else if let Ok(s) = obj.extract::<String>() {
        Ok(serde_json::Value::String(s))
    } else if let Ok(list) = obj.downcast::<pyo3::types::PyList>() {
        let arr: Result<Vec<serde_json::Value>, _> =
            list.iter().map(|item| py_to_json(py, &item)).collect();
        Ok(serde_json::Value::Array(arr?))
    } else if let Ok(dict) = obj.downcast::<pyo3::types::PyDict>() {
        let mut map = serde_json::Map::new();
        for (k, v) in dict.iter() {
            let key: String = k.extract()?;
            map.insert(key, py_to_json(py, &v)?);
        }
        Ok(serde_json::Value::Object(map))
    } else {
        // Fallback: convert to string
        let s: String = obj.str()?.extract()?;
        Ok(serde_json::Value::String(s))
    }
}
