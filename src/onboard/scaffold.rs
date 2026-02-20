use anyhow::Result;
use console::style;
use std::fs;
use std::path::Path;

use super::prompts::ProjectContext;

fn render(template: &str, agent: &str, user: &str, tz: &str, comm_style: &str) -> String {
    template
        .replace("{{agent}}", agent)
        .replace("{{user}}", user)
        .replace("{{tz}}", tz)
        .replace("{{comm_style}}", comm_style)
}

pub fn scaffold_workspace(workspace_dir: &Path, ctx: &ProjectContext) -> Result<()> {
    let agent = if ctx.agent_name.is_empty() {
        "AsteronIris"
    } else {
        &ctx.agent_name
    };
    let user = if ctx.user_name.is_empty() {
        "User"
    } else {
        &ctx.user_name
    };
    let tz = if ctx.timezone.is_empty() {
        "UTC"
    } else {
        &ctx.timezone
    };
    let comm_style = if ctx.communication_style.is_empty() {
        "Be warm, natural, and clear. Use occasional relevant emojis (1-2 max) and avoid robotic phrasing."
    } else {
        &ctx.communication_style
    };

    let r = |tpl: &str| render(tpl, agent, user, tz, comm_style);

    let files: Vec<(&str, String)> = vec![
        ("IDENTITY.md", r(include_str!("templates/IDENTITY.md"))),
        ("AGENTS.md", r(include_str!("templates/AGENTS.md"))),
        ("HEARTBEAT.md", r(include_str!("templates/HEARTBEAT.md"))),
        ("SOUL.md", r(include_str!("templates/SOUL.md"))),
        ("USER.md", r(include_str!("templates/USER.md"))),
        ("TOOLS.md", include_str!("templates/TOOLS.md").to_string()),
        ("BOOTSTRAP.md", r(include_str!("templates/BOOTSTRAP.md"))),
        ("MEMORY.md", include_str!("templates/MEMORY.md").to_string()),
    ];

    // Create subdirectories
    let subdirs = ["sessions", "memory", "state", "cron", "skills"];
    for dir in &subdirs {
        fs::create_dir_all(workspace_dir.join(dir))?;
    }

    let mut created = 0;
    let mut skipped = 0;

    for (filename, content) in &files {
        let path = workspace_dir.join(filename);
        if path.exists() {
            skipped += 1;
        } else {
            fs::write(&path, content)?;
            created += 1;
        }
    }

    println!(
        "  {} {}",
        style("✓").green().bold(),
        t!(
            "onboard.scaffold.created",
            created = created,
            skipped = skipped,
            dirs = subdirs.len()
        )
    );

    // Show workspace tree
    println!();
    println!("  {}", style(t!("onboard.scaffold.layout_header")).dim());
    println!(
        "  {}",
        style(format!("  {}/", workspace_dir.display())).dim()
    );
    for dir in &subdirs {
        println!("  {}", style(format!("  ├── {dir}/")).dim());
    }
    for (i, (filename, _)) in files.iter().enumerate() {
        let prefix = if i == files.len() - 1 {
            "└──"
        } else {
            "├──"
        };
        println!("  {}", style(format!("  {prefix} {filename}")).dim());
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn default_ctx() -> ProjectContext {
        ProjectContext {
            user_name: "TestUser".to_string(),
            timezone: "Asia/Tokyo".to_string(),
            agent_name: "TestAgent".to_string(),
            communication_style: "Be concise.".to_string(),
        }
    }

    #[test]
    fn scaffold_creates_files_and_dirs() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join("workspace");
        std::fs::create_dir_all(&dir).unwrap();
        scaffold_workspace(&dir, &default_ctx()).unwrap();

        // 8 template files
        for file in &[
            "IDENTITY.md",
            "AGENTS.md",
            "HEARTBEAT.md",
            "SOUL.md",
            "USER.md",
            "TOOLS.md",
            "BOOTSTRAP.md",
            "MEMORY.md",
        ] {
            assert!(dir.join(file).exists(), "{file} should exist");
        }

        // 5 subdirectories
        for sub in &["sessions", "memory", "state", "cron", "skills"] {
            assert!(dir.join(sub).is_dir(), "{sub}/ should exist");
        }
    }

    #[test]
    fn scaffold_renders_template_variables() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join("workspace");
        std::fs::create_dir_all(&dir).unwrap();
        scaffold_workspace(&dir, &default_ctx()).unwrap();

        let identity = std::fs::read_to_string(dir.join("IDENTITY.md")).unwrap();
        assert!(
            identity.contains("TestAgent"),
            "IDENTITY.md should contain agent_name"
        );
    }

    #[test]
    fn scaffold_skips_existing_files() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join("workspace");
        std::fs::create_dir_all(&dir).unwrap();

        let sentinel = "DO NOT OVERWRITE";
        std::fs::write(dir.join("IDENTITY.md"), sentinel).unwrap();

        scaffold_workspace(&dir, &default_ctx()).unwrap();

        let content = std::fs::read_to_string(dir.join("IDENTITY.md")).unwrap();
        assert_eq!(content, sentinel, "existing file should not be overwritten");
    }

    #[test]
    fn scaffold_defaults_empty_fields() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join("workspace");
        std::fs::create_dir_all(&dir).unwrap();

        let empty_ctx = ProjectContext::default();
        scaffold_workspace(&dir, &empty_ctx).unwrap();

        let identity = std::fs::read_to_string(dir.join("IDENTITY.md")).unwrap();
        assert!(
            identity.contains("AsteronIris"),
            "empty agent_name should default to AsteronIris"
        );
    }
}
