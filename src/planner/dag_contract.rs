use anyhow::{bail, Result};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DagNode {
    pub id: String,
}

impl DagNode {
    pub fn new(id: impl Into<String>) -> Self {
        Self { id: id.into() }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
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

#[derive(Debug, Clone, PartialEq, Eq)]
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
}
