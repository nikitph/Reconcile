use postgres::Client;
use reconcile_core::audit_log::AuditStore;
use reconcile_core::types::{AuditRecord, AuthorityLevel, InvariantEvaluation, PolicyEvaluation, ResourceId};
use std::cell::RefCell;
use uuid::Uuid;

pub struct PostgresAuditStore {
    client: RefCell<Client>,
}

impl PostgresAuditStore {
    pub fn new(client: Client) -> Self {
        Self {
            client: RefCell::new(client),
        }
    }

    fn row_to_audit(row: &postgres::Row) -> AuditRecord {
        let id: Uuid = row.get("id");
        let resource_id: Uuid = row.get("resource_id");
        let authority_str: String = row.get("authority_level");
        let policies_json: serde_json::Value = row.get("policies_evaluated");
        let invariants_json: serde_json::Value = row.get("invariants_checked");

        let policies: Vec<PolicyEvaluation> = policies_json
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| {
                        Some(PolicyEvaluation {
                            name: v.get("name")?.as_str()?.to_string(),
                            passed: v.get("passed")?.as_bool()?,
                            message: v.get("message")?.as_str().unwrap_or("").to_string(),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();

        let invariants: Vec<InvariantEvaluation> = invariants_json
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| {
                        Some(InvariantEvaluation {
                            name: v.get("name")?.as_str()?.to_string(),
                            holds: v.get("holds")?.as_bool()?,
                            violation: v.get("violation").and_then(|v| v.as_str()).map(|s| s.to_string()),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();

        AuditRecord {
            id,
            resource_type: row.get("resource_type"),
            resource_id: ResourceId(resource_id),
            actor: row.get("actor"),
            role: row.get("role"),
            authority_level: AuthorityLevel::from_str(&authority_str)
                .unwrap_or(AuthorityLevel::System),
            previous_state: row.get("previous_state"),
            new_state: row.get("new_state"),
            policies_evaluated: policies,
            invariants_checked: invariants,
            timestamp: row.get("created_ts"),
        }
    }
}

impl AuditStore for PostgresAuditStore {
    fn write(&mut self, record: AuditRecord) {
        let policies_json = serde_json::to_value(&record.policies_evaluated)
            .unwrap_or(serde_json::Value::Array(vec![]));
        let invariants_json = serde_json::to_value(&record.invariants_checked)
            .unwrap_or(serde_json::Value::Array(vec![]));

        let _ = self.client.borrow_mut().execute(
            "INSERT INTO audit_log (id, resource_type, resource_id, actor, role, authority_level,
             previous_state, new_state, policies_evaluated, invariants_checked, created_ts)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)",
            &[
                &record.id,
                &record.resource_type,
                &record.resource_id.0,
                &record.actor,
                &record.role,
                &record.authority_level.to_string(),
                &record.previous_state,
                &record.new_state,
                &policies_json,
                &invariants_json,
                &record.timestamp,
            ],
        );
    }

    fn get_by_resource(&self, resource_id: &ResourceId) -> Vec<AuditRecord> {
        self.client.borrow_mut().query(
            "SELECT id, resource_type, resource_id, actor, role, authority_level,
             previous_state, new_state, policies_evaluated, invariants_checked, created_ts
             FROM audit_log WHERE resource_id = $1 ORDER BY created_ts",
            &[&resource_id.0],
        ).unwrap_or_default()
        .iter()
        .map(Self::row_to_audit)
        .collect()
    }

    fn get_all(&self) -> Vec<AuditRecord> {
        self.client.borrow_mut().query(
            "SELECT id, resource_type, resource_id, actor, role, authority_level,
             previous_state, new_state, policies_evaluated, invariants_checked, created_ts
             FROM audit_log ORDER BY created_ts",
            &[],
        ).unwrap_or_default()
        .iter()
        .map(Self::row_to_audit)
        .collect()
    }
}

unsafe impl Send for PostgresAuditStore {}
unsafe impl Sync for PostgresAuditStore {}
