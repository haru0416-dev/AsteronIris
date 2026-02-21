use crate::intelligence::planner::{
    DagContract, DagEdge, DagNode, Plan, PlanStep, StepAction, StepStatus,
};
use anyhow::{Context, Result, bail};
use serde::Deserialize;

pub struct PlanParser;

#[derive(Deserialize)]
struct RawPlan {
    id: String,
    description: String,
    steps: Vec<RawStep>,
}

#[derive(Deserialize)]
struct RawStep {
    id: String,
    description: String,
    action: StepAction,
    #[serde(default)]
    depends_on: Vec<String>,
}

impl PlanParser {
    pub fn schema_prompt() -> &'static str {
        concat!(
            "When creating a plan, respond with a JSON object in this exact format:\n",
            "{\n",
            "  \"id\": \"<unique-id>\",\n",
            "  \"description\": \"<plan description>\",\n",
            "  \"steps\": [\n",
            "    {\n",
            "      \"id\": \"<step-id>\",\n",
            "      \"description\": \"<what this step does>\",\n",
            "      \"action\": <action>,\n",
            "      \"depends_on\": [\"<step-ids this depends on>\"]\n",
            "    }\n",
            "  ]\n",
            "}\n\n",
            "Action types:\n",
            "- Tool call: { \"kind\": \"tool_call\", \"tool_name\": \"<name>\", \"args\": { ... } }\n",
            "- Prompt: { \"kind\": \"prompt\", \"text\": \"<instruction>\" }\n",
            "- Checkpoint: { \"kind\": \"checkpoint\", \"label\": \"<label>\" }\n\n",
            "Steps with no dependencies use \"depends_on\": [].\n",
            "Wrap the JSON in a ```json code fence.",
        )
    }

    pub fn parse(json_str: &str) -> Result<Plan> {
        let raw: RawPlan = serde_json::from_str(json_str).context("invalid plan JSON")?;

        if raw.steps.is_empty() {
            bail!("plan must have at least one step");
        }

        let steps: Vec<PlanStep> = raw
            .steps
            .iter()
            .map(|rs| PlanStep {
                id: rs.id.clone(),
                description: rs.description.clone(),
                action: rs.action.clone(),
                status: StepStatus::Pending,
                depends_on: rs.depends_on.clone(),
                output: None,
                error: None,
            })
            .collect();

        let nodes: Vec<DagNode> = raw.steps.iter().map(|rs| DagNode::new(&rs.id)).collect();

        let mut edges = Vec::new();
        for rs in &raw.steps {
            for dep in &rs.depends_on {
                edges.push(DagEdge::new(dep, &rs.id));
            }
        }

        let dag = DagContract::new(nodes, edges);
        Plan::new(raw.id, raw.description, steps, dag)
    }

    pub fn extract_json(text: &str) -> Option<&str> {
        if let Some(start) = text.find("```json") {
            let json_start = start + "```json".len();
            let rest = &text[json_start..];
            if let Some(end) = rest.find("```") {
                let candidate = rest[..end].trim();
                if !candidate.is_empty() {
                    return Some(candidate);
                }
            }
        }

        if let Some(start) = text.find("```\n{") {
            let json_start = start + "```\n".len();
            let rest = &text[json_start..];
            if let Some(end) = rest.find("```") {
                let candidate = rest[..end].trim();
                if !candidate.is_empty() {
                    return Some(candidate);
                }
            }
        }

        let open = text.find('{')?;
        let close = text.rfind('}')?;
        if close > open {
            return Some(&text[open..=close]);
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn valid_plan_json() -> String {
        json!({
            "id": "plan-1",
            "description": "test plan",
            "steps": [
                {
                    "id": "A",
                    "description": "run tests",
                    "action": { "kind": "tool_call", "tool_name": "shell", "args": {"command": "cargo test"} },
                    "depends_on": []
                },
                {
                    "id": "B",
                    "description": "build",
                    "action": { "kind": "tool_call", "tool_name": "shell", "args": {"command": "cargo build"} },
                    "depends_on": ["A"]
                }
            ]
        })
        .to_string()
    }

    #[test]
    fn parse_valid_plan_with_tool_calls() {
        let plan = PlanParser::parse(&valid_plan_json()).unwrap();
        assert_eq!(plan.id, "plan-1");
        assert_eq!(plan.steps.len(), 2);
        assert_eq!(plan.steps[0].id, "A");
        assert_eq!(plan.steps[1].id, "B");
        assert!(matches!(plan.steps[0].status, StepStatus::Pending));
    }

    #[test]
    fn parse_plan_with_mixed_action_types() {
        let input = json!({
            "id": "mix",
            "description": "mixed actions",
            "steps": [
                {
                    "id": "s1",
                    "description": "tool",
                    "action": { "kind": "tool_call", "tool_name": "echo", "args": {} },
                    "depends_on": []
                },
                {
                    "id": "s2",
                    "description": "prompt",
                    "action": { "kind": "prompt", "text": "confirm?" },
                    "depends_on": ["s1"]
                },
                {
                    "id": "s3",
                    "description": "gate",
                    "action": { "kind": "checkpoint", "label": "pre-deploy" },
                    "depends_on": ["s2"]
                }
            ]
        })
        .to_string();

        let plan = PlanParser::parse(&input).unwrap();
        assert_eq!(plan.steps.len(), 3);

        assert!(matches!(plan.steps[0].action, StepAction::ToolCall { .. }));
        assert!(matches!(plan.steps[1].action, StepAction::Prompt { .. }));
        assert!(matches!(
            plan.steps[2].action,
            StepAction::Checkpoint { .. }
        ));
    }

    #[test]
    fn parse_rejects_cyclic_dependencies() {
        let input = json!({
            "id": "cyc",
            "description": "cycle",
            "steps": [
                {
                    "id": "A",
                    "description": "a",
                    "action": { "kind": "checkpoint", "label": "a" },
                    "depends_on": ["B"]
                },
                {
                    "id": "B",
                    "description": "b",
                    "action": { "kind": "checkpoint", "label": "b" },
                    "depends_on": ["A"]
                }
            ]
        })
        .to_string();

        let err = PlanParser::parse(&input).unwrap_err().to_string();
        assert!(err.contains("cycle"), "expected cycle error, got: {err}");
    }

    #[test]
    fn parse_rejects_missing_dependency() {
        let input = json!({
            "id": "miss",
            "description": "missing dep",
            "steps": [
                {
                    "id": "A",
                    "description": "a",
                    "action": { "kind": "checkpoint", "label": "a" },
                    "depends_on": ["Z"]
                }
            ]
        })
        .to_string();

        let err = PlanParser::parse(&input).unwrap_err().to_string();
        assert!(
            err.contains('Z') || err.contains("unknown"),
            "expected reference to Z, got: {err}"
        );
    }

    #[test]
    fn parse_rejects_duplicate_step_ids() {
        let input = json!({
            "id": "dup",
            "description": "duplicates",
            "steps": [
                {
                    "id": "A",
                    "description": "first",
                    "action": { "kind": "checkpoint", "label": "1" },
                    "depends_on": []
                },
                {
                    "id": "A",
                    "description": "second",
                    "action": { "kind": "checkpoint", "label": "2" },
                    "depends_on": []
                }
            ]
        })
        .to_string();

        let err = PlanParser::parse(&input).unwrap_err().to_string();
        assert!(
            err.contains("duplicate"),
            "expected duplicate error, got: {err}"
        );
    }

    #[test]
    fn parse_rejects_invalid_json() {
        let err = PlanParser::parse("not json at all")
            .unwrap_err()
            .to_string();
        assert!(err.contains("invalid plan JSON"));
    }

    #[test]
    fn parse_rejects_empty_steps() {
        let input = json!({
            "id": "empty",
            "description": "no steps",
            "steps": []
        })
        .to_string();

        let err = PlanParser::parse(&input).unwrap_err().to_string();
        assert!(err.contains("at least one step"));
    }

    #[test]
    fn extract_json_from_markdown_fences() {
        let text = "Here is the plan:\n```json\n{\"id\": \"test\"}\n```\nDone.";
        let extracted = PlanParser::extract_json(text).unwrap();
        assert_eq!(extracted, "{\"id\": \"test\"}");
    }

    #[test]
    fn extract_json_from_raw_text() {
        let text = "The plan is {\"id\": \"raw\"} above.";
        let extracted = PlanParser::extract_json(text).unwrap();
        assert_eq!(extracted, "{\"id\": \"raw\"}");
    }

    #[test]
    fn extract_json_returns_none_for_no_json() {
        assert!(PlanParser::extract_json("just plain text").is_none());
    }

    #[test]
    fn schema_prompt_is_not_empty() {
        let prompt = PlanParser::schema_prompt();
        assert!(!prompt.is_empty());
        assert!(prompt.contains("tool_call"));
        assert!(prompt.contains("checkpoint"));
        assert!(prompt.contains("prompt"));
    }

    #[test]
    fn parse_single_step_no_dependencies() {
        let input = json!({
            "id": "single",
            "description": "one step",
            "steps": [{
                "id": "only",
                "description": "just one",
                "action": { "kind": "tool_call", "tool_name": "echo", "args": {} }
            }]
        })
        .to_string();

        let plan = PlanParser::parse(&input).unwrap();
        assert_eq!(plan.steps.len(), 1);
        assert_eq!(plan.steps[0].id, "only");
        assert!(plan.steps[0].depends_on.is_empty());
    }

    #[test]
    fn parse_and_extract_roundtrip() {
        let raw_json = valid_plan_json();
        let text = format!("Here is your plan:\n```json\n{raw_json}\n```\nLet me know!");
        let extracted = PlanParser::extract_json(&text).unwrap();
        let plan = PlanParser::parse(extracted).unwrap();
        assert_eq!(plan.id, "plan-1");
        assert_eq!(plan.steps.len(), 2);
    }
}
