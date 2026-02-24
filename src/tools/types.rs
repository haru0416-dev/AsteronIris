use serde::{Deserialize, Serialize};

/// Result of a tool execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub success: bool,
    pub output: String,
    pub error: Option<String>,
    #[serde(default)]
    pub attachments: Vec<OutputAttachment>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OutputAttachment {
    pub mime_type: String,
    pub filename: Option<String>,
    pub path: Option<String>,
    pub url: Option<String>,
}

impl OutputAttachment {
    pub fn from_path(
        mime_type: impl Into<String>,
        path: impl Into<String>,
        filename: Option<String>,
    ) -> Self {
        Self {
            mime_type: mime_type.into(),
            filename,
            path: Some(path.into()),
            url: None,
        }
    }

    pub fn from_url(
        mime_type: impl Into<String>,
        url: impl Into<String>,
        filename: Option<String>,
    ) -> Self {
        Self {
            mime_type: mime_type.into(),
            filename,
            path: None,
            url: Some(url.into()),
        }
    }
}

/// Description of a tool for the LLM
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn tool_result_serde_defaults_attachments_when_missing() {
        let raw = json!({
            "success": true,
            "output": "ok",
            "error": null
        });
        let parsed: ToolResult = serde_json::from_value(raw).unwrap();
        assert!(parsed.attachments.is_empty());
    }

    #[test]
    fn output_attachment_from_path_sets_path_only() {
        let a = OutputAttachment::from_path("image/png", "/tmp/img.png", Some("img.png".into()));
        assert_eq!(a.path.as_deref(), Some("/tmp/img.png"));
        assert!(a.url.is_none());
    }

    #[test]
    fn output_attachment_from_url_sets_url_only() {
        let a = OutputAttachment::from_url("image/png", "https://example.com/img.png", None);
        assert!(a.path.is_none());
        assert_eq!(a.url.as_deref(), Some("https://example.com/img.png"));
    }
}
