use crate::errors::KernelError;
use crate::event_log::EventPattern;
use crate::types::{AuthorityLevel, ControllerAction, Event, Resource};
use crate::invariant_checker::SystemQuery;

// ---------------------------------------------------------------------------
// Controller handler trait (bridged from Python)
// ---------------------------------------------------------------------------

pub trait ControllerHandler: Send + Sync {
    fn reconcile(&self, resource: &Resource, query: &dyn SystemQuery) -> Result<ControllerAction, KernelError>;
}

// ---------------------------------------------------------------------------
// Controller registration
// ---------------------------------------------------------------------------

pub struct ControllerRegistration {
    pub name: String,
    pub priority: u32,
    pub enforces: Vec<String>,
    pub on_events: Vec<EventPattern>,
    pub authority_level: AuthorityLevel,
    pub handler: Box<dyn ControllerHandler>,
}

impl std::fmt::Debug for ControllerRegistration {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ControllerRegistration")
            .field("name", &self.name)
            .field("priority", &self.priority)
            .field("enforces", &self.enforces)
            .finish()
    }
}

// ---------------------------------------------------------------------------
// Controller scheduler
// ---------------------------------------------------------------------------

pub struct ControllerScheduler {
    controllers: Vec<ControllerRegistration>,
    pub max_cascade_depth: u32,
}

impl ControllerScheduler {
    pub fn new() -> Self {
        Self {
            controllers: Vec::new(),
            max_cascade_depth: 10,
        }
    }

    pub fn register(&mut self, controller: ControllerRegistration) {
        self.controllers.push(controller);
        // Sort by priority descending
        self.controllers.sort_by(|a, b| b.priority.cmp(&a.priority));
    }

    /// Find controllers whose on_events patterns match the given event.
    pub fn get_matching_controllers(&self, event: &Event) -> Vec<&ControllerRegistration> {
        self.controllers
            .iter()
            .filter(|ctrl| {
                ctrl.on_events
                    .iter()
                    .any(|pattern| pattern.matches(&event.event_type))
            })
            .collect()
    }

    pub fn get_all_controllers(&self) -> &[ControllerRegistration] {
        &self.controllers
    }

    pub fn controller_count(&self) -> usize {
        self.controllers.len()
    }
}

impl Default for ControllerScheduler {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{AuthorityLevel, ResourceId};
    use chrono::Utc;
    use uuid::Uuid;

    struct NoOpHandler;
    impl ControllerHandler for NoOpHandler {
        fn reconcile(&self, _r: &Resource, _q: &dyn SystemQuery) -> Result<ControllerAction, KernelError> {
            Ok(ControllerAction::NoOp)
        }
    }

    fn make_event(event_type: &str) -> Event {
        Event {
            id: Uuid::new_v4(),
            offset: 0,
            event_type: event_type.into(),
            resource_id: ResourceId::new(),
            payload: serde_json::json!({}),
            actor: "test".into(),
            authority_level: AuthorityLevel::System,
            timestamp: Utc::now(),
        }
    }

    #[test]
    fn test_matching_controllers() {
        let mut scheduler = ControllerScheduler::new();
        scheduler.register(ControllerRegistration {
            name: "loan_ctrl".into(),
            priority: 50,
            enforces: vec![],
            on_events: vec![EventPattern::parse("loan.*")],
            authority_level: AuthorityLevel::Controller,
            handler: Box::new(NoOpHandler),
        });
        scheduler.register(ControllerRegistration {
            name: "all_ctrl".into(),
            priority: 10,
            enforces: vec![],
            on_events: vec![EventPattern::Wildcard],
            authority_level: AuthorityLevel::Controller,
            handler: Box::new(NoOpHandler),
        });

        let event = make_event("loan.created");
        let matching = scheduler.get_matching_controllers(&event);
        assert_eq!(matching.len(), 2);

        let event2 = make_event("application.created");
        let matching2 = scheduler.get_matching_controllers(&event2);
        assert_eq!(matching2.len(), 1);
        assert_eq!(matching2[0].name, "all_ctrl");
    }

    #[test]
    fn test_priority_ordering() {
        let mut scheduler = ControllerScheduler::new();
        scheduler.register(ControllerRegistration {
            name: "low".into(),
            priority: 10,
            enforces: vec![],
            on_events: vec![EventPattern::Wildcard],
            authority_level: AuthorityLevel::Controller,
            handler: Box::new(NoOpHandler),
        });
        scheduler.register(ControllerRegistration {
            name: "high".into(),
            priority: 90,
            enforces: vec![],
            on_events: vec![EventPattern::Wildcard],
            authority_level: AuthorityLevel::Controller,
            handler: Box::new(NoOpHandler),
        });

        assert_eq!(scheduler.get_all_controllers()[0].name, "high");
        assert_eq!(scheduler.get_all_controllers()[1].name, "low");
    }
}
