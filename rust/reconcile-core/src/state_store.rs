use crate::errors::KernelError;
use crate::types::{Resource, ResourceId};
use std::collections::{HashMap, HashSet};

pub trait StateStore: Send + Sync {
    fn create(&mut self, resource: Resource) -> Result<(), KernelError>;
    fn get(&self, id: &ResourceId) -> Option<Resource>;
    fn update(&mut self, resource: &Resource) -> Result<(), KernelError>;
    fn list_by_type(&self, resource_type: &str) -> Vec<Resource>;
    fn list_by_state(&self, resource_type: &str, state: &str) -> Vec<Resource>;
}

pub struct InMemoryStateStore {
    resources: HashMap<ResourceId, Resource>,
    by_type: HashMap<String, HashSet<ResourceId>>,
}

impl InMemoryStateStore {
    pub fn new() -> Self {
        Self {
            resources: HashMap::new(),
            by_type: HashMap::new(),
        }
    }
}

impl Default for InMemoryStateStore {
    fn default() -> Self {
        Self::new()
    }
}

impl StateStore for InMemoryStateStore {
    fn create(&mut self, resource: Resource) -> Result<(), KernelError> {
        let id = resource.id.clone();
        let rtype = resource.resource_type.clone();
        self.resources.insert(id.clone(), resource);
        self.by_type.entry(rtype).or_default().insert(id);
        Ok(())
    }

    fn get(&self, id: &ResourceId) -> Option<Resource> {
        self.resources.get(id).cloned()
    }

    fn update(&mut self, resource: &Resource) -> Result<(), KernelError> {
        if self.resources.contains_key(&resource.id) {
            self.resources.insert(resource.id.clone(), resource.clone());
            Ok(())
        } else {
            Err(KernelError::ResourceNotFound(resource.id.clone()))
        }
    }

    fn list_by_type(&self, resource_type: &str) -> Vec<Resource> {
        self.by_type
            .get(resource_type)
            .map(|ids| {
                ids.iter()
                    .filter_map(|id| self.resources.get(id).cloned())
                    .collect()
            })
            .unwrap_or_default()
    }

    fn list_by_state(&self, resource_type: &str, state: &str) -> Vec<Resource> {
        self.list_by_type(resource_type)
            .into_iter()
            .filter(|r| r.state == state)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ResourceId;
    use chrono::Utc;

    fn make_resource(rtype: &str, state: &str) -> Resource {
        Resource {
            id: ResourceId::new(),
            resource_type: rtype.into(),
            state: state.into(),
            desired_state: None,
            data: serde_json::json!({}),
            version: 1,
            tenant_id: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn test_create_and_get() {
        let mut store = InMemoryStateStore::new();
        let r = make_resource("loan", "APPLIED");
        let id = r.id.clone();
        store.create(r).unwrap();
        assert!(store.get(&id).is_some());
        assert_eq!(store.get(&id).unwrap().state, "APPLIED");
    }

    #[test]
    fn test_get_nonexistent() {
        let store = InMemoryStateStore::new();
        assert!(store.get(&ResourceId::new()).is_none());
    }

    #[test]
    fn test_list_by_type() {
        let mut store = InMemoryStateStore::new();
        store.create(make_resource("loan", "APPLIED")).unwrap();
        store.create(make_resource("loan", "APPROVED")).unwrap();
        store.create(make_resource("application", "NEW")).unwrap();

        assert_eq!(store.list_by_type("loan").len(), 2);
        assert_eq!(store.list_by_type("application").len(), 1);
        assert_eq!(store.list_by_type("bogus").len(), 0);
    }

    #[test]
    fn test_list_by_state() {
        let mut store = InMemoryStateStore::new();
        store.create(make_resource("loan", "APPLIED")).unwrap();
        store.create(make_resource("loan", "APPLIED")).unwrap();
        store.create(make_resource("loan", "APPROVED")).unwrap();

        assert_eq!(store.list_by_state("loan", "APPLIED").len(), 2);
        assert_eq!(store.list_by_state("loan", "APPROVED").len(), 1);
    }

    #[test]
    fn test_update() {
        let mut store = InMemoryStateStore::new();
        let r = make_resource("loan", "APPLIED");
        let id = r.id.clone();
        store.create(r).unwrap();

        let mut resource = store.get(&id).unwrap();
        resource.state = "UNDERWRITING".into();
        resource.version += 1;
        store.update(&resource).unwrap();

        assert_eq!(store.get(&id).unwrap().state, "UNDERWRITING");
        assert_eq!(store.get(&id).unwrap().version, 2);
    }

    #[test]
    fn test_update_nonexistent_fails() {
        let mut store = InMemoryStateStore::new();
        let r = make_resource("loan", "APPLIED");
        let result = store.update(&r);
        assert!(result.is_err());
    }
}
