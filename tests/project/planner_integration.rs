use std::sync::{Arc, Mutex};

use anyhow::Result;
use asteroniris::core::planner::{PlanExecutor, PlanParser, ToolStepRunner};
use asteroniris::core::tools::ToolRegistry;
use asteroniris::core::tools::middleware::ExecutionContext;
use asteroniris::core::tools::traits::{Tool, ToolResult};
use asteroniris::security::SecurityPolicy;
use async_trait::async_trait;
use serde_json::json;

struct FlakyEchoTool {
    calls: Mutex<u32>,
}

impl FlakyEchoTool {
    fn new() -> Self {
        Self {
            calls: Mutex::new(0),
        }
    }
}

#[async_trait]
impl Tool for FlakyEchoTool {
    fn name(&self) -> &str {
        "flaky_echo"
    }

    fn description(&self) -> &str {
        "fails once then succeeds"
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({"type":"object","properties":{"msg":{"type":"string"}}})
    }

    async fn execute(
        &self,
        args: serde_json::Value,
        _ctx: &ExecutionContext,
    ) -> Result<ToolResult> {
        let mut calls = self.calls.lock().expect("calls lock");
        *calls += 1;
        if *calls == 1 {
            return Ok(ToolResult {
                success: false,
                output: "transient failure".to_string(),
                error: Some("transient".to_string()),
                attachments: Vec::new(),
            });
        }

        let msg = args
            .get("msg")
            .and_then(|v| v.as_str())
            .unwrap_or("ok")
            .to_string();
        Ok(ToolResult {
            success: true,
            output: msg,
            error: None,
            attachments: Vec::new(),
        })
    }
}

fn test_runner() -> ToolStepRunner {
    let security = Arc::new(SecurityPolicy::default());
    let ctx = ExecutionContext::from_security(security);
    let mut registry = ToolRegistry::new(vec![]);
    registry.register(Box::new(FlakyEchoTool::new()));
    ToolStepRunner::new(Arc::new(registry), ctx)
}

async fn execute_with_retry_budget(
    runner: &ToolStepRunner,
    plan_json: &str,
    max_attempts: u32,
) -> (bool, u32, Vec<String>, Vec<String>, Vec<String>) {
    let mut attempts = 0_u32;
    let mut last_completed = Vec::new();
    let mut last_failed = Vec::new();
    let mut last_skipped = Vec::new();

    while attempts < max_attempts.max(1) {
        attempts += 1;
        let mut plan = PlanParser::parse(plan_json).expect("plan parse");
        let report = PlanExecutor::execute(&mut plan, runner)
            .await
            .expect("plan execute");

        last_completed = report.completed_steps;
        last_failed = report.failed_steps;
        last_skipped = report.skipped_steps;

        if report.success {
            return (true, attempts, last_completed, last_failed, last_skipped);
        }
    }

    (false, attempts, last_completed, last_failed, last_skipped)
}

#[tokio::test]
async fn planner_integration_parse_execute_retry_flow() {
    let plan_json = r#"{
      "id":"integration-plan-1",
      "description":"integration",
      "steps":[
        {
          "id":"step_1",
          "description":"run flaky",
          "action":{"kind":"tool_call","tool_name":"flaky_echo","args":{"msg":"ok-after-retry"}},
          "depends_on":[]
        },
        {
          "id":"step_2",
          "description":"verify",
          "action":{"kind":"checkpoint","label":"done"},
          "depends_on":["step_1"]
        }
      ]
    }"#;

    let mut plan = PlanParser::parse(plan_json).expect("plan parse");
    let runner = test_runner();

    let first_report = PlanExecutor::execute(&mut plan, &runner)
        .await
        .expect("first execute");
    assert!(!first_report.success);
    assert_eq!(first_report.failed_steps.len(), 1);
    assert_eq!(first_report.skipped_steps.len(), 1);

    let mut retried_plan = PlanParser::parse(plan_json).expect("retry parse");
    let retry_report = PlanExecutor::execute(&mut retried_plan, &runner)
        .await
        .expect("retry execute");
    assert!(retry_report.success);
    assert_eq!(retry_report.completed_steps, vec!["step_1", "step_2"]);
}

#[tokio::test]
async fn planner_integration_three_step_chain_succeeds_after_retry() {
    let plan_json = r#"{
      "id":"integration-plan-3step",
      "description":"integration chain",
      "steps":[
        {
          "id":"step_1",
          "description":"run flaky",
          "action":{"kind":"tool_call","tool_name":"flaky_echo","args":{"msg":"ok-after-retry"}},
          "depends_on":[]
        },
        {
          "id":"step_2",
          "description":"prompt summarize",
          "action":{"kind":"prompt","text":"summarize"},
          "depends_on":["step_1"]
        },
        {
          "id":"step_3",
          "description":"final checkpoint",
          "action":{"kind":"checkpoint","label":"verified"},
          "depends_on":["step_2"]
        }
      ]
    }"#;

    let runner = test_runner();
    let (success, attempts, completed, failed, skipped) =
        execute_with_retry_budget(&runner, plan_json, 3).await;

    assert!(success);
    assert_eq!(attempts, 2);
    assert_eq!(completed, vec!["step_1", "step_2", "step_3"]);
    assert!(failed.is_empty());
    assert!(skipped.is_empty());
}

#[tokio::test]
async fn planner_integration_retry_budget_stops_at_limit() {
    let plan_json = r#"{
      "id":"integration-plan-budget",
      "description":"integration budget",
      "steps":[
        {
          "id":"step_1",
          "description":"run flaky",
          "action":{"kind":"tool_call","tool_name":"flaky_echo","args":{"msg":"ok-after-retry"}},
          "depends_on":[]
        },
        {
          "id":"step_2",
          "description":"checkpoint",
          "action":{"kind":"checkpoint","label":"done"},
          "depends_on":["step_1"]
        }
      ]
    }"#;

    let runner_one = test_runner();
    let (success_one, attempts_one, _completed_one, failed_one, skipped_one) =
        execute_with_retry_budget(&runner_one, plan_json, 1).await;
    assert!(!success_one);
    assert_eq!(attempts_one, 1);
    assert_eq!(failed_one, vec!["step_1"]);
    assert_eq!(skipped_one, vec!["step_2"]);

    let runner_two = test_runner();
    let (success_two, attempts_two, completed_two, failed_two, skipped_two) =
        execute_with_retry_budget(&runner_two, plan_json, 2).await;
    assert!(success_two);
    assert_eq!(attempts_two, 2);
    assert_eq!(completed_two, vec!["step_1", "step_2"]);
    assert!(failed_two.is_empty());
    assert!(skipped_two.is_empty());
}

#[tokio::test]
async fn planner_integration_zero_retry_budget_clamps_to_single_attempt() {
    let plan_json = r#"{
      "id":"integration-plan-zero-budget",
      "description":"integration zero budget",
      "steps":[
        {
          "id":"step_1",
          "description":"run flaky",
          "action":{"kind":"tool_call","tool_name":"flaky_echo","args":{"msg":"ok-after-retry"}},
          "depends_on":[]
        },
        {
          "id":"step_2",
          "description":"checkpoint",
          "action":{"kind":"checkpoint","label":"done"},
          "depends_on":["step_1"]
        }
      ]
    }"#;

    let runner = test_runner();
    let (success, attempts, _completed, failed, skipped) =
        execute_with_retry_budget(&runner, plan_json, 0).await;

    assert!(!success);
    assert_eq!(attempts, 1);
    assert_eq!(failed, vec!["step_1"]);
    assert_eq!(skipped, vec!["step_2"]);
}
