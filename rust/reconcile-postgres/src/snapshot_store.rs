use postgres::Client;
use reconcile_core::snapshot_store::SnapshotStore;
use reconcile_core::types::{ResourceId, Snapshot};
use std::cell::RefCell;
use uuid::Uuid;

pub struct PostgresSnapshotStore {
    client: RefCell<Client>,
}

impl PostgresSnapshotStore {
    pub fn new(client: Client) -> Self {
        Self {
            client: RefCell::new(client),
        }
    }

    fn row_to_snapshot(row: &postgres::Row) -> Snapshot {
        let id: Uuid = row.get("id");
        let resource_id: Uuid = row.get("resource_id");
        let version: i32 = row.get("version");
        let event_offset: i64 = row.get("event_offset");
        Snapshot {
            id,
            resource_id: ResourceId(resource_id),
            state: row.get("state"),
            data: row.get("data"),
            version: version as u64,
            event_offset: event_offset as u64,
            timestamp: row.get("created_ts"),
        }
    }
}

impl SnapshotStore for PostgresSnapshotStore {
    fn create(&mut self, snapshot: Snapshot) {
        let _ = self.client.borrow_mut().execute(
            "INSERT INTO snapshots (id, resource_id, state, data, version, event_offset, created_ts)
             VALUES ($1, $2, $3, $4, $5, $6, $7)",
            &[
                &snapshot.id,
                &snapshot.resource_id.0,
                &snapshot.state,
                &snapshot.data,
                &(snapshot.version as i32),
                &(snapshot.event_offset as i64),
                &snapshot.timestamp,
            ],
        );
    }

    fn get_latest(&self, resource_id: &ResourceId) -> Option<Snapshot> {
        self.client.borrow_mut().query_opt(
            "SELECT id, resource_id, state, data, version, event_offset, created_ts
             FROM snapshots WHERE resource_id = $1
             ORDER BY event_offset DESC LIMIT 1",
            &[&resource_id.0],
        ).ok().flatten().map(|row| Self::row_to_snapshot(&row))
    }
}

unsafe impl Send for PostgresSnapshotStore {}
unsafe impl Sync for PostgresSnapshotStore {}
