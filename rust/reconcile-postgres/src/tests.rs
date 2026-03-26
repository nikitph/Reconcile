//! Integration tests for PostgreSQL storage backends.
//! Requires a running PostgreSQL instance with database `reconcile_test`.
//! Run with: cargo test -p reconcile-postgres -- --test-threads=1

#[cfg(test)]
mod pg_tests {
    use crate::*;
    use postgres::Client;
    use reconcile_core::audit_log::AuditStore;
    use reconcile_core::event_log::EventLog;
    use reconcile_core::snapshot_store::SnapshotStore;
    use reconcile_core::state_store::StateStore;
    use reconcile_core::types::*;
    use chrono::Utc;
    use uuid::Uuid;

    const CONN_STR: &str = "host=localhost dbname=reconcile_test";

    fn clean_and_connect() -> Option<Client> {
        // Use our connect() which runs refinery migrations
        let mut client = crate::connect(CONN_STR).ok()?;
        client.batch_execute(
            "TRUNCATE snapshots, audit_log, events, resources CASCADE"
        ).expect("TRUNCATE failed — tables should exist after migration");
        Some(client)
    }

    /// Helper: insert a resource directly so FK constraints work for event/audit/snapshot tests.
    fn insert_resource(client: &mut Client, rid: &ResourceId) {
        client.execute(
            "INSERT INTO resources (id, resource_type, state, data, version, created_at, updated_at)
             VALUES ($1, 'loan', 'APPLIED', '{}', 1, NOW(), NOW())",
            &[&rid.0],
        ).unwrap();
    }

    // ---- StateStore tests ----

    #[test]
    fn test_pg_state_store_create_and_get() {
        let client = match clean_and_connect() {
            Some(c) => c,
            None => { eprintln!("Skipping: no PG"); return; }
        };
        let mut store = PostgresStateStore::new(client);

        let id = ResourceId::new();
        let resource = Resource {
            id: id.clone(),
            resource_type: "loan".into(),
            state: "APPLIED".into(),
            desired_state: None,
            data: serde_json::json!({"amount": 500_000, "applicant": "Acme"}),
            version: 1,
            tenant_id: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };

        store.create(resource).unwrap();

        let fetched = store.get(&id).expect("Resource should exist");
        assert_eq!(fetched.state, "APPLIED");
        assert_eq!(fetched.resource_type, "loan");
        assert_eq!(fetched.data["amount"], 500_000);
        assert_eq!(fetched.data["applicant"], "Acme");
        assert_eq!(fetched.version, 1);
    }

    #[test]
    fn test_pg_state_store_get_nonexistent() {
        let client = match clean_and_connect() {
            Some(c) => c,
            None => { eprintln!("Skipping: no PG"); return; }
        };
        let store = PostgresStateStore::new(client);
        assert!(store.get(&ResourceId::new()).is_none());
    }

    #[test]
    fn test_pg_state_store_update() {
        let client = match clean_and_connect() {
            Some(c) => c,
            None => { eprintln!("Skipping: no PG"); return; }
        };
        let mut store = PostgresStateStore::new(client);

        let id = ResourceId::new();
        store.create(Resource {
            id: id.clone(),
            resource_type: "loan".into(),
            state: "APPLIED".into(),
            desired_state: None,
            data: serde_json::json!({"amount": 100}),
            version: 1,
            tenant_id: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }).unwrap();

        let mut updated = store.get(&id).unwrap();
        updated.state = "UNDERWRITING".into();
        updated.version = 2;
        updated.updated_at = Utc::now();
        store.update(&updated).unwrap();

        let fetched = store.get(&id).unwrap();
        assert_eq!(fetched.state, "UNDERWRITING");
        assert_eq!(fetched.version, 2);
    }

    #[test]
    fn test_pg_state_store_optimistic_locking() {
        let client = match clean_and_connect() {
            Some(c) => c,
            None => { eprintln!("Skipping: no PG"); return; }
        };
        let mut store = PostgresStateStore::new(client);

        let id = ResourceId::new();
        store.create(Resource {
            id: id.clone(),
            resource_type: "loan".into(),
            state: "APPLIED".into(),
            desired_state: None,
            data: serde_json::json!({}),
            version: 1,
            tenant_id: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }).unwrap();

        // Try to update with wrong version (skip version 2, go to 3)
        let mut bad = store.get(&id).unwrap();
        bad.state = "BOGUS".into();
        bad.version = 3;
        let result = store.update(&bad);
        assert!(result.is_err(), "Optimistic locking should reject stale update");

        // Original state unchanged
        let fetched = store.get(&id).unwrap();
        assert_eq!(fetched.state, "APPLIED");
        assert_eq!(fetched.version, 1);
    }

    #[test]
    fn test_pg_state_store_list_by_type() {
        let client = match clean_and_connect() {
            Some(c) => c,
            None => { eprintln!("Skipping: no PG"); return; }
        };
        let mut store = PostgresStateStore::new(client);

        for state in ["APPLIED", "UNDERWRITING", "APPROVED"] {
            store.create(Resource {
                id: ResourceId::new(),
                resource_type: "loan".into(),
                state: state.into(),
                desired_state: None,
                data: serde_json::json!({}),
                version: 1,
                tenant_id: None,
                created_at: Utc::now(),
                updated_at: Utc::now(),
            }).unwrap();
        }
        store.create(Resource {
            id: ResourceId::new(),
            resource_type: "claim".into(),
            state: "FILED".into(),
            desired_state: None,
            data: serde_json::json!({}),
            version: 1,
            tenant_id: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }).unwrap();

        assert_eq!(store.list_by_type("loan").len(), 3);
        assert_eq!(store.list_by_type("claim").len(), 1);
        assert_eq!(store.list_by_state("loan", "APPLIED").len(), 1);
    }

    // ---- EventLog tests ----

    #[test]
    fn test_pg_event_log_append_and_query() {
        let mut client = match clean_and_connect() {
            Some(c) => c,
            None => { eprintln!("Skipping: no PG"); return; }
        };

        // Insert resource first (FK constraint)
        let rid = ResourceId::new();
        insert_resource(&mut client, &rid);

        let mut log = PostgresEventLog::new(client);

        let offset1 = log.append(Event {
            id: Uuid::new_v4(),
            offset: 0,
            event_type: "loan.created".into(),
            resource_id: rid.clone(),
            payload: serde_json::json!({"state": "APPLIED"}),
            actor: "user1".into(),
            authority_level: AuthorityLevel::Human,
            timestamp: Utc::now(),
        });

        let offset2 = log.append(Event {
            id: Uuid::new_v4(),
            offset: 0,
            event_type: "loan.transitioned".into(),
            resource_id: rid.clone(),
            payload: serde_json::json!({"from": "APPLIED", "to": "UW"}),
            actor: "user2".into(),
            authority_level: AuthorityLevel::Controller,
            timestamp: Utc::now(),
        });

        assert!(offset2 > offset1, "Offsets should increase: {} > {}", offset2, offset1);

        let events = log.get_by_resource(&rid);
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].event_type, "loan.created");
        assert_eq!(events[1].event_type, "loan.transitioned");
        assert_eq!(events[1].actor, "user2");
        assert!(events[1].authority_level == AuthorityLevel::Controller);

        assert!(log.get_latest_offset() >= offset2);
    }

    // ---- AuditStore tests ----

    #[test]
    fn test_pg_audit_store_write_and_query() {
        let mut client = match clean_and_connect() {
            Some(c) => c,
            None => { eprintln!("Skipping: no PG"); return; }
        };

        let rid = ResourceId::new();
        insert_resource(&mut client, &rid);

        let mut store = PostgresAuditStore::new(client);

        store.write(AuditRecord {
            id: Uuid::new_v4(),
            resource_type: "loan".into(),
            resource_id: rid.clone(),
            actor: "alice".into(),
            role: "officer".into(),
            authority_level: AuthorityLevel::Human,
            previous_state: "APPLIED".into(),
            new_state: "UNDERWRITING".into(),
            policies_evaluated: vec![
                PolicyEvaluation { name: "high_value".into(), passed: true, message: "".into() },
            ],
            invariants_checked: vec![
                InvariantEvaluation { name: "positive_amount".into(), holds: true, violation: None },
            ],
            timestamp: Utc::now(),
        });

        let records = store.get_by_resource(&rid);
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].actor, "alice");
        assert_eq!(records[0].previous_state, "APPLIED");
        assert_eq!(records[0].new_state, "UNDERWRITING");
        assert_eq!(records[0].policies_evaluated.len(), 1);
        assert_eq!(records[0].policies_evaluated[0].name, "high_value");
        assert!(records[0].policies_evaluated[0].passed);
        assert_eq!(records[0].invariants_checked.len(), 1);
        assert!(records[0].invariants_checked[0].holds);
    }

    // ---- SnapshotStore tests ----

    #[test]
    fn test_pg_snapshot_store() {
        let mut client = match clean_and_connect() {
            Some(c) => c,
            None => { eprintln!("Skipping: no PG"); return; }
        };

        let rid = ResourceId::new();
        insert_resource(&mut client, &rid);

        let mut store = PostgresSnapshotStore::new(client);

        store.create(Snapshot {
            id: Uuid::new_v4(),
            resource_id: rid.clone(),
            state: "APPLIED".into(),
            data: serde_json::json!({"amount": 100}),
            version: 1,
            event_offset: 0,
            timestamp: Utc::now(),
        });

        store.create(Snapshot {
            id: Uuid::new_v4(),
            resource_id: rid.clone(),
            state: "APPROVED".into(),
            data: serde_json::json!({"amount": 100}),
            version: 3,
            event_offset: 10,
            timestamp: Utc::now(),
        });

        let latest = store.get_latest(&rid).expect("Should have snapshot");
        assert_eq!(latest.state, "APPROVED");
        assert_eq!(latest.event_offset, 10);
        assert_eq!(latest.version, 3);
    }

    // ---- Full Kernel integration with PostgresBackend ----

    #[test]
    fn test_kernel_with_postgres_backend_full_lifecycle() {
        let client = match clean_and_connect() {
            Some(c) => c,
            None => { eprintln!("Skipping: no PG"); return; }
        };

        use crate::PostgresBackend;
        use reconcile_core::transaction::Kernel;
        use reconcile_core::resource_registry::ResourceTypeDefinition;
        use reconcile_core::state_machine::{StateDefinition, StateStatus, StateMachine, TransitionDefinition};
        use reconcile_core::roles::{RoleDefinition, Permission};

        let backend = PostgresBackend::new(client);
        let mut kernel = Kernel::with_storage(Box::new(backend));

        // Register loan type
        let states = vec![
            StateDefinition { name: "APPLIED".into(), status: StateStatus::Active },
            StateDefinition { name: "UNDERWRITING".into(), status: StateStatus::Active },
            StateDefinition { name: "APPROVED".into(), status: StateStatus::Active },
            StateDefinition { name: "DISBURSED".into(), status: StateStatus::Terminal },
            StateDefinition { name: "REJECTED".into(), status: StateStatus::Terminal },
        ];
        let transitions = vec![
            TransitionDefinition { from_state: "APPLIED".into(), to_state: "UNDERWRITING".into(), guard: None, required_role: None },
            TransitionDefinition { from_state: "UNDERWRITING".into(), to_state: "APPROVED".into(), guard: None, required_role: None },
            TransitionDefinition { from_state: "UNDERWRITING".into(), to_state: "REJECTED".into(), guard: None, required_role: None },
            TransitionDefinition { from_state: "APPROVED".into(), to_state: "DISBURSED".into(), guard: None, required_role: None },
        ];
        let sm = StateMachine::new(states, transitions, "APPLIED".into()).unwrap();
        kernel.register_type(ResourceTypeDefinition {
            name: "loan".into(), schema: serde_json::json!({}), state_machine: sm,
        }).unwrap();

        kernel.role_registry.register(RoleDefinition {
            name: "manager".into(),
            permissions: vec![Permission::from_shorthand("transition:*")],
        });

        // Create resource
        let resource = match kernel.create_resource(
            "loan", serde_json::json!({"amount": 500_000, "applicant": "Acme"}),
            "user1", AuthorityLevel::Human,
        ).unwrap() {
            TransitionOutcome::Success { resource, .. } => resource,
            _ => panic!("Create should succeed"),
        };
        assert_eq!(resource.state, "APPLIED");

        // Full lifecycle: APPLIED -> UW -> APPROVED -> DISBURSED
        let r = kernel.transition(&resource.id, "UNDERWRITING", "u1", "manager", AuthorityLevel::Human).unwrap();
        assert!(matches!(r, TransitionOutcome::Success { .. }));

        let r = kernel.transition(&resource.id, "APPROVED", "u1", "manager", AuthorityLevel::Human).unwrap();
        assert!(matches!(r, TransitionOutcome::Success { .. }));

        let r = kernel.transition(&resource.id, "DISBURSED", "u1", "manager", AuthorityLevel::Human).unwrap();
        assert!(matches!(r, TransitionOutcome::Success { .. }));

        // Verify final state persisted in PG
        let final_r = kernel.get_resource(&resource.id).unwrap();
        assert_eq!(final_r.state, "DISBURSED");
        assert_eq!(final_r.version, 4);
        assert_eq!(final_r.data["applicant"], "Acme");

        // Verify audit trail in PG
        let audit = kernel.get_audit(&resource.id);
        assert_eq!(audit.len(), 3);
        assert_eq!(audit[0].new_state, "UNDERWRITING");
        assert_eq!(audit[1].new_state, "APPROVED");
        assert_eq!(audit[2].new_state, "DISBURSED");

        // Verify events in PG
        let events = kernel.get_events(&resource.id);
        assert!(events.len() >= 4); // created + 3 transitions
        assert_eq!(events[0].event_type, "loan.created");
    }

    #[test]
    fn test_kernel_with_postgres_rejected_transition_no_side_effects() {
        let client = match clean_and_connect() {
            Some(c) => c,
            None => { eprintln!("Skipping: no PG"); return; }
        };

        use crate::PostgresBackend;
        use reconcile_core::transaction::Kernel;
        use reconcile_core::resource_registry::ResourceTypeDefinition;
        use reconcile_core::state_machine::{StateDefinition, StateStatus, StateMachine, TransitionDefinition};

        let backend = PostgresBackend::new(client);
        let mut kernel = Kernel::with_storage(Box::new(backend));

        let states = vec![
            StateDefinition { name: "A".into(), status: StateStatus::Active },
            StateDefinition { name: "B".into(), status: StateStatus::Terminal },
        ];
        let transitions = vec![
            TransitionDefinition { from_state: "A".into(), to_state: "B".into(), guard: None, required_role: None },
        ];
        let sm = StateMachine::new(states, transitions, "A".into()).unwrap();
        kernel.register_type(ResourceTypeDefinition {
            name: "item".into(), schema: serde_json::json!({}), state_machine: sm,
        }).unwrap();

        let resource = match kernel.create_resource("item", serde_json::json!({}), "sys", AuthorityLevel::System).unwrap() {
            TransitionOutcome::Success { resource, .. } => resource,
            _ => panic!("Expected success"),
        };

        // Invalid transition: A -> NONEXISTENT
        let result = kernel.transition(&resource.id, "NONEXISTENT", "sys", "sys", AuthorityLevel::Controller).unwrap();
        assert!(matches!(result, TransitionOutcome::Rejected { .. }));

        // State should be unchanged in PG
        let r = kernel.get_resource(&resource.id).unwrap();
        assert_eq!(r.state, "A");
        assert_eq!(r.version, 1);
    }
}
