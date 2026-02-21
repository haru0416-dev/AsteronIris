use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DagNode {
    pub id: String,
}

impl DagNode {
    pub fn new(id: impl Into<String>) -> Self {
        Self { id: id.into() }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DagEdge {
    pub from: String,
    pub to: String,
}

impl DagEdge {
    pub fn new(from: impl Into<String>, to: impl Into<String>) -> Self {
        Self {
            from: from.into(),
            to: to.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DagContract {
    pub nodes: Vec<DagNode>,
    pub edges: Vec<DagEdge>,
}

impl DagContract {
    pub fn new(nodes: Vec<DagNode>, edges: Vec<DagEdge>) -> Self {
        Self { nodes, edges }
    }

    pub fn validate(&self) -> Result<()> {
        let node_ids = self.validate_nodes()?;
        let adjacency = self.validate_edges(&node_ids)?;
        validate_cycle_free(&node_ids, &adjacency)
    }

    pub fn topological_sort(&self) -> Result<Vec<String>> {
        self.validate()?;

        let mut in_degree = BTreeMap::new();
        let mut adjacency = BTreeMap::new();
        for node in &self.nodes {
            in_degree.insert(node.id.clone(), 0_usize);
            adjacency.insert(node.id.clone(), Vec::new());
        }

        for edge in &self.edges {
            if let Some(degree) = in_degree.get_mut(&edge.to) {
                *degree += 1;
            }
            if let Some(neighbors) = adjacency.get_mut(&edge.from) {
                neighbors.push(edge.to.clone());
            }
        }

        for neighbors in adjacency.values_mut() {
            neighbors.sort_unstable();
        }

        let mut queue = in_degree
            .iter()
            .filter_map(|(node_id, degree)| {
                if *degree == 0 {
                    Some(node_id.clone())
                } else {
                    None
                }
            })
            .collect::<BTreeSet<_>>();

        let mut sorted = Vec::new();
        while let Some(node_id) = queue.pop_first() {
            sorted.push(node_id.clone());

            if let Some(neighbors) = adjacency.get(&node_id) {
                for neighbor in neighbors {
                    if let Some(degree) = in_degree.get_mut(neighbor) {
                        *degree -= 1;
                        if *degree == 0 {
                            queue.insert(neighbor.clone());
                        }
                    }
                }
            }
        }

        if sorted.len() != self.nodes.len() {
            bail!("cycle detected while sorting DAG");
        }

        Ok(sorted)
    }

    fn validate_nodes(&self) -> Result<BTreeSet<String>> {
        let mut node_ids = BTreeSet::new();

        for node in &self.nodes {
            if node.id.trim().is_empty() {
                bail!("node id cannot be empty");
            }

            if !node_ids.insert(node.id.clone()) {
                bail!("duplicate node id: {}", node.id);
            }
        }

        Ok(node_ids)
    }

    fn validate_edges(&self, node_ids: &BTreeSet<String>) -> Result<BTreeMap<String, Vec<String>>> {
        let mut adjacency = BTreeMap::new();
        let mut seen_edges = BTreeSet::new();

        for node_id in node_ids {
            adjacency.entry(node_id.clone()).or_insert_with(Vec::new);
        }

        for edge in &self.edges {
            let from_known = node_ids.contains(&edge.from);
            let to_known = node_ids.contains(&edge.to);
            if !from_known || !to_known {
                let known = node_ids.iter().cloned().collect::<Vec<_>>().join(", ");
                bail!(
                    "edge references unknown node: {} -> {} (known nodes: [{}])",
                    edge.from,
                    edge.to,
                    known
                );
            }

            if !seen_edges.insert((edge.from.clone(), edge.to.clone())) {
                bail!("duplicate edge: {} -> {}", edge.from, edge.to);
            }

            adjacency
                .entry(edge.from.clone())
                .or_insert_with(Vec::new)
                .push(edge.to.clone());
        }

        for neighbors in adjacency.values_mut() {
            neighbors.sort_unstable();
            neighbors.dedup();
        }

        Ok(adjacency)
    }
}

fn validate_cycle_free(
    node_ids: &BTreeSet<String>,
    adjacency: &BTreeMap<String, Vec<String>>,
) -> Result<()> {
    let mut states = BTreeMap::new();
    let mut stack = Vec::new();

    for node_id in node_ids {
        if states.contains_key(node_id) {
            continue;
        }

        if let Some(path) = detect_cycle(node_id, adjacency, &mut states, &mut stack) {
            bail!("cycle detected: {}", path.join(" -> "));
        }
    }

    Ok(())
}

fn detect_cycle(
    node_id: &str,
    adjacency: &BTreeMap<String, Vec<String>>,
    states: &mut BTreeMap<String, NodeState>,
    stack: &mut Vec<String>,
) -> Option<Vec<String>> {
    states.insert(node_id.to_string(), NodeState::Visiting);
    stack.push(node_id.to_string());

    if let Some(neighbors) = adjacency.get(node_id) {
        for neighbor in neighbors {
            match states.get(neighbor.as_str()) {
                Some(NodeState::Visiting) => {
                    if let Some(index) = stack.iter().position(|entry| entry == neighbor) {
                        let mut cycle = stack[index..].to_vec();
                        cycle.push(neighbor.clone());
                        return Some(cycle);
                    }
                    return Some(vec![neighbor.clone(), neighbor.clone()]);
                }
                Some(NodeState::Visited) => {}
                None => {
                    if let Some(path) = detect_cycle(neighbor, adjacency, states, stack) {
                        return Some(path);
                    }
                }
            }
        }
    }

    stack.pop();
    states.insert(node_id.to_string(), NodeState::Visited);
    None
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NodeState {
    Visiting,
    Visited,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dag_contract_rejects_duplicate_edge() {
        let dag = DagContract::new(
            vec![DagNode::new("plan"), DagNode::new("verify")],
            vec![
                DagEdge::new("plan", "verify"),
                DagEdge::new("plan", "verify"),
            ],
        );

        let error = dag.validate().unwrap_err().to_string();
        assert_eq!(error, "duplicate edge: plan -> verify");
    }

    #[test]
    fn dag_contract_rejects_unknown_edge_node() {
        let dag = DagContract::new(
            vec![DagNode::new("plan")],
            vec![DagEdge::new("plan", "verify")],
        );

        let error = dag.validate().unwrap_err().to_string();
        assert_eq!(
            error,
            "edge references unknown node: plan -> verify (known nodes: [plan])"
        );
    }

    #[test]
    fn dag_contract_accepts_valid_dag() {
        let dag = DagContract::new(
            vec![
                DagNode::new("plan"),
                DagNode::new("build"),
                DagNode::new("verify"),
            ],
            vec![
                DagEdge::new("plan", "build"),
                DagEdge::new("build", "verify"),
            ],
        );

        assert!(dag.validate().is_ok());
    }

    #[test]
    fn dag_contract_rejects_three_node_cycle() {
        let dag = DagContract::new(
            vec![DagNode::new("A"), DagNode::new("B"), DagNode::new("C")],
            vec![
                DagEdge::new("A", "B"),
                DagEdge::new("B", "C"),
                DagEdge::new("C", "A"),
            ],
        );

        let error = dag.validate().unwrap_err().to_string();
        assert_eq!(error, "cycle detected: A -> B -> C -> A");
    }

    #[test]
    fn dag_contract_rejects_self_cycle() {
        let dag = DagContract::new(vec![DagNode::new("A")], vec![DagEdge::new("A", "A")]);

        let error = dag.validate().unwrap_err().to_string();
        assert_eq!(error, "cycle detected: A -> A");
    }

    #[test]
    fn dag_contract_rejects_cycle_in_subgraph() {
        let dag = DagContract::new(
            vec![
                DagNode::new("A"),
                DagNode::new("B"),
                DagNode::new("C"),
                DagNode::new("D"),
            ],
            vec![
                DagEdge::new("A", "B"),
                DagEdge::new("B", "C"),
                DagEdge::new("C", "D"),
                DagEdge::new("D", "B"),
            ],
        );

        let error = dag.validate().unwrap_err().to_string();
        assert_eq!(error, "cycle detected: B -> C -> D -> B");
    }

    #[test]
    fn dag_contract_accepts_empty_graph() {
        let dag = DagContract::new(Vec::new(), Vec::new());

        assert!(dag.validate().is_ok());
    }

    #[test]
    fn dag_contract_accepts_single_node_without_edges() {
        let dag = DagContract::new(vec![DagNode::new("plan")], Vec::new());

        assert!(dag.validate().is_ok());
    }

    #[test]
    fn dag_contract_accepts_disconnected_components() {
        let dag = DagContract::new(
            vec![
                DagNode::new("plan"),
                DagNode::new("build"),
                DagNode::new("lint"),
                DagNode::new("test"),
            ],
            vec![DagEdge::new("plan", "build"), DagEdge::new("lint", "test")],
        );

        assert!(dag.validate().is_ok());
    }

    #[test]
    fn dag_contract_rejects_duplicate_node_ids() {
        let dag = DagContract::new(vec![DagNode::new("plan"), DagNode::new("plan")], Vec::new());

        let error = dag.validate().unwrap_err().to_string();
        assert_eq!(error, "duplicate node id: plan");
    }

    #[test]
    fn dag_contract_rejects_empty_node_id() {
        let dag = DagContract::new(vec![DagNode::new("")], Vec::new());

        let error = dag.validate().unwrap_err().to_string();
        assert_eq!(error, "node id cannot be empty");
    }

    #[test]
    fn topological_sort_linear_chain() {
        let dag = DagContract::new(
            vec![DagNode::new("A"), DagNode::new("B"), DagNode::new("C")],
            vec![DagEdge::new("A", "B"), DagEdge::new("B", "C")],
        );

        let order = dag.topological_sort().unwrap();
        assert_eq!(order, vec!["A", "B", "C"]);
    }

    #[test]
    fn topological_sort_diamond() {
        let dag = DagContract::new(
            vec![
                DagNode::new("A"),
                DagNode::new("B"),
                DagNode::new("C"),
                DagNode::new("D"),
            ],
            vec![
                DagEdge::new("A", "B"),
                DagEdge::new("A", "C"),
                DagEdge::new("B", "D"),
                DagEdge::new("C", "D"),
            ],
        );

        let order = dag.topological_sort().unwrap();
        assert_eq!(order, vec!["A", "B", "C", "D"]);
    }

    #[test]
    fn topological_sort_alphabetical_tie_breaking() {
        let dag = DagContract::new(
            vec![DagNode::new("C"), DagNode::new("A"), DagNode::new("B")],
            Vec::new(),
        );

        let order = dag.topological_sort().unwrap();
        assert_eq!(order, vec!["A", "B", "C"]);
    }

    #[test]
    fn topological_sort_empty_graph() {
        let dag = DagContract::new(Vec::new(), Vec::new());

        let order = dag.topological_sort().unwrap();
        assert!(order.is_empty());
    }

    #[test]
    fn topological_sort_single_node() {
        let dag = DagContract::new(vec![DagNode::new("solo")], Vec::new());

        let order = dag.topological_sort().unwrap();
        assert_eq!(order, vec!["solo"]);
    }

    #[test]
    fn topological_sort_rejects_cycle() {
        let dag = DagContract::new(
            vec![DagNode::new("A"), DagNode::new("B")],
            vec![DagEdge::new("A", "B"), DagEdge::new("B", "A")],
        );

        let error = dag.topological_sort().unwrap_err().to_string();
        assert_eq!(error, "cycle detected: A -> B -> A");
    }
}
