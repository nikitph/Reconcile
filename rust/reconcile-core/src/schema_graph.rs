//! Schema Graph — static type topology for compile-time validation.
//!
//! Defines relationships between resource types and provides algorithms
//! for reachability, closure, and validation. The schema graph is immutable
//! after system definition — it changes only when types are registered.

use std::collections::{HashMap, HashSet, VecDeque};

// ---------------------------------------------------------------------------
// Relationship declarations
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cardinality {
    OneToOne,
    OneToMany,
    ManyToOne,
    ManyToMany,
}

#[derive(Debug, Clone)]
pub struct RelationshipDeclaration {
    pub from_type: String,
    pub to_type: String,
    pub relation: String,
    pub cardinality: Cardinality,
    pub required: bool,
    /// Field name in the from_type's data that holds the reference ID
    pub foreign_key: String,
}

// ---------------------------------------------------------------------------
// Schema graph
// ---------------------------------------------------------------------------

pub struct SchemaGraph {
    types: HashSet<String>,
    relationships: Vec<RelationshipDeclaration>,
    /// Adjacency: from_type -> [(relation, to_type, index)]
    outbound: HashMap<String, Vec<(String, String, usize)>>,
    /// Reverse adjacency: to_type -> [(relation, from_type, index)]
    inbound: HashMap<String, Vec<(String, String, usize)>>,
}

impl SchemaGraph {
    pub fn new() -> Self {
        Self {
            types: HashSet::new(),
            relationships: Vec::new(),
            outbound: HashMap::new(),
            inbound: HashMap::new(),
        }
    }

    pub fn register_type(&mut self, type_name: &str) {
        self.types.insert(type_name.to_string());
    }

    pub fn add_relationship(&mut self, rel: RelationshipDeclaration) {
        let idx = self.relationships.len();
        self.outbound
            .entry(rel.from_type.clone())
            .or_default()
            .push((rel.relation.clone(), rel.to_type.clone(), idx));
        self.inbound
            .entry(rel.to_type.clone())
            .or_default()
            .push((rel.relation.clone(), rel.from_type.clone(), idx));
        self.relationships.push(rel);
    }

    pub fn get_relationships(&self) -> &[RelationshipDeclaration] {
        &self.relationships
    }

    /// Get all outbound relationships from a type.
    pub fn outbound_relations(&self, from_type: &str) -> Vec<&RelationshipDeclaration> {
        self.outbound
            .get(from_type)
            .map(|rels| rels.iter().map(|(_, _, idx)| &self.relationships[*idx]).collect())
            .unwrap_or_default()
    }

    /// Get all inbound relationships to a type.
    pub fn inbound_relations(&self, to_type: &str) -> Vec<&RelationshipDeclaration> {
        self.inbound
            .get(to_type)
            .map(|rels| rels.iter().map(|(_, _, idx)| &self.relationships[*idx]).collect())
            .unwrap_or_default()
    }

    /// Can type `from` reach type `to` via any chain of relationships?
    pub fn reachable(&self, from: &str, to: &str) -> bool {
        if from == to {
            return true;
        }
        let mut visited = HashSet::new();
        let mut queue = VecDeque::new();
        visited.insert(from.to_string());
        queue.push_back(from.to_string());

        while let Some(current) = queue.pop_front() {
            if let Some(rels) = self.outbound.get(&current) {
                for (_, target, _) in rels {
                    if target == to {
                        return true;
                    }
                    if visited.insert(target.clone()) {
                        queue.push_back(target.clone());
                    }
                }
            }
        }
        false
    }

    /// All types transitively reachable from `from_type` via outbound relationships.
    pub fn relationship_closure(&self, from_type: &str) -> HashSet<String> {
        let mut visited = HashSet::new();
        let mut queue = VecDeque::new();
        visited.insert(from_type.to_string());
        queue.push_back(from_type.to_string());

        while let Some(current) = queue.pop_front() {
            if let Some(rels) = self.outbound.get(&current) {
                for (_, target, _) in rels {
                    if visited.insert(target.clone()) {
                        queue.push_back(target.clone());
                    }
                }
            }
        }
        visited.remove(from_type);
        visited
    }

    /// Detect types referenced in relationships but not registered.
    pub fn unregistered_types(&self) -> Vec<String> {
        let mut missing = Vec::new();
        for rel in &self.relationships {
            if !self.types.contains(&rel.from_type) {
                missing.push(rel.from_type.clone());
            }
            if !self.types.contains(&rel.to_type) {
                missing.push(rel.to_type.clone());
            }
        }
        missing.sort();
        missing.dedup();
        missing
    }

    /// Detect cycles in the type relationship graph.
    pub fn has_cycle(&self) -> bool {
        let mut visited = HashSet::new();
        let mut in_stack = HashSet::new();

        for t in &self.types {
            if self.dfs_cycle(t, &mut visited, &mut in_stack) {
                return true;
            }
        }
        false
    }

    fn dfs_cycle(&self, node: &str, visited: &mut HashSet<String>, in_stack: &mut HashSet<String>) -> bool {
        if in_stack.contains(node) {
            return true;
        }
        if visited.contains(node) {
            return false;
        }
        visited.insert(node.to_string());
        in_stack.insert(node.to_string());

        if let Some(rels) = self.outbound.get(node) {
            for (_, target, _) in rels {
                if self.dfs_cycle(target, visited, in_stack) {
                    return true;
                }
            }
        }

        in_stack.remove(node);
        false
    }

    /// Get required foreign keys for a type (fields that must be present on creation).
    pub fn required_foreign_keys(&self, resource_type: &str) -> Vec<(&str, &str)> {
        self.outbound_relations(resource_type)
            .iter()
            .filter(|r| r.required)
            .map(|r| (r.foreign_key.as_str(), r.to_type.as_str()))
            .collect()
    }

    pub fn type_count(&self) -> usize {
        self.types.len()
    }

    pub fn relationship_count(&self) -> usize {
        self.relationships.len()
    }
}

impl Default for SchemaGraph {
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

    fn loan_schema() -> SchemaGraph {
        let mut sg = SchemaGraph::new();
        sg.register_type("applicant");
        sg.register_type("loan");
        sg.register_type("collateral");
        sg.register_type("disbursement");

        sg.add_relationship(RelationshipDeclaration {
            from_type: "loan".into(), to_type: "applicant".into(),
            relation: "belongs_to".into(), cardinality: Cardinality::ManyToOne,
            required: true, foreign_key: "applicant_id".into(),
        });
        sg.add_relationship(RelationshipDeclaration {
            from_type: "loan".into(), to_type: "collateral".into(),
            relation: "secured_by".into(), cardinality: Cardinality::ManyToOne,
            required: false, foreign_key: "collateral_id".into(),
        });
        sg.add_relationship(RelationshipDeclaration {
            from_type: "loan".into(), to_type: "disbursement".into(),
            relation: "funds".into(), cardinality: Cardinality::OneToMany,
            required: false, foreign_key: "loan_id".into(),
        });
        sg
    }

    #[test]
    fn test_reachability() {
        let sg = loan_schema();
        assert!(sg.reachable("loan", "applicant"));
        assert!(sg.reachable("loan", "collateral"));
        assert!(sg.reachable("loan", "disbursement"));
        assert!(!sg.reachable("applicant", "loan")); // No reverse edge
        assert!(sg.reachable("loan", "loan")); // Self
    }

    #[test]
    fn test_relationship_closure() {
        let sg = loan_schema();
        let closure = sg.relationship_closure("loan");
        assert!(closure.contains("applicant"));
        assert!(closure.contains("collateral"));
        assert!(closure.contains("disbursement"));
        assert!(!closure.contains("loan"));
        assert_eq!(closure.len(), 3);
    }

    #[test]
    fn test_empty_closure() {
        let sg = loan_schema();
        let closure = sg.relationship_closure("applicant");
        assert!(closure.is_empty()); // Applicant has no outbound
    }

    #[test]
    fn test_outbound_inbound() {
        let sg = loan_schema();
        assert_eq!(sg.outbound_relations("loan").len(), 3);
        assert_eq!(sg.inbound_relations("applicant").len(), 1);
        assert_eq!(sg.inbound_relations("loan").len(), 0);
    }

    #[test]
    fn test_no_cycle() {
        let sg = loan_schema();
        assert!(!sg.has_cycle());
    }

    #[test]
    fn test_cycle_detection() {
        let mut sg = SchemaGraph::new();
        sg.register_type("A");
        sg.register_type("B");
        sg.add_relationship(RelationshipDeclaration {
            from_type: "A".into(), to_type: "B".into(),
            relation: "refs".into(), cardinality: Cardinality::OneToOne,
            required: false, foreign_key: "b_id".into(),
        });
        sg.add_relationship(RelationshipDeclaration {
            from_type: "B".into(), to_type: "A".into(),
            relation: "refs".into(), cardinality: Cardinality::OneToOne,
            required: false, foreign_key: "a_id".into(),
        });
        assert!(sg.has_cycle());
    }

    #[test]
    fn test_required_foreign_keys() {
        let sg = loan_schema();
        let required = sg.required_foreign_keys("loan");
        assert_eq!(required.len(), 1);
        assert_eq!(required[0], ("applicant_id", "applicant"));
    }

    #[test]
    fn test_unregistered_types() {
        let mut sg = SchemaGraph::new();
        sg.register_type("loan");
        sg.add_relationship(RelationshipDeclaration {
            from_type: "loan".into(), to_type: "mystery".into(),
            relation: "refs".into(), cardinality: Cardinality::OneToOne,
            required: false, foreign_key: "m_id".into(),
        });
        assert_eq!(sg.unregistered_types(), vec!["mystery"]);
    }

    #[test]
    fn test_transitive_reachability() {
        let mut sg = SchemaGraph::new();
        sg.register_type("A");
        sg.register_type("B");
        sg.register_type("C");
        sg.add_relationship(RelationshipDeclaration {
            from_type: "A".into(), to_type: "B".into(),
            relation: "r1".into(), cardinality: Cardinality::OneToOne,
            required: false, foreign_key: "b_id".into(),
        });
        sg.add_relationship(RelationshipDeclaration {
            from_type: "B".into(), to_type: "C".into(),
            relation: "r2".into(), cardinality: Cardinality::OneToOne,
            required: false, foreign_key: "c_id".into(),
        });
        assert!(sg.reachable("A", "C")); // Transitive
        assert!(!sg.reachable("C", "A")); // No reverse
    }
}
