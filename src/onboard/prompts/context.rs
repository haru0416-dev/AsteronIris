use anyhow::Result;
use dialoguer::{Input, Select};

use crate::ui::style as ui;

use super::super::view::print_bullet;

#[derive(Debug, Clone, Default)]
pub struct ProjectContext {
    pub user_name: String,
    pub timezone: String,
    pub agent_name: String,
    pub communication_style: String,
}

pub fn setup_project_context() -> Result<ProjectContext> {
    print_bullet(&t!("onboard.context.intro"));
    print_bullet(&t!("onboard.context.defaults_hint"));
    println!();

    let user_name: String = Input::new()
        .with_prompt(format!("  {}", t!("onboard.context.name_prompt")))
        .default("User".into())
        .interact_text()?;

    let tz_other = t!("onboard.context.tz_other").to_string();
    let tz_options = vec![
        "US/Eastern (EST/EDT)",
        "US/Central (CST/CDT)",
        "US/Mountain (MST/MDT)",
        "US/Pacific (PST/PDT)",
        "Europe/London (GMT/BST)",
        "Europe/Berlin (CET/CEST)",
        "Asia/Tokyo (JST)",
        "UTC",
        &tz_other,
    ];

    let tz_idx = Select::new()
        .with_prompt(format!("  {}", t!("onboard.context.tz_prompt")))
        .items(&tz_options)
        .default(0)
        .interact()?;

    let timezone = if tz_idx == tz_options.len() - 1 {
        Input::new()
            .with_prompt(format!("  {}", t!("onboard.context.tz_manual_prompt")))
            .default("UTC".into())
            .interact_text()?
    } else {
        tz_options[tz_idx]
            .split('(')
            .next()
            .unwrap_or("UTC")
            .trim()
            .to_string()
    };

    let agent_name: String = Input::new()
        .with_prompt(format!("  {}", t!("onboard.context.agent_name_prompt")))
        .default("AsteronIris".into())
        .interact_text()?;

    let style_options = vec![
        t!("onboard.context.style_direct").to_string(),
        t!("onboard.context.style_friendly").to_string(),
        t!("onboard.context.style_professional").to_string(),
        t!("onboard.context.style_expressive").to_string(),
        t!("onboard.context.style_technical").to_string(),
        t!("onboard.context.style_balanced").to_string(),
        t!("onboard.context.style_custom").to_string(),
    ];

    let style_idx = Select::new()
        .with_prompt(format!("  {}", t!("onboard.context.style_prompt")))
        .items(&style_options)
        .default(1)
        .interact()?;

    let communication_style = match style_idx {
        0 => "Be direct and concise. Skip pleasantries. Get to the point.".to_string(),
        1 => "Be friendly, human, and conversational. Show warmth and empathy while staying efficient. Use natural contractions.".to_string(),
        2 => "Be professional and polished. Stay calm, structured, and respectful. Use occasional tone-setting emojis only when appropriate.".to_string(),
        3 => "Be expressive and playful when appropriate. Use relevant emojis naturally (0-2 max), and keep serious topics emoji-light.".to_string(),
        4 => "Be technical and detailed. Thorough explanations, code-first.".to_string(),
        5 => "Adapt to the situation. Default to warm and clear communication; be concise when needed, thorough when it matters.".to_string(),
        _ => Input::new()
            .with_prompt(format!("  {}", t!("onboard.context.custom_style_prompt")))
            .default(
                "Be warm, natural, and clear. Use occasional relevant emojis (1-2 max) and avoid robotic phrasing.".into(),
            )
            .interact_text()?,
    };

    println!(
        "  {} {}",
        ui::success("✓"),
        t!(
            "onboard.context.confirm",
            name = ui::value(&user_name),
            tz = ui::value(&timezone),
            agent = ui::value(&agent_name),
            style = ui::dim_value(&communication_style)
        )
    );

    Ok(ProjectContext {
        user_name,
        timezone,
        agent_name,
        communication_style,
    })
}
