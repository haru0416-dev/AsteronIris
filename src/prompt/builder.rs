use super::engine::TeraEngine;
use tera::Context;

const SYSTEM_PROMPT_TEMPLATE: &str = "\
You are {{ persona_name }}, {{ persona_description }}.

{% if tools_description %}## Available Tools
{{ tools_description }}
{% endif %}\
{% if memory_context %}## Memory Context
{{ memory_context }}
{% endif %}\
{% if custom_instructions %}## Custom Instructions
{{ custom_instructions }}
{% endif %}";

const COMPACTION_TEMPLATE: &str = "\
Summarize the following conversation, preserving key facts, decisions, and context.
Target compression ratio: {{ target_ratio }}%.
Conversation:
{{ messages_text }}";

const CONSOLIDATION_TEMPLATE: &str = "\
Review these related memories and produce a consolidated summary:
{% for memory in memories %}
- {{ memory }}
{% endfor %}
Produce a single, coherent summary that captures all important information.";

const SYSTEM_PROMPT_NAME: &str = "system_prompt";
const COMPACTION_NAME: &str = "compaction";
const CONSOLIDATION_NAME: &str = "consolidation";

/// Ensure the default templates are registered in the engine.
fn ensure_defaults(engine: &mut TeraEngine) -> anyhow::Result<()> {
    // `add_template` overwrites silently, so we always register.
    // This keeps the code simple without needing a "has template" check.
    engine.add_template(SYSTEM_PROMPT_NAME, SYSTEM_PROMPT_TEMPLATE)?;
    engine.add_template(COMPACTION_NAME, COMPACTION_TEMPLATE)?;
    engine.add_template(CONSOLIDATION_NAME, CONSOLIDATION_TEMPLATE)?;
    Ok(())
}

/// Build a system prompt from components.
pub fn build_system_prompt(
    engine: &mut TeraEngine,
    persona_name: &str,
    persona_description: &str,
    tools_description: &str,
    memory_context: Option<&str>,
    custom_instructions: Option<&str>,
) -> anyhow::Result<String> {
    ensure_defaults(engine)?;

    let mut ctx = Context::new();
    ctx.insert("persona_name", persona_name);
    ctx.insert("persona_description", persona_description);
    ctx.insert("tools_description", tools_description);
    ctx.insert("memory_context", &memory_context.unwrap_or_default());
    ctx.insert(
        "custom_instructions",
        &custom_instructions.unwrap_or_default(),
    );

    engine.render(SYSTEM_PROMPT_NAME, &ctx)
}

/// Build a compaction prompt for summarizing conversation history.
pub fn build_compaction_prompt(
    engine: &mut TeraEngine,
    messages_text: &str,
    target_ratio: f64,
) -> anyhow::Result<String> {
    ensure_defaults(engine)?;

    let mut ctx = Context::new();
    ctx.insert("messages_text", messages_text);
    ctx.insert("target_ratio", &target_ratio);

    engine.render(COMPACTION_NAME, &ctx)
}

/// Build a memory consolidation prompt.
pub fn build_consolidation_prompt(
    engine: &mut TeraEngine,
    memories: &[String],
) -> anyhow::Result<String> {
    ensure_defaults(engine)?;

    let mut ctx = Context::new();
    ctx.insert("memories", memories);

    engine.render(CONSOLIDATION_NAME, &ctx)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fresh_engine() -> TeraEngine {
        TeraEngine::new().unwrap()
    }

    #[test]
    fn system_prompt_all_fields() {
        let mut engine = fresh_engine();
        let result = build_system_prompt(
            &mut engine,
            "Iris",
            "a helpful AI assistant",
            "search, calculator",
            Some("User prefers concise answers."),
            Some("Always respond in English."),
        )
        .unwrap();

        assert!(result.contains("You are Iris, a helpful AI assistant."));
        assert!(result.contains("## Available Tools"));
        assert!(result.contains("search, calculator"));
        assert!(result.contains("## Memory Context"));
        assert!(result.contains("User prefers concise answers."));
        assert!(result.contains("## Custom Instructions"));
        assert!(result.contains("Always respond in English."));
    }

    #[test]
    fn system_prompt_no_optional_fields() {
        let mut engine = fresh_engine();
        let result = build_system_prompt(
            &mut engine,
            "Iris",
            "a helpful AI assistant",
            "",
            None,
            None,
        )
        .unwrap();

        assert!(result.contains("You are Iris, a helpful AI assistant."));
        // Empty tools_description is falsy in Tera, so the section should be absent.
        assert!(!result.contains("## Available Tools"));
        assert!(!result.contains("## Memory Context"));
        assert!(!result.contains("## Custom Instructions"));
    }

    #[test]
    fn system_prompt_only_tools() {
        let mut engine = fresh_engine();
        let result = build_system_prompt(
            &mut engine,
            "Bot",
            "an assistant",
            "shell, file_read",
            None,
            None,
        )
        .unwrap();

        assert!(result.contains("## Available Tools"));
        assert!(result.contains("shell, file_read"));
        assert!(!result.contains("## Memory Context"));
        assert!(!result.contains("## Custom Instructions"));
    }

    #[test]
    fn system_prompt_only_memory() {
        let mut engine = fresh_engine();
        let result = build_system_prompt(
            &mut engine,
            "Bot",
            "an assistant",
            "",
            Some("Remember: user is named Alice."),
            None,
        )
        .unwrap();

        assert!(!result.contains("## Available Tools"));
        assert!(result.contains("## Memory Context"));
        assert!(result.contains("Remember: user is named Alice."));
        assert!(!result.contains("## Custom Instructions"));
    }

    #[test]
    fn system_prompt_only_custom_instructions() {
        let mut engine = fresh_engine();
        let result = build_system_prompt(
            &mut engine,
            "Bot",
            "an assistant",
            "",
            None,
            Some("Be brief."),
        )
        .unwrap();

        assert!(!result.contains("## Available Tools"));
        assert!(!result.contains("## Memory Context"));
        assert!(result.contains("## Custom Instructions"));
        assert!(result.contains("Be brief."));
    }

    #[test]
    fn compaction_prompt_renders() {
        let mut engine = fresh_engine();
        let result =
            build_compaction_prompt(&mut engine, "User: Hello\nBot: Hi there!", 0.5).unwrap();

        assert!(result.contains("Summarize the following conversation"));
        assert!(result.contains("0.5%"));
        assert!(result.contains("User: Hello"));
        assert!(result.contains("Bot: Hi there!"));
    }

    #[test]
    fn consolidation_prompt_renders() {
        let mut engine = fresh_engine();
        let memories = vec![
            "User likes Rust.".to_string(),
            "User prefers dark mode.".to_string(),
        ];
        let result = build_consolidation_prompt(&mut engine, &memories).unwrap();

        assert!(result.contains("Review these related memories"));
        assert!(result.contains("- User likes Rust."));
        assert!(result.contains("- User prefers dark mode."));
        assert!(result.contains("consolidated summary"));
    }

    #[test]
    fn consolidation_prompt_empty_memories() {
        let mut engine = fresh_engine();
        let memories: Vec<String> = vec![];
        let result = build_consolidation_prompt(&mut engine, &memories).unwrap();

        assert!(result.contains("Review these related memories"));
        assert!(result.contains("consolidated summary"));
        // No memory bullets should appear.
        assert!(!result.contains("- "));
    }
}
