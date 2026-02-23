use super::status;
use crate::plugins::integrations::{IntegrationCategory, IntegrationEntry};

/// Returns the full catalog of integrations.
#[allow(clippy::too_many_lines)]
pub(super) fn all_integrations() -> Vec<IntegrationEntry> {
    vec![
        // ── Chat Providers ──────────────────────────────────────
        IntegrationEntry {
            name: "Telegram",
            description: "Bot API — long-polling",
            category: IntegrationCategory::Chat,
            status_fn: status::telegram,
        },
        IntegrationEntry {
            name: "Discord",
            description: "Servers, channels & DMs",
            category: IntegrationCategory::Chat,
            status_fn: status::discord,
        },
        IntegrationEntry {
            name: "Slack",
            description: "Workspace apps via Web API",
            category: IntegrationCategory::Chat,
            status_fn: status::slack,
        },
        IntegrationEntry {
            name: "Webhooks",
            description: "HTTP endpoint for triggers",
            category: IntegrationCategory::Chat,
            status_fn: status::webhooks,
        },
        IntegrationEntry {
            name: "WhatsApp",
            description: "QR pairing via web bridge",
            category: IntegrationCategory::Chat,
            status_fn: status::coming_soon,
        },
        IntegrationEntry {
            name: "Signal",
            description: "Privacy-focused via signal-cli",
            category: IntegrationCategory::Chat,
            status_fn: status::coming_soon,
        },
        IntegrationEntry {
            name: "iMessage",
            description: "macOS AppleScript bridge",
            category: IntegrationCategory::Chat,
            status_fn: status::imessage,
        },
        IntegrationEntry {
            name: "Microsoft Teams",
            description: "Enterprise chat support",
            category: IntegrationCategory::Chat,
            status_fn: status::coming_soon,
        },
        IntegrationEntry {
            name: "Matrix",
            description: "Matrix protocol (Element)",
            category: IntegrationCategory::Chat,
            status_fn: status::matrix,
        },
        IntegrationEntry {
            name: "Nostr",
            description: "Decentralized DMs (NIP-04)",
            category: IntegrationCategory::Chat,
            status_fn: status::coming_soon,
        },
        IntegrationEntry {
            name: "WebChat",
            description: "Browser-based chat UI",
            category: IntegrationCategory::Chat,
            status_fn: status::coming_soon,
        },
        IntegrationEntry {
            name: "Nextcloud Talk",
            description: "Self-hosted Nextcloud chat",
            category: IntegrationCategory::Chat,
            status_fn: status::coming_soon,
        },
        IntegrationEntry {
            name: "Zalo",
            description: "Zalo Bot API",
            category: IntegrationCategory::Chat,
            status_fn: status::coming_soon,
        },
        // ── AI Models ───────────────────────────────────────────
        IntegrationEntry {
            name: "OpenRouter",
            description: "200+ models, 1 API key",
            category: IntegrationCategory::AiModel,
            status_fn: status::openrouter,
        },
        IntegrationEntry {
            name: "Anthropic",
            description: "Claude 3.5/4 Sonnet & Opus",
            category: IntegrationCategory::AiModel,
            status_fn: status::anthropic,
        },
        IntegrationEntry {
            name: "OpenAI",
            description: "GPT-4o, GPT-5, o1",
            category: IntegrationCategory::AiModel,
            status_fn: status::openai,
        },
        IntegrationEntry {
            name: "Google",
            description: "Gemini 2.5 Pro/Flash",
            category: IntegrationCategory::AiModel,
            status_fn: status::google,
        },
        IntegrationEntry {
            name: "DeepSeek",
            description: "DeepSeek V3 & R1",
            category: IntegrationCategory::AiModel,
            status_fn: status::deepseek,
        },
        IntegrationEntry {
            name: "xAI",
            description: "Grok 3 & 4",
            category: IntegrationCategory::AiModel,
            status_fn: status::xai,
        },
        IntegrationEntry {
            name: "Mistral",
            description: "Mistral Large & Codestral",
            category: IntegrationCategory::AiModel,
            status_fn: status::mistral_model,
        },
        IntegrationEntry {
            name: "Ollama",
            description: "Local models (Llama, etc.)",
            category: IntegrationCategory::AiModel,
            status_fn: status::ollama,
        },
        IntegrationEntry {
            name: "Perplexity",
            description: "Search-augmented AI",
            category: IntegrationCategory::AiModel,
            status_fn: status::perplexity,
        },
        IntegrationEntry {
            name: "Hugging Face",
            description: "Open-source models",
            category: IntegrationCategory::AiModel,
            status_fn: status::coming_soon,
        },
        IntegrationEntry {
            name: "LM Studio",
            description: "Local model server",
            category: IntegrationCategory::AiModel,
            status_fn: status::coming_soon,
        },
        IntegrationEntry {
            name: "Venice",
            description: "Privacy-first inference (Llama, Opus)",
            category: IntegrationCategory::AiModel,
            status_fn: status::venice,
        },
        IntegrationEntry {
            name: "Vercel AI",
            description: "Vercel AI Gateway",
            category: IntegrationCategory::AiModel,
            status_fn: status::vercel,
        },
        IntegrationEntry {
            name: "Cloudflare AI",
            description: "Cloudflare AI Gateway",
            category: IntegrationCategory::AiModel,
            status_fn: status::cloudflare,
        },
        IntegrationEntry {
            name: "Moonshot",
            description: "Kimi & Kimi Coding",
            category: IntegrationCategory::AiModel,
            status_fn: status::moonshot,
        },
        IntegrationEntry {
            name: "Synthetic",
            description: "Synthetic AI models",
            category: IntegrationCategory::AiModel,
            status_fn: status::synthetic,
        },
        IntegrationEntry {
            name: "OpenCode Zen",
            description: "Code-focused AI models",
            category: IntegrationCategory::AiModel,
            status_fn: status::opencode,
        },
        IntegrationEntry {
            name: "Z.AI",
            description: "Z.AI inference",
            category: IntegrationCategory::AiModel,
            status_fn: status::zai,
        },
        IntegrationEntry {
            name: "GLM",
            description: "ChatGLM / Zhipu models",
            category: IntegrationCategory::AiModel,
            status_fn: status::glm,
        },
        IntegrationEntry {
            name: "MiniMax",
            description: "MiniMax AI models",
            category: IntegrationCategory::AiModel,
            status_fn: status::minimax,
        },
        IntegrationEntry {
            name: "Amazon Bedrock",
            description: "AWS managed model access",
            category: IntegrationCategory::AiModel,
            status_fn: status::bedrock,
        },
        IntegrationEntry {
            name: "Qianfan",
            description: "Baidu AI models",
            category: IntegrationCategory::AiModel,
            status_fn: status::qianfan,
        },
        IntegrationEntry {
            name: "Groq",
            description: "Ultra-fast LPU inference",
            category: IntegrationCategory::AiModel,
            status_fn: status::groq,
        },
        IntegrationEntry {
            name: "Together AI",
            description: "Open-source model hosting",
            category: IntegrationCategory::AiModel,
            status_fn: status::together,
        },
        IntegrationEntry {
            name: "Fireworks AI",
            description: "Fast open-source inference",
            category: IntegrationCategory::AiModel,
            status_fn: status::fireworks,
        },
        IntegrationEntry {
            name: "Cohere",
            description: "Command R+ & embeddings",
            category: IntegrationCategory::AiModel,
            status_fn: status::cohere,
        },
        // ── Productivity ────────────────────────────────────────
        IntegrationEntry {
            name: "GitHub",
            description: "Code, issues, PRs",
            category: IntegrationCategory::Productivity,
            status_fn: status::coming_soon,
        },
        IntegrationEntry {
            name: "Notion",
            description: "Workspace & databases",
            category: IntegrationCategory::Productivity,
            status_fn: status::coming_soon,
        },
        IntegrationEntry {
            name: "Apple Notes",
            description: "Native macOS/iOS notes",
            category: IntegrationCategory::Productivity,
            status_fn: status::coming_soon,
        },
        IntegrationEntry {
            name: "Apple Reminders",
            description: "Task management",
            category: IntegrationCategory::Productivity,
            status_fn: status::coming_soon,
        },
        IntegrationEntry {
            name: "Obsidian",
            description: "Knowledge graph notes",
            category: IntegrationCategory::Productivity,
            status_fn: status::coming_soon,
        },
        IntegrationEntry {
            name: "Things 3",
            description: "GTD task manager",
            category: IntegrationCategory::Productivity,
            status_fn: status::coming_soon,
        },
        IntegrationEntry {
            name: "Bear Notes",
            description: "Markdown notes",
            category: IntegrationCategory::Productivity,
            status_fn: status::coming_soon,
        },
        IntegrationEntry {
            name: "Trello",
            description: "Kanban boards",
            category: IntegrationCategory::Productivity,
            status_fn: status::coming_soon,
        },
        IntegrationEntry {
            name: "Linear",
            description: "Issue tracking",
            category: IntegrationCategory::Productivity,
            status_fn: status::coming_soon,
        },
        // ── Music & Audio ───────────────────────────────────────
        IntegrationEntry {
            name: "Spotify",
            description: "Music playback control",
            category: IntegrationCategory::MusicAudio,
            status_fn: status::coming_soon,
        },
        IntegrationEntry {
            name: "Sonos",
            description: "Multi-room audio",
            category: IntegrationCategory::MusicAudio,
            status_fn: status::coming_soon,
        },
        IntegrationEntry {
            name: "Shazam",
            description: "Song recognition",
            category: IntegrationCategory::MusicAudio,
            status_fn: status::coming_soon,
        },
        // ── Smart Home ──────────────────────────────────────────
        IntegrationEntry {
            name: "Home Assistant",
            description: "Home automation hub",
            category: IntegrationCategory::SmartHome,
            status_fn: status::coming_soon,
        },
        IntegrationEntry {
            name: "Philips Hue",
            description: "Smart lighting",
            category: IntegrationCategory::SmartHome,
            status_fn: status::coming_soon,
        },
        IntegrationEntry {
            name: "8Sleep",
            description: "Smart mattress",
            category: IntegrationCategory::SmartHome,
            status_fn: status::coming_soon,
        },
        // ── Tools & Automation ──────────────────────────────────
        IntegrationEntry {
            name: "Browser",
            description: "Chrome/Chromium control",
            category: IntegrationCategory::ToolsAutomation,
            status_fn: status::available,
        },
        IntegrationEntry {
            name: "Shell",
            description: "Terminal command execution",
            category: IntegrationCategory::ToolsAutomation,
            status_fn: status::active,
        },
        IntegrationEntry {
            name: "File System",
            description: "Read/write files",
            category: IntegrationCategory::ToolsAutomation,
            status_fn: status::active,
        },
        IntegrationEntry {
            name: "Cron",
            description: "Scheduled tasks",
            category: IntegrationCategory::ToolsAutomation,
            status_fn: status::available,
        },
        IntegrationEntry {
            name: "Voice",
            description: "Voice wake + talk mode",
            category: IntegrationCategory::ToolsAutomation,
            status_fn: status::coming_soon,
        },
        IntegrationEntry {
            name: "Gmail",
            description: "Email triggers & send",
            category: IntegrationCategory::ToolsAutomation,
            status_fn: status::coming_soon,
        },
        IntegrationEntry {
            name: "1Password",
            description: "Secure credentials",
            category: IntegrationCategory::ToolsAutomation,
            status_fn: status::coming_soon,
        },
        IntegrationEntry {
            name: "Weather",
            description: "Forecasts & conditions",
            category: IntegrationCategory::ToolsAutomation,
            status_fn: status::coming_soon,
        },
        IntegrationEntry {
            name: "Canvas",
            description: "Visual workspace + A2UI",
            category: IntegrationCategory::ToolsAutomation,
            status_fn: status::coming_soon,
        },
        // ── Media & Creative ────────────────────────────────────
        IntegrationEntry {
            name: "Image Gen",
            description: "AI image generation",
            category: IntegrationCategory::MediaCreative,
            status_fn: status::coming_soon,
        },
        IntegrationEntry {
            name: "GIF Search",
            description: "Find the perfect GIF",
            category: IntegrationCategory::MediaCreative,
            status_fn: status::coming_soon,
        },
        IntegrationEntry {
            name: "Screen Capture",
            description: "Screenshot & screen control",
            category: IntegrationCategory::MediaCreative,
            status_fn: status::coming_soon,
        },
        IntegrationEntry {
            name: "Camera",
            description: "Photo/video capture",
            category: IntegrationCategory::MediaCreative,
            status_fn: status::coming_soon,
        },
        // ── Social ──────────────────────────────────────────────
        IntegrationEntry {
            name: "Twitter/X",
            description: "Tweet, reply, search",
            category: IntegrationCategory::Social,
            status_fn: status::coming_soon,
        },
        IntegrationEntry {
            name: "Email",
            description: "Send & read emails",
            category: IntegrationCategory::Social,
            status_fn: status::coming_soon,
        },
        // ── Platforms ───────────────────────────────────────────
        IntegrationEntry {
            name: "macOS",
            description: "Native support + AppleScript",
            category: IntegrationCategory::Platform,
            status_fn: status::macos,
        },
        IntegrationEntry {
            name: "Linux",
            description: "Native support",
            category: IntegrationCategory::Platform,
            status_fn: status::linux,
        },
        IntegrationEntry {
            name: "Windows",
            description: "WSL2 recommended",
            category: IntegrationCategory::Platform,
            status_fn: status::available,
        },
        IntegrationEntry {
            name: "iOS",
            description: "Chat via Telegram/Discord",
            category: IntegrationCategory::Platform,
            status_fn: status::available,
        },
        IntegrationEntry {
            name: "Android",
            description: "Chat via Telegram/Discord",
            category: IntegrationCategory::Platform,
            status_fn: status::available,
        },
    ]
}
