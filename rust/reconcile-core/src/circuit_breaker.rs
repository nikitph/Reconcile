//! Circuit Breaker — stops dispatching to repeatedly failing controllers.
//!
//! Tracks consecutive failure counts per controller. After exceeding the
//! threshold, the circuit opens and events are routed to the dead letter queue.

use crate::types::DeadLetter;
use std::collections::{HashMap, HashSet};

pub struct CircuitBreaker {
    failure_counts: HashMap<String, u32>,
    open_circuits: HashSet<String>,
    pub threshold: u32,
    dead_letters: Vec<DeadLetter>,
}

impl CircuitBreaker {
    pub fn new(threshold: u32) -> Self {
        Self {
            failure_counts: HashMap::new(),
            open_circuits: HashSet::new(),
            threshold,
            dead_letters: Vec::new(),
        }
    }

    pub fn record_success(&mut self, controller: &str) {
        self.failure_counts.remove(controller);
        self.open_circuits.remove(controller);
    }

    pub fn record_failure(&mut self, controller: &str) {
        let count = self.failure_counts.entry(controller.to_string()).or_insert(0);
        *count += 1;
        if *count >= self.threshold {
            self.open_circuits.insert(controller.to_string());
        }
    }

    pub fn is_open(&self, controller: &str) -> bool {
        self.open_circuits.contains(controller)
    }

    pub fn reset(&mut self, controller: &str) {
        self.failure_counts.remove(controller);
        self.open_circuits.remove(controller);
    }

    pub fn add_dead_letter(&mut self, dl: DeadLetter) {
        self.dead_letters.push(dl);
    }

    pub fn dead_letters(&self) -> &[DeadLetter] {
        &self.dead_letters
    }

    pub fn dead_letter_count(&self) -> usize {
        self.dead_letters.len()
    }

    pub fn open_circuit_count(&self) -> usize {
        self.open_circuits.len()
    }

    pub fn get_failure_count(&self, controller: &str) -> u32 {
        self.failure_counts.get(controller).copied().unwrap_or(0)
    }
}

impl Default for CircuitBreaker {
    fn default() -> Self {
        Self::new(5)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_circuit_stays_closed() {
        let mut cb = CircuitBreaker::new(3);
        cb.record_failure("ctrl_a");
        cb.record_failure("ctrl_a");
        assert!(!cb.is_open("ctrl_a"));
    }

    #[test]
    fn test_circuit_opens_at_threshold() {
        let mut cb = CircuitBreaker::new(3);
        cb.record_failure("ctrl_a");
        cb.record_failure("ctrl_a");
        cb.record_failure("ctrl_a");
        assert!(cb.is_open("ctrl_a"));
    }

    #[test]
    fn test_success_resets_count() {
        let mut cb = CircuitBreaker::new(3);
        cb.record_failure("ctrl_a");
        cb.record_failure("ctrl_a");
        cb.record_success("ctrl_a");
        cb.record_failure("ctrl_a");
        cb.record_failure("ctrl_a");
        // Only 2 consecutive failures after reset
        assert!(!cb.is_open("ctrl_a"));
    }

    #[test]
    fn test_manual_reset() {
        let mut cb = CircuitBreaker::new(3);
        for _ in 0..5 {
            cb.record_failure("ctrl_a");
        }
        assert!(cb.is_open("ctrl_a"));
        cb.reset("ctrl_a");
        assert!(!cb.is_open("ctrl_a"));
    }

    #[test]
    fn test_independent_circuits() {
        let mut cb = CircuitBreaker::new(2);
        cb.record_failure("ctrl_a");
        cb.record_failure("ctrl_a");
        cb.record_failure("ctrl_b");
        assert!(cb.is_open("ctrl_a"));
        assert!(!cb.is_open("ctrl_b"));
    }

    #[test]
    fn test_dead_letter_queue() {
        let mut cb = CircuitBreaker::new(2);
        assert_eq!(cb.dead_letter_count(), 0);
        cb.add_dead_letter(DeadLetter {
            event: crate::types::Event {
                id: uuid::Uuid::new_v4(),
                offset: 0,
                event_type: "test".into(),
                resource_id: crate::types::ResourceId::new(),
                payload: serde_json::json!({}),
                actor: "test".into(),
                authority_level: crate::types::AuthorityLevel::System,
                timestamp: chrono::Utc::now(),
            },
            controller: "broken_ctrl".into(),
            error: "panicked".into(),
            attempts: 3,
            timestamp: chrono::Utc::now(),
        });
        assert_eq!(cb.dead_letter_count(), 1);
        assert_eq!(cb.dead_letters()[0].controller, "broken_ctrl");
    }
}
