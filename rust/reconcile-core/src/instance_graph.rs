//! Instance Graph — live resource relationships as a derived projection.
//!
//! Built incrementally from the event log. Provides graph queries for
//! controllers, invariants, and the API layer. Read-only from the kernel's
//! perspective — mutations happen only via the graph builder post-commit.

use crate::types::ResourceId;
use std::collections::{HashMap, HashSet, VecDeque};

// ---------------------------------------------------------------------------
// Graph primitives
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct GraphNode {
    pub id: ResourceId,
    pub resource_type: String,
    pub state: String,
    pub data: serde_json::Value,
    pub version: u64,
}

#[derive(Debug, Clone)]
pub struct GraphEdge {
    pub from_id: ResourceId,
    pub to_id: ResourceId,
    pub relation: String,
    pub metadata: serde_json::Value,
}

#[derive(Debug, Clone, Copy)]
pub enum AggFn {
    Sum,
    Avg,
    Min,
    Max,
    Count,
}

impl AggFn {
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_uppercase().as_str() {
            "SUM" => Some(Self::Sum),
            "AVG" => Some(Self::Avg),
            "MIN" => Some(Self::Min),
            "MAX" => Some(Self::Max),
            "COUNT" => Some(Self::Count),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Instance graph trait
// ---------------------------------------------------------------------------

pub trait InstanceGraph: Send + Sync {
    fn add_node(&mut self, node: GraphNode);
    fn update_node(&mut self, id: &ResourceId, state: &str, data: &serde_json::Value, version: u64);
    fn add_edge(&mut self, edge: GraphEdge);

    // Query interface
    fn neighbors(&self, id: &ResourceId, edge_type: Option<&str>) -> Vec<GraphNode>;
    fn subgraph(&self, id: &ResourceId, depth: usize) -> (Vec<GraphNode>, Vec<GraphEdge>);
    fn path(&self, from: &ResourceId, to: &ResourceId) -> Option<Vec<GraphEdge>>;
    fn aggregate(&self, id: &ResourceId, edge_type: &str, field: &str, agg_fn: AggFn) -> serde_json::Value;
    fn has_cycle(&self, root: &ResourceId) -> bool;
    fn degree(&self, id: &ResourceId, edge_type: Option<&str>) -> usize;
    fn get_node(&self, id: &ResourceId) -> Option<&GraphNode>;
}

// ---------------------------------------------------------------------------
// In-memory implementation
// ---------------------------------------------------------------------------

pub struct InMemoryInstanceGraph {
    nodes: HashMap<ResourceId, GraphNode>,
    /// Outbound edges: from_id -> [edges]
    outbound: HashMap<ResourceId, Vec<GraphEdge>>,
    /// Inbound edges: to_id -> [edges]
    inbound: HashMap<ResourceId, Vec<GraphEdge>>,
}

impl InMemoryInstanceGraph {
    pub fn new() -> Self {
        Self {
            nodes: HashMap::new(),
            outbound: HashMap::new(),
            inbound: HashMap::new(),
        }
    }
}

impl Default for InMemoryInstanceGraph {
    fn default() -> Self {
        Self::new()
    }
}

impl InstanceGraph for InMemoryInstanceGraph {
    fn add_node(&mut self, node: GraphNode) {
        self.nodes.insert(node.id.clone(), node);
    }

    fn update_node(&mut self, id: &ResourceId, state: &str, data: &serde_json::Value, version: u64) {
        if let Some(node) = self.nodes.get_mut(id) {
            node.state = state.to_string();
            node.data = data.clone();
            node.version = version;
        }
    }

    fn add_edge(&mut self, edge: GraphEdge) {
        self.outbound.entry(edge.from_id.clone()).or_default().push(edge.clone());
        self.inbound.entry(edge.to_id.clone()).or_default().push(edge);
    }

    fn neighbors(&self, id: &ResourceId, edge_type: Option<&str>) -> Vec<GraphNode> {
        let mut result = Vec::new();
        let mut seen = HashSet::new();

        // Outbound neighbors
        if let Some(edges) = self.outbound.get(id) {
            for edge in edges {
                if edge_type.map_or(true, |t| edge.relation == t) {
                    if seen.insert(edge.to_id.clone()) {
                        if let Some(node) = self.nodes.get(&edge.to_id) {
                            result.push(node.clone());
                        }
                    }
                }
            }
        }
        // Inbound neighbors
        if let Some(edges) = self.inbound.get(id) {
            for edge in edges {
                if edge_type.map_or(true, |t| edge.relation == t) {
                    if seen.insert(edge.from_id.clone()) {
                        if let Some(node) = self.nodes.get(&edge.from_id) {
                            result.push(node.clone());
                        }
                    }
                }
            }
        }
        result
    }

    fn subgraph(&self, id: &ResourceId, depth: usize) -> (Vec<GraphNode>, Vec<GraphEdge>) {
        let mut visited_nodes = HashSet::new();
        let mut collected_edges = Vec::new();
        let mut queue: VecDeque<(ResourceId, usize)> = VecDeque::new();

        visited_nodes.insert(id.clone());
        queue.push_back((id.clone(), 0));

        while let Some((current, d)) = queue.pop_front() {
            if d >= depth {
                continue;
            }
            // Outbound
            if let Some(edges) = self.outbound.get(&current) {
                for edge in edges {
                    collected_edges.push(edge.clone());
                    if visited_nodes.insert(edge.to_id.clone()) {
                        queue.push_back((edge.to_id.clone(), d + 1));
                    }
                }
            }
            // Inbound
            if let Some(edges) = self.inbound.get(&current) {
                for edge in edges {
                    collected_edges.push(edge.clone());
                    if visited_nodes.insert(edge.from_id.clone()) {
                        queue.push_back((edge.from_id.clone(), d + 1));
                    }
                }
            }
        }

        let nodes: Vec<GraphNode> = visited_nodes
            .iter()
            .filter_map(|id| self.nodes.get(id).cloned())
            .collect();
        (nodes, collected_edges)
    }

    fn path(&self, from: &ResourceId, to: &ResourceId) -> Option<Vec<GraphEdge>> {
        if from == to {
            return Some(vec![]);
        }
        // BFS for shortest path
        let mut visited = HashSet::new();
        let mut parent: HashMap<ResourceId, (ResourceId, GraphEdge)> = HashMap::new();
        let mut queue = VecDeque::new();

        visited.insert(from.clone());
        queue.push_back(from.clone());

        while let Some(current) = queue.pop_front() {
            if let Some(edges) = self.outbound.get(&current) {
                for edge in edges {
                    if !visited.contains(&edge.to_id) {
                        visited.insert(edge.to_id.clone());
                        parent.insert(edge.to_id.clone(), (current.clone(), edge.clone()));
                        if &edge.to_id == to {
                            // Reconstruct path
                            let mut path = Vec::new();
                            let mut cur = to.clone();
                            while let Some((prev, edge)) = parent.get(&cur) {
                                path.push(edge.clone());
                                cur = prev.clone();
                            }
                            path.reverse();
                            return Some(path);
                        }
                        queue.push_back(edge.to_id.clone());
                    }
                }
            }
        }
        None
    }

    fn aggregate(&self, id: &ResourceId, edge_type: &str, field: &str, agg_fn: AggFn) -> serde_json::Value {
        let neighbors = self.neighbors(id, Some(edge_type));
        let values: Vec<f64> = neighbors
            .iter()
            .filter_map(|n| n.data.get(field).and_then(|v| v.as_f64()))
            .collect();

        if values.is_empty() {
            return serde_json::Value::Null;
        }

        let result = match agg_fn {
            AggFn::Sum => values.iter().sum::<f64>(),
            AggFn::Avg => values.iter().sum::<f64>() / values.len() as f64,
            AggFn::Min => values.iter().cloned().fold(f64::INFINITY, f64::min),
            AggFn::Max => values.iter().cloned().fold(f64::NEG_INFINITY, f64::max),
            AggFn::Count => values.len() as f64,
        };
        serde_json::json!(result)
    }

    fn has_cycle(&self, root: &ResourceId) -> bool {
        let mut visited = HashSet::new();
        let mut in_stack = HashSet::new();
        self.dfs_cycle(root, &mut visited, &mut in_stack)
    }

    fn degree(&self, id: &ResourceId, edge_type: Option<&str>) -> usize {
        let out = self.outbound.get(id)
            .map(|edges| edges.iter().filter(|e| edge_type.map_or(true, |t| e.relation == t)).count())
            .unwrap_or(0);
        let inp = self.inbound.get(id)
            .map(|edges| edges.iter().filter(|e| edge_type.map_or(true, |t| e.relation == t)).count())
            .unwrap_or(0);
        out + inp
    }

    fn get_node(&self, id: &ResourceId) -> Option<&GraphNode> {
        self.nodes.get(id)
    }
}

impl InMemoryInstanceGraph {
    fn dfs_cycle(&self, node: &ResourceId, visited: &mut HashSet<ResourceId>, in_stack: &mut HashSet<ResourceId>) -> bool {
        if in_stack.contains(node) {
            return true;
        }
        if visited.contains(node) {
            return false;
        }
        visited.insert(node.clone());
        in_stack.insert(node.clone());

        if let Some(edges) = self.outbound.get(node) {
            for edge in edges {
                if self.dfs_cycle(&edge.to_id, visited, in_stack) {
                    return true;
                }
            }
        }
        in_stack.remove(node);
        false
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ResourceId;

    fn setup_loan_graph() -> (InMemoryInstanceGraph, ResourceId, ResourceId, ResourceId) {
        let mut graph = InMemoryInstanceGraph::new();

        let applicant_id = ResourceId::new();
        let loan1_id = ResourceId::new();
        let loan2_id = ResourceId::new();

        graph.add_node(GraphNode {
            id: applicant_id.clone(), resource_type: "applicant".into(),
            state: "ACTIVE".into(), data: serde_json::json!({"name": "Acme Corp"}),
            version: 1,
        });
        graph.add_node(GraphNode {
            id: loan1_id.clone(), resource_type: "loan".into(),
            state: "APPROVED".into(), data: serde_json::json!({"amount": 500_000}),
            version: 3,
        });
        graph.add_node(GraphNode {
            id: loan2_id.clone(), resource_type: "loan".into(),
            state: "APPLIED".into(), data: serde_json::json!({"amount": 300_000}),
            version: 1,
        });

        graph.add_edge(GraphEdge {
            from_id: loan1_id.clone(), to_id: applicant_id.clone(),
            relation: "belongs_to".into(), metadata: serde_json::json!({}),
        });
        graph.add_edge(GraphEdge {
            from_id: loan2_id.clone(), to_id: applicant_id.clone(),
            relation: "belongs_to".into(), metadata: serde_json::json!({}),
        });

        (graph, applicant_id, loan1_id, loan2_id)
    }

    #[test]
    fn test_neighbors() {
        let (graph, app_id, loan1_id, _) = setup_loan_graph();
        // Applicant's neighbors (via inbound edges)
        let neighbors = graph.neighbors(&app_id, None);
        assert_eq!(neighbors.len(), 2);

        // Loan1's neighbors (via outbound belongs_to)
        let neighbors = graph.neighbors(&loan1_id, Some("belongs_to"));
        assert_eq!(neighbors.len(), 1);
        assert_eq!(neighbors[0].resource_type, "applicant");
    }

    #[test]
    fn test_aggregate_sum() {
        let (graph, app_id, _, _) = setup_loan_graph();
        // Sum loan amounts via belongs_to edges pointing to applicant
        let total = graph.aggregate(&app_id, "belongs_to", "amount", AggFn::Sum);
        assert_eq!(total.as_f64().unwrap(), 800_000.0);
    }

    #[test]
    fn test_aggregate_count() {
        let (graph, app_id, _, _) = setup_loan_graph();
        let count = graph.aggregate(&app_id, "belongs_to", "amount", AggFn::Count);
        assert_eq!(count.as_f64().unwrap(), 2.0);
    }

    #[test]
    fn test_degree() {
        let (graph, app_id, loan1_id, _) = setup_loan_graph();
        assert_eq!(graph.degree(&app_id, None), 2); // 2 inbound
        assert_eq!(graph.degree(&loan1_id, None), 1); // 1 outbound
        assert_eq!(graph.degree(&loan1_id, Some("belongs_to")), 1);
        assert_eq!(graph.degree(&loan1_id, Some("other")), 0);
    }

    #[test]
    fn test_path() {
        let (graph, app_id, loan1_id, _) = setup_loan_graph();
        let path = graph.path(&loan1_id, &app_id);
        assert!(path.is_some());
        assert_eq!(path.unwrap().len(), 1);
    }

    #[test]
    fn test_no_path() {
        let (graph, app_id, loan1_id, _) = setup_loan_graph();
        // No path from applicant to loan (no outbound edges)
        let path = graph.path(&app_id, &loan1_id);
        assert!(path.is_none());
    }

    #[test]
    fn test_subgraph() {
        let (graph, app_id, _, _) = setup_loan_graph();
        let (nodes, edges) = graph.subgraph(&app_id, 1);
        assert_eq!(nodes.len(), 3); // applicant + 2 loans
        assert_eq!(edges.len(), 2);
    }

    #[test]
    fn test_no_cycle() {
        let (graph, app_id, _, _) = setup_loan_graph();
        assert!(!graph.has_cycle(&app_id));
    }

    #[test]
    fn test_cycle_detection() {
        let mut graph = InMemoryInstanceGraph::new();
        let a = ResourceId::new();
        let b = ResourceId::new();
        graph.add_node(GraphNode { id: a.clone(), resource_type: "t".into(), state: "S".into(), data: serde_json::json!({}), version: 1 });
        graph.add_node(GraphNode { id: b.clone(), resource_type: "t".into(), state: "S".into(), data: serde_json::json!({}), version: 1 });
        graph.add_edge(GraphEdge { from_id: a.clone(), to_id: b.clone(), relation: "r".into(), metadata: serde_json::json!({}) });
        graph.add_edge(GraphEdge { from_id: b.clone(), to_id: a.clone(), relation: "r".into(), metadata: serde_json::json!({}) });
        assert!(graph.has_cycle(&a));
    }

    #[test]
    fn test_update_node() {
        let (mut graph, _, loan1_id, _) = setup_loan_graph();
        graph.update_node(&loan1_id, "DISBURSED", &serde_json::json!({"amount": 500_000}), 4);
        let node = graph.get_node(&loan1_id).unwrap();
        assert_eq!(node.state, "DISBURSED");
        assert_eq!(node.version, 4);
    }
}
