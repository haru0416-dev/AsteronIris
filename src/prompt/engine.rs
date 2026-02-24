use tera::Tera;

/// Tera-backed template engine for building structured prompts.
pub struct TeraEngine {
    tera: Tera,
}

impl TeraEngine {
    /// Create from a glob pattern pointing at template files.
    pub fn from_glob(glob: &str) -> anyhow::Result<Self> {
        let tera = Tera::new(glob)?;
        Ok(Self { tera })
    }

    /// Create with inline templates (no filesystem).
    pub fn new() -> anyhow::Result<Self> {
        let tera = Tera::default();
        Ok(Self { tera })
    }

    /// Register a template from a string.
    pub fn add_template(&mut self, name: &str, content: &str) -> anyhow::Result<()> {
        self.tera.add_raw_template(name, content)?;
        Ok(())
    }

    /// Render a named template with the given context.
    pub fn render(&self, template_name: &str, context: &tera::Context) -> anyhow::Result<String> {
        let rendered = self.tera.render(template_name, context)?;
        Ok(rendered)
    }

    /// Render a one-off string template (not registered).
    pub fn render_string(&self, template: &str, context: &tera::Context) -> anyhow::Result<String> {
        let rendered = Tera::one_off(template, context, false)?;
        Ok(rendered)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tera::Context;

    #[test]
    fn new_creates_empty_engine() {
        let engine = TeraEngine::new().unwrap();
        // Rendering a non-existent template should fail.
        let ctx = Context::new();
        assert!(engine.render("nonexistent", &ctx).is_err());
    }

    #[test]
    fn add_template_and_render() {
        let mut engine = TeraEngine::new().unwrap();
        engine
            .add_template("greeting", "Hello, {{ name }}!")
            .unwrap();

        let mut ctx = Context::new();
        ctx.insert("name", "World");
        let result = engine.render("greeting", &ctx).unwrap();
        assert_eq!(result, "Hello, World!");
    }

    #[test]
    fn render_missing_variable_fails() {
        let mut engine = TeraEngine::new().unwrap();
        engine
            .add_template("greeting", "Hello, {{ name }}!")
            .unwrap();

        let ctx = Context::new();
        // Tera strict mode: missing variable should error.
        assert!(engine.render("greeting", &ctx).is_err());
    }

    #[test]
    fn render_string_one_off() {
        let engine = TeraEngine::new().unwrap();
        let mut ctx = Context::new();
        ctx.insert("item", "Rust");
        let result = engine.render_string("I love {{ item }}.", &ctx).unwrap();
        assert_eq!(result, "I love Rust.");
    }

    #[test]
    fn add_template_replaces_existing() {
        let mut engine = TeraEngine::new().unwrap();
        engine.add_template("t", "version 1").unwrap();
        engine.add_template("t", "version 2").unwrap();

        let ctx = Context::new();
        let result = engine.render("t", &ctx).unwrap();
        assert_eq!(result, "version 2");
    }

    #[test]
    fn from_glob_invalid_pattern() {
        // A glob that matches nothing is fine for Tera (empty set).
        // But a malformed glob should still succeed with an empty engine.
        let result = TeraEngine::from_glob("/tmp/nonexistent_dir_xyz/**/*.html");
        // Tera::new with no matches returns Ok with empty templates.
        assert!(result.is_ok());
    }

    #[test]
    fn render_with_conditional() {
        let mut engine = TeraEngine::new().unwrap();
        engine
            .add_template("cond", "{% if show_greeting %}Hello!{% endif %}")
            .unwrap();

        let mut ctx = Context::new();
        ctx.insert("show_greeting", &true);
        assert_eq!(engine.render("cond", &ctx).unwrap(), "Hello!");

        let mut ctx2 = Context::new();
        ctx2.insert("show_greeting", &false);
        assert_eq!(engine.render("cond", &ctx2).unwrap(), "");
    }

    #[test]
    fn render_with_loop() {
        let mut engine = TeraEngine::new().unwrap();
        engine
            .add_template("list", "{% for item in items %}- {{ item }}\n{% endfor %}")
            .unwrap();

        let mut ctx = Context::new();
        ctx.insert("items", &vec!["alpha", "beta"]);
        let result = engine.render("list", &ctx).unwrap();
        assert_eq!(result, "- alpha\n- beta\n");
    }
}
