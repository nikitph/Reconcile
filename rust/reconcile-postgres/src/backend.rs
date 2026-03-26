//! PostgreSQL storage backend — wraps all 4 stores with shared connection
//! and real transaction support (BEGIN/COMMIT/ROLLBACK).

use postgres::Client;
use reconcile_core::audit_log::AuditStore;
use reconcile_core::errors::KernelError;
use reconcile_core::event_log::EventLog;
use reconcile_core::snapshot_store::SnapshotStore;
use reconcile_core::state_store::StateStore;
use reconcile_core::storage::StorageBackend;
use reconcile_core::types::*;
use std::cell::RefCell;
use uuid::Uuid;

/// PostgreSQL backend — all 4 stores share a single connection.
/// Supports real BEGIN/COMMIT/ROLLBACK transactions.
pub struct PostgresBackend {
    client: RefCell<Client>,
    in_transaction: bool,
}

impl PostgresBackend {
    pub fn new(client: Client) -> Self {
        Self {
            client: RefCell::new(client),
            in_transaction: false,
        }
    }

    /// Connect and run migrations.
    pub fn connect(database_url: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let client = crate::connect(database_url)?;
        Ok(Self::new(client))
    }

    fn row_to_resource(row: &postgres::Row) -> Resource {
        let id: Uuid = row.get("id");
        Resource {
            id: ResourceId(id),
            resource_type: row.get("resource_type"),
            state: row.get("state"),
            desired_state: row.get("desired_state"),
            data: row.get("data"),
            version: row.get::<_, i32>("version") as u64,
            created_at: row.get("created_at"),
            updated_at: row.get("updated_at"),
        }
    }

    fn row_to_event(row: &postgres::Row) -> Event {
        let id: Uuid = row.get("id");
        let offset: i64 = row.get("event_offset");
        let resource_id: Uuid = row.get("resource_id");
        let authority_str: String = row.get("authority_level");
        Event {
            id,
            offset: offset as u64,
            event_type: row.get("event_type"),
            resource_id: ResourceId(resource_id),
            payload: row.get("payload"),
            actor: row.get("actor"),
            authority_level: AuthorityLevel::from_str(&authority_str)
                .unwrap_or(AuthorityLevel::System),
            timestamp: row.get("created_ts"),
        }
    }

    fn row_to_audit(row: &postgres::Row) -> AuditRecord {
        let id: Uuid = row.get("id");
        let resource_id: Uuid = row.get("resource_id");
        let authority_str: String = row.get("authority_level");
        let policies_json: serde_json::Value = row.get("policies_evaluated");
        let invariants_json: serde_json::Value = row.get("invariants_checked");

        let policies = policies_json.as_array().map(|arr| {
            arr.iter().filter_map(|v| Some(PolicyEvaluation {
                name: v.get("name")?.as_str()?.to_string(),
                passed: v.get("passed")?.as_bool()?,
                message: v.get("message")?.as_str().unwrap_or("").to_string(),
            })).collect()
        }).unwrap_or_default();

        let invariants = invariants_json.as_array().map(|arr| {
            arr.iter().filter_map(|v| Some(InvariantEvaluation {
                name: v.get("name")?.as_str()?.to_string(),
                holds: v.get("holds")?.as_bool()?,
                violation: v.get("violation").and_then(|v| v.as_str()).map(|s| s.to_string()),
            })).collect()
        }).unwrap_or_default();

        AuditRecord {
            id,
            resource_type: row.get("resource_type"),
            resource_id: ResourceId(resource_id),
            actor: row.get("actor"),
            role: row.get("role"),
            authority_level: AuthorityLevel::from_str(&authority_str).unwrap_or(AuthorityLevel::System),
            previous_state: row.get("previous_state"),
            new_state: row.get("new_state"),
            policies_evaluated: policies,
            invariants_checked: invariants,
            timestamp: row.get("created_ts"),
        }
    }

    fn row_to_snapshot(row: &postgres::Row) -> Snapshot {
        let id: Uuid = row.get("id");
        let resource_id: Uuid = row.get("resource_id");
        Snapshot {
            id,
            resource_id: ResourceId(resource_id),
            state: row.get("state"),
            data: row.get("data"),
            version: row.get::<_, i32>("version") as u64,
            event_offset: row.get::<_, i64>("event_offset") as u64,
            timestamp: row.get("created_ts"),
        }
    }
}

// PostgresBackend implements all 4 storage traits directly on itself.
// StorageBackend::state_store() etc. return &self cast to the trait.
// This way all 4 "stores" share the same connection and transaction state.

impl StateStore for PostgresBackend {
    fn create(&mut self, resource: Resource) -> Result<(), KernelError> {
        self.client.borrow_mut().execute(
            "INSERT INTO resources (id, resource_type, state, desired_state, data, version, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
            &[&resource.id.0, &resource.resource_type, &resource.state,
              &resource.desired_state, &resource.data, &(resource.version as i32),
              &resource.created_at, &resource.updated_at],
        ).map_err(|e| KernelError::CallbackError(format!("PG insert: {}", e)))?;
        Ok(())
    }

    fn get(&self, id: &ResourceId) -> Option<Resource> {
        self.client.borrow_mut().query_opt(
            "SELECT id, resource_type, state, desired_state, data, version, created_at, updated_at
             FROM resources WHERE id = $1", &[&id.0],
        ).ok().flatten().map(|row| Self::row_to_resource(&row))
    }

    fn update(&mut self, resource: &Resource) -> Result<(), KernelError> {
        let rows = self.client.borrow_mut().execute(
            "UPDATE resources SET state=$2, desired_state=$3, data=$4, version=$5, updated_at=$6
             WHERE id=$1 AND version=$5-1",
            &[&resource.id.0, &resource.state, &resource.desired_state,
              &resource.data, &(resource.version as i32), &resource.updated_at],
        ).map_err(|e| KernelError::CallbackError(format!("PG update: {}", e)))?;
        if rows == 0 {
            if self.get(&resource.id).is_some() {
                return Err(KernelError::VersionConflict {
                    resource_id: resource.id.clone(),
                    expected: resource.version - 1, found: resource.version,
                });
            }
            return Err(KernelError::ResourceNotFound(resource.id.clone()));
        }
        Ok(())
    }

    fn list_by_type(&self, resource_type: &str) -> Vec<Resource> {
        self.client.borrow_mut().query(
            "SELECT id, resource_type, state, desired_state, data, version, created_at, updated_at
             FROM resources WHERE resource_type=$1", &[&resource_type],
        ).unwrap_or_default().iter().map(Self::row_to_resource).collect()
    }

    fn list_by_state(&self, resource_type: &str, state: &str) -> Vec<Resource> {
        self.client.borrow_mut().query(
            "SELECT id, resource_type, state, desired_state, data, version, created_at, updated_at
             FROM resources WHERE resource_type=$1 AND state=$2", &[&resource_type, &state],
        ).unwrap_or_default().iter().map(Self::row_to_resource).collect()
    }
}

impl EventLog for PostgresBackend {
    fn append(&mut self, event: Event) -> u64 {
        let row = self.client.borrow_mut().query_one(
            "INSERT INTO events (id, event_type, resource_id, payload, actor, authority_level, created_ts)
             VALUES ($1, $2, $3, $4, $5, $6, $7) RETURNING event_offset",
            &[&event.id, &event.event_type, &event.resource_id.0, &event.payload,
              &event.actor, &event.authority_level.to_string(), &event.timestamp],
        ).expect("Failed to insert event");
        row.get::<_, i64>("event_offset") as u64
    }

    fn get_by_resource(&self, resource_id: &ResourceId) -> Vec<Event> {
        self.client.borrow_mut().query(
            "SELECT id, event_offset, event_type, resource_id, payload, actor, authority_level, created_ts
             FROM events WHERE resource_id=$1 ORDER BY event_offset", &[&resource_id.0],
        ).unwrap_or_default().iter().map(Self::row_to_event).collect()
    }

    fn get_by_resource_since(&self, resource_id: &ResourceId, after_offset: u64) -> Vec<Event> {
        self.client.borrow_mut().query(
            "SELECT id, event_offset, event_type, resource_id, payload, actor, authority_level, created_ts
             FROM events WHERE resource_id=$1 AND event_offset > $2 ORDER BY event_offset",
            &[&resource_id.0, &(after_offset as i64)],
        ).unwrap_or_default().iter().map(Self::row_to_event).collect()
    }

    fn get_all(&self) -> Vec<Event> {
        self.client.borrow_mut().query(
            "SELECT id, event_offset, event_type, resource_id, payload, actor, authority_level, created_ts
             FROM events ORDER BY event_offset", &[],
        ).unwrap_or_default().iter().map(Self::row_to_event).collect()
    }

    fn get_latest_offset(&self) -> u64 {
        self.client.borrow_mut().query_one(
            "SELECT COALESCE(MAX(event_offset), 0) as v FROM events", &[],
        ).map(|r| r.get::<_, i64>("v") as u64).unwrap_or(0)
    }
}

impl AuditStore for PostgresBackend {
    fn write(&mut self, record: AuditRecord) {
        let policies = serde_json::to_value(&record.policies_evaluated).unwrap_or_default();
        let invariants = serde_json::to_value(&record.invariants_checked).unwrap_or_default();
        let _ = self.client.borrow_mut().execute(
            "INSERT INTO audit_log (id, resource_type, resource_id, actor, role, authority_level,
             previous_state, new_state, policies_evaluated, invariants_checked, created_ts)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)",
            &[&record.id, &record.resource_type, &record.resource_id.0,
              &record.actor, &record.role, &record.authority_level.to_string(),
              &record.previous_state, &record.new_state, &policies, &invariants, &record.timestamp],
        );
    }

    fn get_by_resource(&self, resource_id: &ResourceId) -> Vec<AuditRecord> {
        self.client.borrow_mut().query(
            "SELECT id, resource_type, resource_id, actor, role, authority_level,
             previous_state, new_state, policies_evaluated, invariants_checked, created_ts
             FROM audit_log WHERE resource_id=$1 ORDER BY created_ts", &[&resource_id.0],
        ).unwrap_or_default().iter().map(Self::row_to_audit).collect()
    }

    fn get_all(&self) -> Vec<AuditRecord> {
        self.client.borrow_mut().query(
            "SELECT id, resource_type, resource_id, actor, role, authority_level,
             previous_state, new_state, policies_evaluated, invariants_checked, created_ts
             FROM audit_log ORDER BY created_ts", &[],
        ).unwrap_or_default().iter().map(Self::row_to_audit).collect()
    }
}

impl reconcile_core::snapshot_store::SnapshotStore for PostgresBackend {
    fn create(&mut self, snapshot: Snapshot) {
        let _ = self.client.borrow_mut().execute(
            "INSERT INTO snapshots (id, resource_id, state, data, version, event_offset, created_ts)
             VALUES ($1,$2,$3,$4,$5,$6,$7)",
            &[&snapshot.id, &snapshot.resource_id.0, &snapshot.state, &snapshot.data,
              &(snapshot.version as i32), &(snapshot.event_offset as i64), &snapshot.timestamp],
        );
    }

    fn get_latest(&self, resource_id: &ResourceId) -> Option<Snapshot> {
        self.client.borrow_mut().query_opt(
            "SELECT id, resource_id, state, data, version, event_offset, created_ts
             FROM snapshots WHERE resource_id=$1 ORDER BY event_offset DESC LIMIT 1",
            &[&resource_id.0],
        ).ok().flatten().map(|row| Self::row_to_snapshot(&row))
    }
}

impl StorageBackend for PostgresBackend {
    fn state_store(&self) -> &dyn StateStore { self }
    fn state_store_mut(&mut self) -> &mut dyn StateStore { self }
    fn event_log(&self) -> &dyn EventLog { self }
    fn event_log_mut(&mut self) -> &mut dyn EventLog { self }
    fn audit_store(&self) -> &dyn AuditStore { self }
    fn audit_store_mut(&mut self) -> &mut dyn AuditStore { self }
    fn snapshot_store(&self) -> &dyn SnapshotStore { self }
    fn snapshot_store_mut(&mut self) -> &mut dyn SnapshotStore { self }

    fn begin(&mut self) -> Result<(), KernelError> {
        if !self.in_transaction {
            self.client.borrow_mut().batch_execute("BEGIN")
                .map_err(|e| KernelError::CallbackError(format!("BEGIN failed: {}", e)))?;
            self.in_transaction = true;
        }
        Ok(())
    }

    fn commit(&mut self) -> Result<(), KernelError> {
        if self.in_transaction {
            self.client.borrow_mut().batch_execute("COMMIT")
                .map_err(|e| KernelError::CallbackError(format!("COMMIT failed: {}", e)))?;
            self.in_transaction = false;
        }
        Ok(())
    }

    fn rollback(&mut self) -> Result<(), KernelError> {
        if self.in_transaction {
            self.client.borrow_mut().batch_execute("ROLLBACK")
                .map_err(|e| KernelError::CallbackError(format!("ROLLBACK failed: {}", e)))?;
            self.in_transaction = false;
        }
        Ok(())
    }
}

unsafe impl Send for PostgresBackend {}
unsafe impl Sync for PostgresBackend {}
