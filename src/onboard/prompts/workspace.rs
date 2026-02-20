use anyhow::{Context, Result};
use console::style;
use dialoguer::{Confirm, Input};
use std::fs;
use std::path::PathBuf;

use super::super::domain::validate_non_empty;
use super::super::view::print_bullet;

pub fn setup_workspace() -> Result<(PathBuf, PathBuf)> {
    let home = directories::UserDirs::new()
        .map(|u| u.home_dir().to_path_buf())
        .context("Could not find home directory")?;
    let default_dir = home.join(".asteroniris");

    print_bullet(&t!(
        "onboard.workspace.default_location",
        path = style(default_dir.display()).green()
    ));

    let use_default = Confirm::new()
        .with_prompt(format!("  {}", t!("onboard.workspace.use_default")))
        .default(true)
        .interact()?;

    let asteroniris_dir = if use_default {
        default_dir
    } else {
        let custom = input_workspace_path()?;
        PathBuf::from(custom)
    };

    let workspace_dir = asteroniris_dir.join("workspace");
    let config_path = asteroniris_dir.join("config.toml");

    fs::create_dir_all(&workspace_dir).context("Failed to create workspace directory")?;

    println!(
        "  {} {}",
        style("✓").green().bold(),
        t!(
            "onboard.workspace.confirm",
            path = style(workspace_dir.display()).green()
        )
    );

    Ok((workspace_dir, config_path))
}

#[allow(clippy::too_many_lines)]
fn input_workspace_path() -> Result<String> {
    let custom: String = Input::new()
        .with_prompt(format!("  {}", t!("onboard.workspace.enter_path")))
        .interact_text()?;
    let expanded = shellexpand::tilde(&custom).to_string();
    validate_non_empty("workspace path", &expanded)
}
