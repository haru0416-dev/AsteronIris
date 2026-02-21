use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Serialize)]
pub(super) struct GenerateContentRequest {
    pub(super) contents: Vec<Content>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) system_instruction: Option<Content>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) tools: Option<Vec<GeminiTool>>,
    #[serde(rename = "generationConfig")]
    pub(super) generation_config: GenerationConfig,
}

#[derive(Debug, Serialize)]
pub(super) struct Content {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) role: Option<String>,
    pub(super) parts: Vec<Part>,
}

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct Part {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) text: Option<String>,
    #[serde(rename = "functionCall", skip_serializing_if = "Option::is_none")]
    pub(super) function_call: Option<GeminiFunctionCall>,
    #[serde(rename = "functionResponse", skip_serializing_if = "Option::is_none")]
    pub(super) function_response: Option<GeminiFunctionResponse>,
    #[serde(rename = "inlineData", skip_serializing_if = "Option::is_none")]
    pub(super) inline_data: Option<GeminiInlineData>,
    #[serde(rename = "fileData", skip_serializing_if = "Option::is_none")]
    pub(super) file_data: Option<GeminiFileData>,
}

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct GeminiInlineData {
    #[serde(rename = "mimeType")]
    pub(super) mime_type: String,
    pub(super) data: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct GeminiFileData {
    #[serde(rename = "mimeType")]
    pub(super) mime_type: String,
    #[serde(rename = "fileUri")]
    pub(super) file_uri: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct GeminiFunctionCall {
    pub(super) name: String,
    #[serde(default)]
    pub(super) args: Value,
    #[serde(default)]
    pub(super) id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct GeminiFunctionResponse {
    pub(super) name: String,
    pub(super) response: Value,
}

#[derive(Debug, Serialize)]
pub(super) struct GeminiTool {
    #[serde(rename = "function_declarations")]
    pub(super) function_declarations: Vec<GeminiFunctionDeclaration>,
}

#[derive(Debug, Serialize)]
pub(super) struct GeminiFunctionDeclaration {
    pub(super) name: String,
    pub(super) description: String,
    pub(super) parameters: Value,
}

#[derive(Debug, Serialize)]
pub(super) struct GenerationConfig {
    pub(super) temperature: f64,
    #[serde(rename = "maxOutputTokens")]
    pub(super) max_output_tokens: u32,
}

#[derive(Debug, Deserialize)]
pub(super) struct GenerateContentResponse {
    pub(super) candidates: Option<Vec<Candidate>>,
    pub(super) error: Option<ApiError>,
    #[serde(rename = "usageMetadata")]
    pub(super) usage_metadata: Option<UsageMetadata>,
    #[serde(rename = "modelVersion")]
    pub(super) model_version: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct UsageMetadata {
    #[serde(rename = "promptTokenCount")]
    pub(super) prompt_token_count: u64,
    #[serde(rename = "candidatesTokenCount")]
    pub(super) candidates_token_count: u64,
}

#[derive(Debug, Deserialize)]
pub(super) struct Candidate {
    pub(super) content: CandidateContent,
    #[serde(rename = "finishReason")]
    pub(super) finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct CandidateContent {
    pub(super) parts: Vec<ResponsePart>,
}

#[derive(Debug, Deserialize)]
pub(super) struct ResponsePart {
    pub(super) text: Option<String>,
    #[serde(rename = "functionCall")]
    pub(super) function_call: Option<GeminiFunctionCall>,
}

#[derive(Debug, Deserialize)]
pub(super) struct ApiError {
    pub(super) message: String,
}
