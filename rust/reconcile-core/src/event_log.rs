use crate::types::{Event, ResourceId};
use std::collections::HashMap;

// ---------------------------------------------------------------------------
// Event pattern for subscriptions
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum EventPattern {
    Exact(String),
    Prefix(String),
    Wildcard,
}

impl EventPattern {
    pub fn matches(&self, event_type: &str) -> bool {
        match self {
            EventPattern::Exact(s) => event_type == s,
            EventPattern::Prefix(p) => event_type.starts_with(p),
            EventPattern::Wildcard => true,
        }
    }

    pub fn parse(pattern: &str) -> Self {
        if pattern == "*" {
            EventPattern::Wildcard
        } else if pattern.ends_with(".*") || pattern.ends_with("*") {
            EventPattern::Prefix(pattern.trim_end_matches('*').trim_end_matches('.').to_string() + ".")
        } else {
            EventPattern::Exact(pattern.to_string())
        }
    }
}

// ---------------------------------------------------------------------------
// Event log trait + in-memory impl
// ---------------------------------------------------------------------------

pub trait EventLog: Send + Sync {
    fn append(&mut self, event: Event) -> u64;
    fn get_by_resource(&self, resource_id: &ResourceId) -> Vec<Event>;
    fn get_by_resource_since(&self, resource_id: &ResourceId, after_offset: u64) -> Vec<Event>;
    fn get_all(&self) -> Vec<Event>;
    fn get_latest_offset(&self) -> u64;
}

pub struct InMemoryEventLog {
    events: Vec<Event>,
    next_offset: u64,
    resource_index: HashMap<ResourceId, Vec<usize>>,
}

impl InMemoryEventLog {
    pub fn new() -> Self {
        Self {
            events: Vec::new(),
            next_offset: 0,
            resource_index: HashMap::new(),
        }
    }
}

impl Default for InMemoryEventLog {
    fn default() -> Self {
        Self::new()
    }
}

impl EventLog for InMemoryEventLog {
    fn append(&mut self, mut event: Event) -> u64 {
        let offset = self.next_offset;
        event.offset = offset;
        self.next_offset += 1;

        let idx = self.events.len();
        self.resource_index
            .entry(event.resource_id.clone())
            .or_default()
            .push(idx);
        self.events.push(event);
        offset
    }

    fn get_by_resource(&self, resource_id: &ResourceId) -> Vec<Event> {
        self.resource_index
            .get(resource_id)
            .map(|indices| indices.iter().map(|&i| self.events[i].clone()).collect())
            .unwrap_or_default()
    }

    fn get_by_resource_since(&self, resource_id: &ResourceId, after_offset: u64) -> Vec<Event> {
        self.resource_index
            .get(resource_id)
            .map(|indices| {
                indices.iter()
                    .map(|&i| &self.events[i])
                    .filter(|e| e.offset > after_offset)
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }

    fn get_all(&self) -> Vec<Event> {
        self.events.clone()
    }

    fn get_latest_offset(&self) -> u64 {
        if self.next_offset == 0 {
            0
        } else {
            self.next_offset - 1
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{AuthorityLevel, ResourceId};
    use chrono::Utc;
    use uuid::Uuid;

    fn make_event(resource_id: &ResourceId, event_type: &str) -> Event {
        Event {
            id: Uuid::new_v4(),
            offset: 0,
            event_type: event_type.into(),
            resource_id: resource_id.clone(),
            payload: serde_json::json!({}),
            actor: "test".into(),
            authority_level: AuthorityLevel::Human,
            timestamp: Utc::now(),
        }
    }

    #[test]
    fn test_append_and_offset() {
        let mut log = InMemoryEventLog::new();
        let rid = ResourceId::new();
        let o1 = log.append(make_event(&rid, "loan.created"));
        let o2 = log.append(make_event(&rid, "loan.transitioned"));
        assert_eq!(o1, 0);
        assert_eq!(o2, 1);
        assert_eq!(log.get_latest_offset(), 1);
    }

    #[test]
    fn test_get_by_resource() {
        let mut log = InMemoryEventLog::new();
        let rid1 = ResourceId::new();
        let rid2 = ResourceId::new();
        log.append(make_event(&rid1, "loan.created"));
        log.append(make_event(&rid2, "loan.created"));
        log.append(make_event(&rid1, "loan.transitioned"));

        assert_eq!(log.get_by_resource(&rid1).len(), 2);
        assert_eq!(log.get_by_resource(&rid2).len(), 1);
    }

    #[test]
    fn test_get_all() {
        let mut log = InMemoryEventLog::new();
        let rid = ResourceId::new();
        log.append(make_event(&rid, "a"));
        log.append(make_event(&rid, "b"));
        assert_eq!(log.get_all().len(), 2);
    }

    #[test]
    fn test_event_pattern_matching() {
        assert!(EventPattern::Exact("loan.created".into()).matches("loan.created"));
        assert!(!EventPattern::Exact("loan.created".into()).matches("loan.transitioned"));
        assert!(EventPattern::Wildcard.matches("anything"));
        assert!(EventPattern::parse("loan.*").matches("loan.created"));
        assert!(EventPattern::parse("loan.*").matches("loan.transitioned"));
        assert!(!EventPattern::parse("loan.*").matches("application.created"));
    }

    #[test]
    fn test_per_resource_ordering() {
        let mut log = InMemoryEventLog::new();
        let rid = ResourceId::new();
        log.append(make_event(&rid, "a"));
        log.append(make_event(&rid, "b"));
        log.append(make_event(&rid, "c"));

        let events = log.get_by_resource(&rid);
        let offsets: Vec<u64> = events.iter().map(|e| e.offset).collect();
        assert_eq!(offsets, vec![0, 1, 2]);
    }
}
