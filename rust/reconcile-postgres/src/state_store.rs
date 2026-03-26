use postgres::Client;
use reconcile_core::errors::KernelError;
use reconcile_core::state_store::StateStore;
use reconcile_core::types::{Resource, ResourceId};
use std::cell::RefCell;
use uuid::Uuid;

pub struct PostgresStateStore {
    client: RefCell<Client>,
}

impl PostgresStateStore {
    pub fn new(client: Client) -> Self {
        Self {
            client: RefCell::new(client),
        }
    }

    fn row_to_resource(row: &postgres::Row) -> Resource {
        let id: Uuid = row.get("id");
        let desired_state: Option<String> = row.get("desired_state");
        Resource {
            id: ResourceId(id),
            resource_type: row.get("resource_type"),
            state: row.get("state"),
            desired_state,
            data: row.get("data"),
            version: row.get::<_, i32>("version") as u64,
            created_at: row.get("created_at"),
            updated_at: row.get("updated_at"),
        }
    }
}

impl StateStore for PostgresStateStore {
    fn create(&mut self, resource: Resource) -> Result<(), KernelError> {
        self.client.borrow_mut().execute(
            "INSERT INTO resources (id, resource_type, state, desired_state, data, version, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
            &[
                &resource.id.0,
                &resource.resource_type,
                &resource.state,
                &resource.desired_state,
                &resource.data,
                &(resource.version as i32),
                &resource.created_at,
                &resource.updated_at,
            ],
        ).map_err(|e| KernelError::CallbackError(format!("PG insert failed: {}", e)))?;
        Ok(())
    }

    fn get(&self, id: &ResourceId) -> Option<Resource> {
        self.client.borrow_mut().query_opt(
            "SELECT id, resource_type, state, desired_state, data, version, created_at, updated_at
             FROM resources WHERE id = $1",
            &[&id.0],
        ).ok().flatten().map(|row| Self::row_to_resource(&row))
    }

    fn update(&mut self, resource: &Resource) -> Result<(), KernelError> {
        let rows = self.client.borrow_mut().execute(
            "UPDATE resources
             SET state = $2, desired_state = $3, data = $4, version = $5, updated_at = $6
             WHERE id = $1 AND version = $5 - 1",
            &[
                &resource.id.0,
                &resource.state,
                &resource.desired_state,
                &resource.data,
                &(resource.version as i32),
                &resource.updated_at,
            ],
        ).map_err(|e| KernelError::CallbackError(format!("PG update failed: {}", e)))?;

        if rows == 0 {
            let exists = self.get(&resource.id).is_some();
            if exists {
                return Err(KernelError::VersionConflict {
                    resource_id: resource.id.clone(),
                    expected: resource.version - 1,
                    found: resource.version,
                });
            } else {
                return Err(KernelError::ResourceNotFound(resource.id.clone()));
            }
        }
        Ok(())
    }

    fn list_by_type(&self, resource_type: &str) -> Vec<Resource> {
        self.client.borrow_mut().query(
            "SELECT id, resource_type, state, desired_state, data, version, created_at, updated_at
             FROM resources WHERE resource_type = $1",
            &[&resource_type],
        ).unwrap_or_default()
        .iter()
        .map(Self::row_to_resource)
        .collect()
    }

    fn list_by_state(&self, resource_type: &str, state: &str) -> Vec<Resource> {
        self.client.borrow_mut().query(
            "SELECT id, resource_type, state, desired_state, data, version, created_at, updated_at
             FROM resources WHERE resource_type = $1 AND state = $2",
            &[&resource_type, &state],
        ).unwrap_or_default()
        .iter()
        .map(Self::row_to_resource)
        .collect()
    }
}

unsafe impl Send for PostgresStateStore {}
unsafe impl Sync for PostgresStateStore {}
