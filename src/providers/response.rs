use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderResponse {
    pub text: String,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub model: Option<String>,
}

impl ProviderResponse {
    pub fn text_only(text: String) -> Self {
        Self {
            text,
            input_tokens: None,
            output_tokens: None,
            model: None,
        }
    }

    pub fn with_usage(text: String, input_tokens: u64, output_tokens: u64) -> Self {
        Self {
            text,
            input_tokens: Some(input_tokens),
            output_tokens: Some(output_tokens),
            model: None,
        }
    }

    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
        self
    }

    pub fn total_tokens(&self) -> Option<u64> {
        match (self.input_tokens, self.output_tokens) {
            (Some(input), Some(output)) => Some(input + output),
            _ => None,
        }
    }
}
