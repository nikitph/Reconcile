//! Agent framework — observe/propose pattern.
//!
//! Agents are read-only analyzers that observe events and emit Proposals.
//! Unlike controllers (which act immediately), agents advise. Proposals
//! flow through decision nodes before becoming actions.

use crate::errors::KernelError;
use crate::event_log::EventPattern;
use crate::invariant_checker::SystemQuery;
use crate::types::*;

// ---------------------------------------------------------------------------
// Agent handler trait
// ---------------------------------------------------------------------------

pub trait AgentHandler: Send + Sync {
    /// Observe a resource and optionally propose an action.
    /// Returns None if the agent has no opinion.
    fn observe(&self, resource: &Resource, query: &dyn SystemQuery) -> Option<Proposal>;
}

// ---------------------------------------------------------------------------
// Agent registration
// ---------------------------------------------------------------------------

pub struct AgentRegistration {
    pub name: String,
    pub priority: u32,
    pub on_events: Vec<EventPattern>,
    pub handler: Box<dyn AgentHandler>,
}

impl std::fmt::Debug for AgentRegistration {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AgentRegistration")
            .field("name", &self.name)
            .field("priority", &self.priority)
            .finish()
    }
}

// ---------------------------------------------------------------------------
// Agent scheduler
// ---------------------------------------------------------------------------

pub struct AgentScheduler {
    agents: Vec<AgentRegistration>,
}

impl AgentScheduler {
    pub fn new() -> Self {
        Self { agents: Vec::new() }
    }

    pub fn register(&mut self, agent: AgentRegistration) {
        self.agents.push(agent);
        self.agents.sort_by(|a, b| b.priority.cmp(&a.priority));
    }

    /// Find agents whose on_events patterns match the given event.
    pub fn get_matching_agents(&self, event: &Event) -> Vec<&AgentRegistration> {
        self.agents
            .iter()
            .filter(|a| a.on_events.iter().any(|p| p.matches(&event.event_type)))
            .collect()
    }

    /// Run all matching agents and collect proposals.
    pub fn collect_proposals(
        &self,
        event: &Event,
        resource: &Resource,
        query: &dyn SystemQuery,
    ) -> Vec<Proposal> {
        self.get_matching_agents(event)
            .iter()
            .filter_map(|agent| {
                agent.handler.observe(resource, query).map(|mut proposal| {
                    proposal.agent = agent.name.clone();
                    proposal
                })
            })
            .collect()
    }

    pub fn agent_count(&self) -> usize {
        self.agents.len()
    }
}

impl Default for AgentScheduler {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ResourceId;
    use chrono::Utc;
    use uuid::Uuid;

    struct EmptyQuery;
    impl SystemQuery for EmptyQuery {
        fn get_resource(&self, _id: &ResourceId) -> Option<Resource> { None }
        fn list_by_type(&self, _t: &str) -> Vec<Resource> { vec![] }
    }

    struct RiskScorer;
    impl AgentHandler for RiskScorer {
        fn observe(&self, resource: &Resource, _query: &dyn SystemQuery) -> Option<Proposal> {
            let amount = resource.data.get("amount").and_then(|v| v.as_f64()).unwrap_or(0.0);
            let confidence = if amount > 1_000_000.0 { 0.3 } else { 0.9 };
            Some(Proposal {
                id: Uuid::new_v4(),
                agent: "risk".into(),
                action: if confidence > 0.5 {
                    ProposedAction::Transition { to_state: "APPROVED".into() }
                } else {
                    ProposedAction::Flag { reason: "High risk".into() }
                },
                resource_id: resource.id.clone(),
                confidence,
                reasoning: format!("Amount {} risk score {}", amount, confidence),
                timestamp: Utc::now(),
            })
        }
    }

    struct NoOpAgent;
    impl AgentHandler for NoOpAgent {
        fn observe(&self, _resource: &Resource, _query: &dyn SystemQuery) -> Option<Proposal> {
            None
        }
    }

    fn make_resource(amount: f64) -> Resource {
        Resource {
            id: ResourceId::new(),
            resource_type: "loan".into(),
            state: "UNDERWRITING".into(),
            desired_state: None,
            data: serde_json::json!({"amount": amount}),
            version: 1,
            tenant_id: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    fn make_event(resource: &Resource) -> Event {
        Event {
            id: Uuid::new_v4(),
            offset: 0,
            event_type: "loan.transitioned".into(),
            resource_id: resource.id.clone(),
            payload: serde_json::json!({}),
            actor: "test".into(),
            authority_level: AuthorityLevel::Human,
            timestamp: Utc::now(),
        }
    }

    #[test]
    fn test_agent_produces_proposal() {
        let r = make_resource(500_000.0);
        let scorer = RiskScorer;
        let proposal = scorer.observe(&r, &EmptyQuery).unwrap();
        assert_eq!(proposal.agent, "risk");
        assert!(proposal.confidence > 0.5);
        assert!(matches!(proposal.action, ProposedAction::Transition { .. }));
    }

    #[test]
    fn test_agent_high_risk_flags() {
        let r = make_resource(5_000_000.0);
        let scorer = RiskScorer;
        let proposal = scorer.observe(&r, &EmptyQuery).unwrap();
        assert!(proposal.confidence < 0.5);
        assert!(matches!(proposal.action, ProposedAction::Flag { .. }));
    }

    #[test]
    fn test_noop_agent_returns_none() {
        let r = make_resource(100.0);
        assert!(NoOpAgent.observe(&r, &EmptyQuery).is_none());
    }

    #[test]
    fn test_scheduler_collects_proposals() {
        let mut scheduler = AgentScheduler::new();
        scheduler.register(AgentRegistration {
            name: "risk".into(),
            priority: 80,
            on_events: vec![EventPattern::parse("loan.*")],
            handler: Box::new(RiskScorer),
        });
        scheduler.register(AgentRegistration {
            name: "noop".into(),
            priority: 10,
            on_events: vec![EventPattern::parse("loan.*")],
            handler: Box::new(NoOpAgent),
        });

        let r = make_resource(500_000.0);
        let event = make_event(&r);
        let proposals = scheduler.collect_proposals(&event, &r, &EmptyQuery);
        assert_eq!(proposals.len(), 1); // Only RiskScorer produces a proposal
        assert_eq!(proposals[0].agent, "risk");
    }

    #[test]
    fn test_scheduler_event_filtering() {
        let mut scheduler = AgentScheduler::new();
        scheduler.register(AgentRegistration {
            name: "loan_only".into(),
            priority: 50,
            on_events: vec![EventPattern::parse("loan.*")],
            handler: Box::new(RiskScorer),
        });

        let r = make_resource(100.0);

        // Matching event
        let matching = Event {
            id: Uuid::new_v4(), offset: 0, event_type: "loan.created".into(),
            resource_id: r.id.clone(), payload: serde_json::json!({}),
            actor: "test".into(), authority_level: AuthorityLevel::Human, timestamp: Utc::now(),
        };
        assert_eq!(scheduler.collect_proposals(&matching, &r, &EmptyQuery).len(), 1);

        // Non-matching event
        let non_matching = Event {
            id: Uuid::new_v4(), offset: 0, event_type: "claim.created".into(),
            resource_id: r.id.clone(), payload: serde_json::json!({}),
            actor: "test".into(), authority_level: AuthorityLevel::Human, timestamp: Utc::now(),
        };
        assert_eq!(scheduler.collect_proposals(&non_matching, &r, &EmptyQuery).len(), 0);
    }

    #[test]
    fn test_priority_ordering() {
        let mut scheduler = AgentScheduler::new();
        scheduler.register(AgentRegistration {
            name: "low".into(), priority: 10,
            on_events: vec![EventPattern::Wildcard],
            handler: Box::new(NoOpAgent),
        });
        scheduler.register(AgentRegistration {
            name: "high".into(), priority: 90,
            on_events: vec![EventPattern::Wildcard],
            handler: Box::new(NoOpAgent),
        });
        // High priority should be first
        let agents = scheduler.get_matching_agents(&make_event(&make_resource(0.0)));
        assert_eq!(agents[0].name, "high");
    }
}
