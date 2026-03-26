use crate::types::{ResourceId, Snapshot};
use std::collections::HashMap;

pub trait SnapshotStore: Send + Sync {
    fn create(&mut self, snapshot: Snapshot);
    fn get_latest(&self, resource_id: &ResourceId) -> Option<Snapshot>;
}

pub struct InMemorySnapshotStore {
    snapshots: Vec<Snapshot>,
    /// resource_id -> indices into snapshots vec, ordered by event_offset
    resource_index: HashMap<ResourceId, Vec<usize>>,
}

impl InMemorySnapshotStore {
    pub fn new() -> Self {
        Self {
            snapshots: Vec::new(),
            resource_index: HashMap::new(),
        }
    }
}

impl Default for InMemorySnapshotStore {
    fn default() -> Self {
        Self::new()
    }
}

impl SnapshotStore for InMemorySnapshotStore {
    fn create(&mut self, snapshot: Snapshot) {
        let idx = self.snapshots.len();
        self.resource_index
            .entry(snapshot.resource_id.clone())
            .or_default()
            .push(idx);
        self.snapshots.push(snapshot);
    }

    fn get_latest(&self, resource_id: &ResourceId) -> Option<Snapshot> {
        self.resource_index
            .get(resource_id)
            .and_then(|indices| indices.last())
            .map(|&i| self.snapshots[i].clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ResourceId;
    use chrono::Utc;
    use uuid::Uuid;

    fn make_snapshot(resource_id: &ResourceId, offset: u64) -> Snapshot {
        Snapshot {
            id: Uuid::new_v4(),
            resource_id: resource_id.clone(),
            state: "APPLIED".into(),
            data: serde_json::json!({}),
            version: 1,
            event_offset: offset,
            timestamp: Utc::now(),
        }
    }

    #[test]
    fn test_create_and_get_latest() {
        let mut store = InMemorySnapshotStore::new();
        let rid = ResourceId::new();
        store.create(make_snapshot(&rid, 0));
        store.create(make_snapshot(&rid, 5));

        let latest = store.get_latest(&rid).unwrap();
        assert_eq!(latest.event_offset, 5);
    }

    #[test]
    fn test_no_snapshots() {
        let store = InMemorySnapshotStore::new();
        assert!(store.get_latest(&ResourceId::new()).is_none());
    }
}
