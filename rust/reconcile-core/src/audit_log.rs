use crate::types::{AuditRecord, ResourceId};
use std::collections::HashMap;

pub trait AuditStore: Send + Sync {
    fn write(&mut self, record: AuditRecord);
    fn get_by_resource(&self, resource_id: &ResourceId) -> Vec<AuditRecord>;
    fn get_all(&self) -> Vec<AuditRecord>;
}

pub struct InMemoryAuditStore {
    records: Vec<AuditRecord>,
    resource_index: HashMap<ResourceId, Vec<usize>>,
}

impl InMemoryAuditStore {
    pub fn new() -> Self {
        Self {
            records: Vec::new(),
            resource_index: HashMap::new(),
        }
    }
}

impl Default for InMemoryAuditStore {
    fn default() -> Self {
        Self::new()
    }
}

impl AuditStore for InMemoryAuditStore {
    fn write(&mut self, record: AuditRecord) {
        let idx = self.records.len();
        self.resource_index
            .entry(record.resource_id.clone())
            .or_default()
            .push(idx);
        self.records.push(record);
    }

    fn get_by_resource(&self, resource_id: &ResourceId) -> Vec<AuditRecord> {
        self.resource_index
            .get(resource_id)
            .map(|indices| indices.iter().map(|&i| self.records[i].clone()).collect())
            .unwrap_or_default()
    }

    fn get_all(&self) -> Vec<AuditRecord> {
        self.records.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{AuthorityLevel, ResourceId};
    use chrono::Utc;
    use uuid::Uuid;

    fn make_audit(resource_id: &ResourceId) -> AuditRecord {
        AuditRecord {
            id: Uuid::new_v4(),
            resource_type: "loan".into(),
            resource_id: resource_id.clone(),
            actor: "user1".into(),
            role: "officer".into(),
            authority_level: AuthorityLevel::Human,
            previous_state: "APPLIED".into(),
            new_state: "UNDERWRITING".into(),
            policies_evaluated: vec![],
            invariants_checked: vec![],
            timestamp: Utc::now(),
        }
    }

    #[test]
    fn test_write_and_query() {
        let mut store = InMemoryAuditStore::new();
        let rid = ResourceId::new();
        store.write(make_audit(&rid));
        store.write(make_audit(&rid));

        assert_eq!(store.get_by_resource(&rid).len(), 2);
        assert_eq!(store.get_all().len(), 2);
    }

    #[test]
    fn test_separate_resources() {
        let mut store = InMemoryAuditStore::new();
        let rid1 = ResourceId::new();
        let rid2 = ResourceId::new();
        store.write(make_audit(&rid1));
        store.write(make_audit(&rid2));

        assert_eq!(store.get_by_resource(&rid1).len(), 1);
        assert_eq!(store.get_by_resource(&rid2).len(), 1);
    }
}
