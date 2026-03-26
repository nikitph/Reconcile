use crate::errors::KernelError;
use crate::state_machine::StateMachine;
use std::collections::HashMap;

pub struct ResourceTypeDefinition {
    pub name: String,
    pub schema: serde_json::Value,
    pub state_machine: StateMachine,
}

pub struct ResourceRegistry {
    types: HashMap<String, ResourceTypeDefinition>,
}

impl ResourceRegistry {
    pub fn new() -> Self {
        Self {
            types: HashMap::new(),
        }
    }

    pub fn register(&mut self, def: ResourceTypeDefinition) -> Result<(), KernelError> {
        if self.types.contains_key(&def.name) {
            return Err(KernelError::TypeAlreadyRegistered(def.name));
        }
        self.types.insert(def.name.clone(), def);
        Ok(())
    }

    pub fn get(&self, type_name: &str) -> Option<&ResourceTypeDefinition> {
        self.types.get(type_name)
    }

    pub fn get_state_machine(&self, type_name: &str) -> Result<&StateMachine, KernelError> {
        self.types
            .get(type_name)
            .map(|def| &def.state_machine)
            .ok_or_else(|| KernelError::TypeNotRegistered(type_name.to_string()))
    }

    pub fn list_types(&self) -> Vec<&str> {
        self.types.keys().map(|s| s.as_str()).collect()
    }
}

impl Default for ResourceRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state_machine::{StateDefinition, StateStatus, TransitionDefinition};

    fn make_def(name: &str) -> ResourceTypeDefinition {
        let states = vec![
            StateDefinition { name: "A".into(), status: StateStatus::Active },
            StateDefinition { name: "B".into(), status: StateStatus::Terminal },
        ];
        let transitions = vec![
            TransitionDefinition { from_state: "A".into(), to_state: "B".into(), guard: None, required_role: None },
        ];
        ResourceTypeDefinition {
            name: name.into(),
            schema: serde_json::json!({}),
            state_machine: StateMachine::new(states, transitions, "A".into()).unwrap(),
        }
    }

    #[test]
    fn test_register_and_get() {
        let mut registry = ResourceRegistry::new();
        registry.register(make_def("loan")).unwrap();
        assert!(registry.get("loan").is_some());
        assert!(registry.get("bogus").is_none());
    }

    #[test]
    fn test_duplicate_registration() {
        let mut registry = ResourceRegistry::new();
        registry.register(make_def("loan")).unwrap();
        let result = registry.register(make_def("loan"));
        assert!(matches!(result, Err(KernelError::TypeAlreadyRegistered(_))));
    }

    #[test]
    fn test_list_types() {
        let mut registry = ResourceRegistry::new();
        registry.register(make_def("loan")).unwrap();
        registry.register(make_def("application")).unwrap();
        let types = registry.list_types();
        assert_eq!(types.len(), 2);
    }
}
