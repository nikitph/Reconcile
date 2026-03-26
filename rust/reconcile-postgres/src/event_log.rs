use postgres::Client;
use reconcile_core::event_log::EventLog;
use reconcile_core::types::{AuthorityLevel, Event, ResourceId};
use std::cell::RefCell;
use uuid::Uuid;

pub struct PostgresEventLog {
    client: RefCell<Client>,
}

impl PostgresEventLog {
    pub fn new(client: Client) -> Self {
        Self {
            client: RefCell::new(client),
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
}

impl EventLog for PostgresEventLog {
    fn append(&mut self, event: Event) -> u64 {
        let row = self.client.borrow_mut().query_one(
            "INSERT INTO events (id, event_type, resource_id, payload, actor, authority_level, created_ts)
             VALUES ($1, $2, $3, $4, $5, $6, $7)
             RETURNING event_offset",
            &[
                &event.id,
                &event.event_type,
                &event.resource_id.0,
                &event.payload,
                &event.actor,
                &event.authority_level.to_string(),
                &event.timestamp,
            ],
        ).expect("Failed to insert event");

        let offset: i64 = row.get("event_offset");
        offset as u64
    }

    fn get_by_resource(&self, resource_id: &ResourceId) -> Vec<Event> {
        self.client.borrow_mut().query(
            "SELECT id, event_offset, event_type, resource_id, payload, actor, authority_level, created_ts
             FROM events WHERE resource_id = $1 ORDER BY event_offset",
            &[&resource_id.0],
        ).unwrap_or_default()
        .iter()
        .map(Self::row_to_event)
        .collect()
    }

    fn get_by_resource_since(&self, resource_id: &ResourceId, after_offset: u64) -> Vec<Event> {
        self.client.borrow_mut().query(
            "SELECT id, event_offset, event_type, resource_id, payload, actor, authority_level, created_ts
             FROM events WHERE resource_id = $1 AND event_offset > $2 ORDER BY event_offset",
            &[&resource_id.0, &(after_offset as i64)],
        ).unwrap_or_default()
        .iter()
        .map(Self::row_to_event)
        .collect()
    }

    fn get_all(&self) -> Vec<Event> {
        self.client.borrow_mut().query(
            "SELECT id, event_offset, event_type, resource_id, payload, actor, authority_level, created_ts
             FROM events ORDER BY event_offset",
            &[],
        ).unwrap_or_default()
        .iter()
        .map(Self::row_to_event)
        .collect()
    }

    fn get_latest_offset(&self) -> u64 {
        self.client.borrow_mut().query_one(
            "SELECT COALESCE(MAX(event_offset), 0) as max_offset FROM events",
            &[],
        ).map(|row| {
            let v: i64 = row.get("max_offset");
            v as u64
        }).unwrap_or(0)
    }
}

unsafe impl Send for PostgresEventLog {}
unsafe impl Sync for PostgresEventLog {}
