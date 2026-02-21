use crate::intelligence::planner::{Plan, PlanStep, StepAction, StepStatus};
use crate::tools::ToolRegistry;
use crate::tools::middleware::ExecutionContext;
use anyhow::Result;
use async_trait::async_trait;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::Arc;

pub struct PlanExecutor;

pub struct AgentLoopPlanInterface;

pub struct ToolStepRunner {
    registry: Arc<ToolRegistry>,
    ctx: ExecutionContext,
}

impl ToolStepRunner {
    pub fn new(registry: Arc<ToolRegistry>, ctx: ExecutionContext) -> Self {
        Self { registry, ctx }
    }
}

#[async_trait]
impl StepRunner for ToolStepRunner {
    async fn run_step(&self, step: &PlanStep) -> Result<StepOutput> {
        match &step.action {
            StepAction::ToolCall { tool_name, args } => {
                let result = self
                    .registry
                    .execute(tool_name, args.clone(), &self.ctx)
                    .await?;
                Ok(StepOutput {
                    success: result.success,
                    output: result.output,
                    error: result.error,
                })
            }
            StepAction::Prompt { text } => Ok(StepOutput {
                success: true,
                output: format!("[prompt] {text}"),
                error: None,
            }),
            StepAction::Checkpoint { label } => Ok(StepOutput {
                success: true,
                output: format!("[checkpoint] {label}"),
                error: None,
            }),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ExecutionReport {
    pub plan_id: String,
    pub completed_steps: Vec<String>,
    pub failed_steps: Vec<String>,
    pub skipped_steps: Vec<String>,
    pub success: bool,
}

#[derive(Debug, Clone)]
pub struct StepOutput {
    pub success: bool,
    pub output: String,
    pub error: Option<String>,
}

#[async_trait]
pub trait StepRunner: Send + Sync {
    async fn run_step(&self, step: &PlanStep) -> Result<StepOutput>;
}

impl PlanExecutor {
    pub async fn execute(plan: &mut Plan, runner: &dyn StepRunner) -> Result<ExecutionReport> {
        let execution_order = plan.execution_order()?;
        let step_index = plan.step_index();

        let mut dependencies = BTreeMap::new();
        let mut downstream = BTreeMap::new();
        for node in &plan.dag.nodes {
            dependencies.insert(node.id.clone(), BTreeSet::new());
            downstream.insert(node.id.clone(), BTreeSet::new());
        }
        for edge in &plan.dag.edges {
            if let Some(parents) = dependencies.get_mut(&edge.to) {
                parents.insert(edge.from.clone());
            }
            if let Some(children) = downstream.get_mut(&edge.from) {
                children.insert(edge.to.clone());
            }
        }

        let mut completed_steps = Vec::new();
        let mut failed_steps = Vec::new();
        let mut skipped_steps = Vec::new();
        let mut skipped_ids = BTreeSet::new();

        for step_id in execution_order {
            if skipped_ids.contains(&step_id) {
                if let Some(index) = step_index.get(&step_id) {
                    plan.steps[*index].status = StepStatus::Skipped;
                    skipped_steps.push(step_id.clone());
                }
                continue;
            }

            let Some(index) = step_index.get(&step_id).copied() else {
                continue;
            };

            plan.steps[index].status = StepStatus::Running;
            let step_snapshot = plan.steps[index].clone();
            let result = runner.run_step(&step_snapshot).await?;

            if result.success {
                plan.steps[index].status = StepStatus::Completed;
                plan.steps[index].output = Some(result.output);
                plan.steps[index].error = None;
                completed_steps.push(step_id);
                continue;
            }

            plan.steps[index].status = StepStatus::Failed;
            plan.steps[index].output = Some(result.output);
            plan.steps[index].error = result.error;
            failed_steps.push(step_id.clone());
            mark_downstream_skipped(&step_id, &downstream, &mut skipped_ids);
        }

        Ok(ExecutionReport {
            plan_id: plan.id.clone(),
            completed_steps,
            failed_steps: failed_steps.clone(),
            skipped_steps,
            success: failed_steps.is_empty(),
        })
    }
}

impl AgentLoopPlanInterface {
    pub async fn execute_plan(
        &self,
        plan: &mut Plan,
        runner: &dyn StepRunner,
    ) -> Result<ExecutionReport> {
        PlanExecutor::execute(plan, runner).await
    }
}

fn mark_downstream_skipped(
    root_id: &str,
    downstream: &BTreeMap<String, BTreeSet<String>>,
    skipped_ids: &mut BTreeSet<String>,
) {
    let mut queue = VecDeque::new();
    queue.push_back(root_id.to_string());

    while let Some(current) = queue.pop_front() {
        if let Some(children) = downstream.get(&current) {
            for child in children {
                if skipped_ids.insert(child.clone()) {
                    queue.push_back(child.clone());
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::intelligence::planner::{
        DagContract, DagEdge, DagNode, Plan, PlanStep, StepAction, StepStatus,
    };
    use anyhow::bail;
    use async_trait::async_trait;
    use serde_json::json;
    use std::sync::Mutex;

    struct MockRunner {
        outcomes: BTreeMap<String, StepOutput>,
        calls: Mutex<Vec<String>>,
        fail_with_error: bool,
    }

    impl MockRunner {
        fn new(outcomes: BTreeMap<String, StepOutput>) -> Self {
            Self {
                outcomes,
                calls: Mutex::new(Vec::new()),
                fail_with_error: false,
            }
        }

        fn with_runner_error(mut self) -> Self {
            self.fail_with_error = true;
            self
        }

        fn called_ids(&self) -> Vec<String> {
            self.calls.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl StepRunner for MockRunner {
        async fn run_step(&self, step: &PlanStep) -> Result<StepOutput> {
            self.calls.lock().unwrap().push(step.id.clone());

            if self.fail_with_error && step.id == "A" {
                bail!("runner transport error");
            }

            if let Some(outcome) = self.outcomes.get(&step.id) {
                return Ok(outcome.clone());
            }

            Ok(StepOutput {
                success: true,
                output: format!("ok:{}", step.id),
                error: None,
            })
        }
    }

    fn step(id: &str) -> PlanStep {
        PlanStep {
            id: id.to_string(),
            description: format!("step {id}"),
            action: StepAction::ToolCall {
                tool_name: "shell".to_string(),
                args: json!({"id":id}),
            },
            status: StepStatus::Pending,
            depends_on: Vec::new(),
            output: None,
            error: None,
        }
    }

    fn make_plan(nodes: Vec<&str>, edges: Vec<(&str, &str)>) -> Plan {
        let dag = DagContract::new(
            nodes.iter().map(|id| DagNode::new(*id)).collect(),
            edges
                .iter()
                .map(|(from, to)| DagEdge::new(*from, *to))
                .collect(),
        );

        Plan::new(
            "plan-1",
            "executor tests",
            nodes.iter().map(|id| step(id)).collect(),
            dag,
        )
        .unwrap()
    }

    #[tokio::test]
    async fn executor_runs_steps_in_topological_order() {
        let mut plan = make_plan(vec!["A", "B", "C"], vec![("A", "B"), ("B", "C")]);
        let runner = MockRunner::new(BTreeMap::new());

        let report = PlanExecutor::execute(&mut plan, &runner).await.unwrap();

        assert_eq!(runner.called_ids(), vec!["A", "B", "C"]);
        assert!(report.success);
        assert_eq!(report.completed_steps, vec!["A", "B", "C"]);
    }

    #[tokio::test]
    async fn executor_marks_failed_step_and_skips_dependents() {
        let mut plan = make_plan(vec!["A", "B", "C"], vec![("A", "B"), ("B", "C")]);

        let mut outcomes = BTreeMap::new();
        outcomes.insert(
            "B".to_string(),
            StepOutput {
                success: false,
                output: "failed output".to_string(),
                error: Some("B failed".to_string()),
            },
        );
        let runner = MockRunner::new(outcomes);

        let report = PlanExecutor::execute(&mut plan, &runner).await.unwrap();

        assert_eq!(runner.called_ids(), vec!["A", "B"]);
        assert_eq!(plan.steps[0].status, StepStatus::Completed);
        assert_eq!(plan.steps[1].status, StepStatus::Failed);
        assert_eq!(plan.steps[2].status, StepStatus::Skipped);
        assert_eq!(report.failed_steps, vec!["B"]);
        assert_eq!(report.skipped_steps, vec!["C"]);
        assert!(!report.success);
    }

    #[tokio::test]
    async fn executor_handles_all_success_case() {
        let mut plan = make_plan(vec!["A", "B"], vec![("A", "B")]);
        let runner = MockRunner::new(BTreeMap::new());

        let report = PlanExecutor::execute(&mut plan, &runner).await.unwrap();

        assert!(
            plan.steps
                .iter()
                .all(|step| step.status == StepStatus::Completed)
        );
        assert!(report.failed_steps.is_empty());
        assert!(report.skipped_steps.is_empty());
        assert!(report.success);
    }

    #[tokio::test]
    async fn executor_handles_first_step_failure() {
        let mut plan = make_plan(vec!["A", "B", "C"], vec![("A", "B"), ("B", "C")]);

        let mut outcomes = BTreeMap::new();
        outcomes.insert(
            "A".to_string(),
            StepOutput {
                success: false,
                output: String::new(),
                error: Some("A failed".to_string()),
            },
        );
        let runner = MockRunner::new(outcomes);

        let report = PlanExecutor::execute(&mut plan, &runner).await.unwrap();

        assert_eq!(runner.called_ids(), vec!["A"]);
        assert_eq!(plan.steps[0].status, StepStatus::Failed);
        assert_eq!(plan.steps[1].status, StepStatus::Skipped);
        assert_eq!(plan.steps[2].status, StepStatus::Skipped);
        assert_eq!(report.skipped_steps, vec!["B", "C"]);
    }

    #[tokio::test]
    async fn executor_independent_branch_continues_after_failure() {
        let mut plan = make_plan(vec!["A", "B", "C", "D"], vec![("A", "B"), ("C", "D")]);

        let mut outcomes = BTreeMap::new();
        outcomes.insert(
            "A".to_string(),
            StepOutput {
                success: false,
                output: String::new(),
                error: Some("A failed".to_string()),
            },
        );
        let runner = MockRunner::new(outcomes);

        let report = PlanExecutor::execute(&mut plan, &runner).await.unwrap();

        assert_eq!(plan.steps[0].status, StepStatus::Failed);
        assert_eq!(plan.steps[1].status, StepStatus::Skipped);
        assert_eq!(plan.steps[2].status, StepStatus::Completed);
        assert_eq!(plan.steps[3].status, StepStatus::Completed);
        assert_eq!(report.failed_steps, vec!["A"]);
        assert_eq!(report.skipped_steps, vec!["B"]);
        assert_eq!(report.completed_steps, vec!["C", "D"]);
    }

    #[tokio::test]
    async fn execution_report_reflects_correct_counts() {
        let mut plan = make_plan(
            vec!["A", "B", "C", "D", "E"],
            vec![("A", "B"), ("A", "C"), ("D", "E")],
        );

        let mut outcomes = BTreeMap::new();
        outcomes.insert(
            "A".to_string(),
            StepOutput {
                success: false,
                output: "x".to_string(),
                error: Some("fail".to_string()),
            },
        );
        let runner = MockRunner::new(outcomes);

        let report = PlanExecutor::execute(&mut plan, &runner).await.unwrap();

        assert_eq!(report.plan_id, "plan-1");
        assert_eq!(report.completed_steps.len(), 2);
        assert_eq!(report.failed_steps.len(), 1);
        assert_eq!(report.skipped_steps.len(), 2);
        assert!(!report.success);
    }

    #[tokio::test]
    async fn executor_preserves_output_and_error_fields() {
        let mut plan = make_plan(vec!["A"], Vec::new());

        let mut outcomes = BTreeMap::new();
        outcomes.insert(
            "A".to_string(),
            StepOutput {
                success: false,
                output: "stderr chunk".to_string(),
                error: Some("exit code 1".to_string()),
            },
        );
        let runner = MockRunner::new(outcomes);

        let _report = PlanExecutor::execute(&mut plan, &runner).await.unwrap();

        assert_eq!(plan.steps[0].status, StepStatus::Failed);
        assert_eq!(plan.steps[0].output.as_deref(), Some("stderr chunk"));
        assert_eq!(plan.steps[0].error.as_deref(), Some("exit code 1"));
    }

    #[tokio::test]
    async fn executor_propagates_runner_errors() {
        let mut plan = make_plan(vec!["A", "B"], vec![("A", "B")]);
        let runner = MockRunner::new(BTreeMap::new()).with_runner_error();

        let error = PlanExecutor::execute(&mut plan, &runner)
            .await
            .unwrap_err()
            .to_string();

        assert_eq!(error, "runner transport error");
        assert_eq!(plan.steps[0].status, StepStatus::Running);
        assert_eq!(plan.steps[1].status, StepStatus::Pending);
    }

    use crate::security::SecurityPolicy;
    use crate::tools::ToolRegistry;
    use crate::tools::middleware::ExecutionContext;
    use crate::tools::traits::{Tool, ToolResult, ToolSpec};

    struct EchoTool;

    #[async_trait]
    impl Tool for EchoTool {
        fn name(&self) -> &str {
            "echo"
        }

        fn description(&self) -> &str {
            "echoes input"
        }

        fn parameters_schema(&self) -> serde_json::Value {
            json!({"type": "object", "properties": {"msg": {"type": "string"}}})
        }

        async fn execute(
            &self,
            args: serde_json::Value,
            _ctx: &ExecutionContext,
        ) -> anyhow::Result<ToolResult> {
            let msg = args
                .get("msg")
                .and_then(|v| v.as_str())
                .unwrap_or("(empty)");
            Ok(ToolResult {
                success: true,
                output: msg.to_string(),
                error: None,
                attachments: Vec::new(),
            })
        }
    }

    fn test_ctx() -> ExecutionContext {
        let security = Arc::new(SecurityPolicy::default());
        ExecutionContext::test_default(security)
    }

    #[tokio::test]
    async fn tool_step_runner_executes_tool_call() {
        let mut registry = ToolRegistry::new(vec![]);
        registry.register(Box::new(EchoTool));
        let runner = ToolStepRunner::new(Arc::new(registry), test_ctx());

        let step = PlanStep {
            id: "s1".to_string(),
            description: "echo test".to_string(),
            action: StepAction::ToolCall {
                tool_name: "echo".to_string(),
                args: json!({"msg": "hello"}),
            },
            status: StepStatus::Pending,
            depends_on: Vec::new(),
            output: None,
            error: None,
        };

        let out = runner.run_step(&step).await.unwrap();
        assert!(out.success);
        assert_eq!(out.output, "hello");
        assert!(out.error.is_none());
    }

    #[tokio::test]
    async fn tool_step_runner_handles_prompt_action() {
        let registry = ToolRegistry::new(vec![]);
        let runner = ToolStepRunner::new(Arc::new(registry), test_ctx());

        let step = PlanStep {
            id: "p1".to_string(),
            description: "ask user".to_string(),
            action: StepAction::Prompt {
                text: "Confirm deployment?".to_string(),
            },
            status: StepStatus::Pending,
            depends_on: Vec::new(),
            output: None,
            error: None,
        };

        let out = runner.run_step(&step).await.unwrap();
        assert!(out.success);
        assert_eq!(out.output, "[prompt] Confirm deployment?");
    }

    #[tokio::test]
    async fn tool_step_runner_handles_checkpoint_action() {
        let registry = ToolRegistry::new(vec![]);
        let runner = ToolStepRunner::new(Arc::new(registry), test_ctx());

        let step = PlanStep {
            id: "c1".to_string(),
            description: "pre-deploy gate".to_string(),
            action: StepAction::Checkpoint {
                label: "pre-deploy".to_string(),
            },
            status: StepStatus::Pending,
            depends_on: Vec::new(),
            output: None,
            error: None,
        };

        let out = runner.run_step(&step).await.unwrap();
        assert!(out.success);
        assert_eq!(out.output, "[checkpoint] pre-deploy");
    }

    #[tokio::test]
    async fn tool_step_runner_reports_tool_not_found() {
        let registry = ToolRegistry::new(vec![]);
        let runner = ToolStepRunner::new(Arc::new(registry), test_ctx());

        let step = PlanStep {
            id: "s1".to_string(),
            description: "missing tool".to_string(),
            action: StepAction::ToolCall {
                tool_name: "nonexistent".to_string(),
                args: json!({}),
            },
            status: StepStatus::Pending,
            depends_on: Vec::new(),
            output: None,
            error: None,
        };

        let out = runner.run_step(&step).await.unwrap();
        assert!(!out.success);
        assert!(out.error.as_deref().unwrap().contains("not found"));
    }

    #[tokio::test]
    async fn tool_step_runner_integrates_with_plan_executor() {
        let mut registry = ToolRegistry::new(vec![]);
        registry.register(Box::new(EchoTool));
        let runner = ToolStepRunner::new(Arc::new(registry), test_ctx());

        let dag = DagContract::new(
            vec![DagNode::new("A"), DagNode::new("B"), DagNode::new("C")],
            vec![DagEdge::new("A", "B"), DagEdge::new("B", "C")],
        );

        let steps = vec![
            PlanStep {
                id: "A".to_string(),
                description: "echo step".to_string(),
                action: StepAction::ToolCall {
                    tool_name: "echo".to_string(),
                    args: json!({"msg": "step-a"}),
                },
                status: StepStatus::Pending,
                depends_on: Vec::new(),
                output: None,
                error: None,
            },
            PlanStep {
                id: "B".to_string(),
                description: "checkpoint".to_string(),
                action: StepAction::Checkpoint {
                    label: "mid".to_string(),
                },
                status: StepStatus::Pending,
                depends_on: vec!["A".to_string()],
                output: None,
                error: None,
            },
            PlanStep {
                id: "C".to_string(),
                description: "prompt".to_string(),
                action: StepAction::Prompt {
                    text: "done?".to_string(),
                },
                status: StepStatus::Pending,
                depends_on: vec!["B".to_string()],
                output: None,
                error: None,
            },
        ];

        let mut plan = Plan::new("integration-test", "mixed actions", steps, dag).unwrap();
        let report = PlanExecutor::execute(&mut plan, &runner).await.unwrap();

        assert!(report.success);
        assert_eq!(report.completed_steps, vec!["A", "B", "C"]);
        assert_eq!(plan.steps[0].output.as_deref(), Some("step-a"));
        assert_eq!(plan.steps[1].output.as_deref(), Some("[checkpoint] mid"));
        assert_eq!(plan.steps[2].output.as_deref(), Some("[prompt] done?"));
    }
}
