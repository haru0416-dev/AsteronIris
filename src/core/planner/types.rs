use crate::core::planner::DagContract;
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Skipped,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanStep {
    pub id: String,
    pub description: String,
    pub action: StepAction,
    pub status: StepStatus,
    pub depends_on: Vec<String>,
    pub output: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StepAction {
    ToolCall {
        tool_name: String,
        args: serde_json::Value,
    },
    Prompt {
        text: String,
    },
    Checkpoint {
        label: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Plan {
    pub id: String,
    pub description: String,
    pub steps: Vec<PlanStep>,
    pub dag: DagContract,
}

impl Plan {
    pub fn new(
        id: impl Into<String>,
        description: impl Into<String>,
        steps: Vec<PlanStep>,
        dag: DagContract,
    ) -> Result<Self> {
        dag.validate()?;

        let dag_node_ids = dag
            .nodes
            .iter()
            .map(|node| node.id.clone())
            .collect::<BTreeSet<_>>();

        let mut step_ids = BTreeSet::new();
        for step in &steps {
            if step.id.trim().is_empty() {
                bail!("plan step id cannot be empty");
            }

            if !step_ids.insert(step.id.clone()) {
                bail!("duplicate plan step id: {}", step.id);
            }
        }

        if step_ids != dag_node_ids {
            let missing_steps = dag_node_ids
                .difference(&step_ids)
                .cloned()
                .collect::<Vec<_>>();
            let extra_steps = step_ids
                .difference(&dag_node_ids)
                .cloned()
                .collect::<Vec<_>>();
            bail!(
                "plan step ids must match DAG node ids (missing steps: [{}], extra steps: [{}])",
                missing_steps.join(", "),
                extra_steps.join(", ")
            );
        }

        Ok(Self {
            id: id.into(),
            description: description.into(),
            steps,
            dag,
        })
    }

    pub fn execution_order(&self) -> Result<Vec<String>> {
        self.dag.topological_sort()
    }

    pub fn step_index(&self) -> BTreeMap<String, usize> {
        self.steps
            .iter()
            .enumerate()
            .map(|(index, step)| (step.id.clone(), index))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::planner::{DagEdge, DagNode};
    use serde_json::json;

    fn step(id: &str) -> PlanStep {
        PlanStep {
            id: id.to_string(),
            description: format!("step {id}"),
            action: StepAction::Checkpoint {
                label: format!("checkpoint-{id}"),
            },
            status: StepStatus::Pending,
            depends_on: Vec::new(),
            output: None,
            error: None,
        }
    }

    #[test]
    fn plan_new_validates_dag() {
        let dag = DagContract::new(
            vec![DagNode::new("A"), DagNode::new("B")],
            vec![DagEdge::new("A", "B"), DagEdge::new("B", "A")],
        );

        let error = Plan::new("p", "desc", vec![step("A"), step("B")], dag)
            .unwrap_err()
            .to_string();
        assert_eq!(error, "cycle detected: A -> B -> A");
    }

    #[test]
    fn plan_new_rejects_missing_step_for_dag_node() {
        let dag = DagContract::new(
            vec![DagNode::new("A"), DagNode::new("B")],
            vec![DagEdge::new("A", "B")],
        );

        let error = Plan::new("p", "desc", vec![step("A")], dag)
            .unwrap_err()
            .to_string();
        assert_eq!(
            error,
            "plan step ids must match DAG node ids (missing steps: [B], extra steps: [])"
        );
    }

    #[test]
    fn plan_new_rejects_extra_step_not_in_dag() {
        let dag = DagContract::new(vec![DagNode::new("A")], Vec::new());

        let error = Plan::new("p", "desc", vec![step("A"), step("B")], dag)
            .unwrap_err()
            .to_string();
        assert_eq!(
            error,
            "plan step ids must match DAG node ids (missing steps: [], extra steps: [B])"
        );
    }

    #[test]
    fn plan_new_rejects_duplicate_step_id() {
        let dag = DagContract::new(vec![DagNode::new("A")], Vec::new());

        let error = Plan::new("p", "desc", vec![step("A"), step("A")], dag)
            .unwrap_err()
            .to_string();
        assert_eq!(error, "duplicate plan step id: A");
    }

    #[test]
    fn plan_new_rejects_empty_step_id() {
        let dag = DagContract::new(vec![DagNode::new("A")], Vec::new());

        let mut invalid = step("A");
        invalid.id.clear();
        let error = Plan::new("p", "desc", vec![invalid], dag)
            .unwrap_err()
            .to_string();
        assert_eq!(error, "plan step id cannot be empty");
    }

    #[test]
    fn plan_execution_order_follows_topological_sort() {
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

        let plan = Plan::new(
            "p",
            "desc",
            vec![step("D"), step("C"), step("A"), step("B")],
            dag,
        )
        .unwrap();

        let order = plan.execution_order().unwrap();
        assert_eq!(order, vec!["A", "B", "C", "D"]);
    }

    #[test]
    fn plan_execution_order_alphabetical_for_independent_steps() {
        let dag = DagContract::new(
            vec![DagNode::new("C"), DagNode::new("A"), DagNode::new("B")],
            Vec::new(),
        );
        let plan = Plan::new("p", "desc", vec![step("B"), step("C"), step("A")], dag).unwrap();

        let order = plan.execution_order().unwrap();
        assert_eq!(order, vec!["A", "B", "C"]);
    }

    #[test]
    fn step_action_tool_call_serde_roundtrip() {
        let action = StepAction::ToolCall {
            tool_name: "shell".to_string(),
            args: json!({"command":"cargo test"}),
        };

        let value = serde_json::to_value(&action).unwrap();
        assert_eq!(value["kind"], "tool_call");

        let roundtrip: StepAction = serde_json::from_value(value).unwrap();
        match roundtrip {
            StepAction::ToolCall { tool_name, args } => {
                assert_eq!(tool_name, "shell");
                assert_eq!(args, json!({"command":"cargo test"}));
            }
            _ => panic!("expected tool_call action"),
        }
    }

    #[test]
    fn step_action_prompt_serde_roundtrip() {
        let action = StepAction::Prompt {
            text: "collect diagnostics".to_string(),
        };

        let value = serde_json::to_value(&action).unwrap();
        assert_eq!(value["kind"], "prompt");

        let roundtrip: StepAction = serde_json::from_value(value).unwrap();
        match roundtrip {
            StepAction::Prompt { text } => assert_eq!(text, "collect diagnostics"),
            _ => panic!("expected prompt action"),
        }
    }

    #[test]
    fn step_action_checkpoint_serde_roundtrip() {
        let action = StepAction::Checkpoint {
            label: "after_build".to_string(),
        };

        let value = serde_json::to_value(&action).unwrap();
        assert_eq!(value["kind"], "checkpoint");

        let roundtrip: StepAction = serde_json::from_value(value).unwrap();
        match roundtrip {
            StepAction::Checkpoint { label } => assert_eq!(label, "after_build"),
            _ => panic!("expected checkpoint action"),
        }
    }

    #[test]
    fn step_status_serde_roundtrip() {
        let status = StepStatus::Running;

        let encoded = serde_json::to_string(&status).unwrap();
        assert_eq!(encoded, "\"running\"");

        let decoded: StepStatus = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, StepStatus::Running);
    }

    #[test]
    fn plan_serde_roundtrip() {
        let dag = DagContract::new(
            vec![DagNode::new("A"), DagNode::new("B")],
            vec![DagEdge::new("A", "B")],
        );
        let plan = Plan::new("plan-1", "demo", vec![step("A"), step("B")], dag).unwrap();

        let encoded = serde_json::to_string(&plan).unwrap();
        let decoded: Plan = serde_json::from_str(&encoded).unwrap();

        assert_eq!(decoded.id, "plan-1");
        assert_eq!(decoded.description, "demo");
        assert_eq!(decoded.steps.len(), 2);
        assert_eq!(decoded.dag.edges.len(), 1);
    }
}
