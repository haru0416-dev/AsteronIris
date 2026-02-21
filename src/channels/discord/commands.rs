use anyhow::Result;
use serde_json::json;

use super::http_client::DiscordHttpClient;
use super::types::InteractionCallbackType;

pub fn build_default_commands() -> Vec<serde_json::Value> {
    vec![json!({
        "name": "ask",
        "description": "Send a message to the AI assistant",
        "type": 1,
        "options": [
            {
                "name": "message",
                "description": "Your message to the assistant",
                "type": 3,
                "required": true
            }
        ]
    })]
}

pub async fn register_commands(
    http: &DiscordHttpClient,
    application_id: &str,
    guild_id: Option<&str>,
    commands: &[serde_json::Value],
) -> Result<()> {
    http.register_commands(application_id, guild_id, commands)
        .await
}

pub fn extract_command_input(data: &serde_json::Value) -> Option<String> {
    let name = data.get("name")?.as_str()?;
    if name != "ask" {
        return None;
    }

    data.get("options")
        .and_then(|opts| opts.as_array())
        .and_then(|opts| {
            opts.iter().find_map(|opt| {
                let opt_name = opt.get("name")?.as_str()?;
                if opt_name == "message" {
                    opt.get("value")?.as_str().map(String::from)
                } else {
                    None
                }
            })
        })
}

pub async fn defer_interaction(
    http: &DiscordHttpClient,
    interaction_id: &str,
    interaction_token: &str,
) -> Result<()> {
    http.create_interaction_response(
        interaction_id,
        interaction_token,
        InteractionCallbackType::DeferredChannelMessageWithSource as u8,
        None,
    )
    .await
}

pub async fn send_interaction_followup(
    http: &DiscordHttpClient,
    application_id: &str,
    interaction_token: &str,
    content: &str,
) -> Result<()> {
    http.edit_original_interaction_response(application_id, interaction_token, content)
        .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_commands_has_ask() {
        let cmds = build_default_commands();
        assert_eq!(cmds.len(), 1);
        assert_eq!(cmds[0]["name"], "ask");
        assert_eq!(cmds[0]["type"], 1);
    }

    #[test]
    fn extract_ask_command_input() {
        let data = json!({
            "name": "ask",
            "options": [
                {"name": "message", "type": 3, "value": "Hello AI"}
            ]
        });
        assert_eq!(extract_command_input(&data), Some("Hello AI".to_string()));
    }

    #[test]
    fn extract_unknown_command_returns_none() {
        let data = json!({"name": "unknown", "options": []});
        assert_eq!(extract_command_input(&data), None);
    }

    #[test]
    fn extract_ask_command_missing_message_option() {
        let data = json!({
            "name": "ask",
            "options": [
                {"name": "other", "type": 3, "value": "stuff"}
            ]
        });
        assert_eq!(extract_command_input(&data), None);
    }

    #[test]
    fn extract_ask_command_empty_options() {
        let data = json!({"name": "ask", "options": []});
        assert_eq!(extract_command_input(&data), None);
    }
}
