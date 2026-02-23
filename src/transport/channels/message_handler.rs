use crate::config::Config;
use crate::core::agent::tool_loop::{LoopStopReason, ToolLoop, ToolLoopRunParams};
use crate::core::memory::ingestion::IngestionPipeline;
use crate::core::providers::streaming::{ChannelStreamSink, StreamSink};
use crate::core::tools::middleware::ExecutionContext;
use crate::media::MediaProcessor;
use crate::security::writeback_guard::enforce_external_autosave_write_policy;
use crate::security::{ChannelApprovalContext, broker_for_channel};
use crate::utils::text::truncate_with_ellipsis;
use anyhow::Result;
use std::sync::Arc;
use std::time::Duration;

use super::attachments::{output_attachment_to_media_attachment, prepare_channel_input_and_images};
use super::ingress_policy::{
    apply_external_ingress_policy, channel_autosave_entity_id, channel_autosave_input,
    channel_runtime_policy_context,
};
use super::policy::min_autonomy;
use super::startup::ChannelRuntime;
use super::traits::{Channel, ChannelMessage, MediaAttachment};
use crate::core::agent::tool_loop::ToolLoopResult;
use crate::security::policy::AutonomyLevel;
use std::collections::HashSet;
use tokio::task::JoinHandle;

fn build_channel_ingestion_envelopes(
    msg: &ChannelMessage,
    autosave_entity_id: &str,
    persisted_summary: &str,
) -> Vec<crate::core::memory::SignalEnvelope> {
    let source_kind = match msg.channel.as_str() {
        "discord" => crate::core::memory::SourceKind::Discord,
        "telegram" => crate::core::memory::SourceKind::Telegram,
        "slack" => crate::core::memory::SourceKind::Slack,
        _ => crate::core::memory::SourceKind::Api,
    };

    let mut base = crate::core::memory::SignalEnvelope::new(
        source_kind,
        format!("{}:{}", msg.channel, msg.sender),
        persisted_summary,
        autosave_entity_id,
    )
    .with_metadata("channel", &msg.channel)
    .with_metadata("sender", &msg.sender)
    .with_metadata("timestamp", msg.timestamp.to_string())
    .with_metadata("attachment_count", msg.attachments.len().to_string());

    if let Some(conversation_id) = &msg.conversation_id {
        base = base.with_metadata("conversation_id", conversation_id);
    }
    if let Some(thread_id) = &msg.thread_id {
        base = base.with_metadata("thread_id", thread_id);
    }
    if let Some(message_id) = &msg.message_id {
        base = base.with_metadata("message_id", message_id);
    }

    let mut envelopes = vec![base];
    if msg.channel == "discord" {
        for attachment in &msg.attachments {
            let filename = attachment.filename.as_deref().unwrap_or("unnamed");
            let attachment_content = format!(
                "discord attachment observed: file={filename} mime={}",
                attachment.mime_type
            );
            envelopes.push(
                crate::core::memory::SignalEnvelope::new(
                    source_kind,
                    format!("{}:{}:attachment:{filename}", msg.channel, msg.sender),
                    attachment_content,
                    autosave_entity_id,
                )
                .with_metadata("channel", &msg.channel)
                .with_metadata("sender", &msg.sender)
                .with_metadata("attachment", "true"),
            );
        }
    }

    envelopes
}

async fn reply_to_origin(
    channels: &[Arc<dyn Channel>],
    channel_name: &str,
    message: &str,
    sender: &str,
) -> Result<()> {
    for ch in channels {
        if ch.name() == channel_name {
            ch.send_chunked(message, sender).await?;
            break;
        }
    }
    Ok(())
}

async fn send_media_to_origin(
    channels: &[Arc<dyn Channel>],
    channel_name: &str,
    attachment: &MediaAttachment,
    sender: &str,
) -> Result<()> {
    for ch in channels {
        if ch.name() == channel_name {
            ch.send_media(attachment, sender).await?;
            break;
        }
    }
    Ok(())
}

fn approval_context_for_message(config: &Config, msg: &ChannelMessage) -> ChannelApprovalContext {
    let mut context = ChannelApprovalContext {
        timeout: Duration::from_secs(60),
        ..ChannelApprovalContext::default()
    };

    match msg.channel.as_str() {
        "discord" => {
            context.bot_token = config
                .channels_config
                .discord
                .as_ref()
                .map(|discord| discord.bot_token.clone());
            context.channel_id = Some(msg.sender.clone());
        }
        "telegram" => {
            context.bot_token = config
                .channels_config
                .telegram
                .as_ref()
                .map(|telegram| telegram.bot_token.clone());
            context.channel_id = Some(msg.sender.clone());
        }
        _ => {}
    }

    context
}

fn resolve_channel_policy(
    rt: &ChannelRuntime,
    msg: &ChannelMessage,
) -> (AutonomyLevel, Option<HashSet<String>>) {
    let global_autonomy = rt.config.autonomy.effective_autonomy_level();
    let channel_policy = rt.channel_policies.get(&msg.channel);
    let channel_level = channel_policy
        .and_then(|policy| policy.autonomy_level)
        .unwrap_or(global_autonomy);
    let effective_autonomy = min_autonomy(global_autonomy, channel_level);
    let tool_allowlist = channel_policy.and_then(|policy| policy.tool_allowlist.clone());

    tracing::debug!(
        channel = %msg.channel,
        sender = %msg.sender,
        effective_autonomy = ?effective_autonomy,
        has_tool_allowlist = tool_allowlist.is_some(),
        "resolved channel runtime policy"
    );

    (effective_autonomy, tool_allowlist)
}

async fn autosave_and_ingest(
    rt: &ChannelRuntime,
    msg: &ChannelMessage,
    autosave_entity_id: &str,
    persisted_summary: &str,
) {
    if rt.config.memory.auto_save {
        let policy_context = channel_runtime_policy_context();
        if let Err(error) = policy_context.enforce_recall_scope(autosave_entity_id) {
            tracing::warn!(error, "channel autosave skipped due to policy context");
        } else {
            let event = channel_autosave_input(
                autosave_entity_id,
                &msg.channel,
                &msg.sender,
                persisted_summary.to_string(),
            );
            if let Err(error) = enforce_external_autosave_write_policy(&event) {
                tracing::warn!(%error, "channel autosave rejected by write policy");
            } else if let Err(error) = rt.mem.append_event(event).await {
                tracing::warn!(%error, "failed to autosave channel input");
            }
        }
    }

    if msg.channel != "cli" {
        let envelopes =
            build_channel_ingestion_envelopes(msg, autosave_entity_id, persisted_summary);
        let pipeline =
            crate::core::memory::ingestion::SqliteIngestionPipeline::new(Arc::clone(&rt.mem));
        match pipeline.ingest_batch(envelopes).await {
            Ok(results) => {
                let accepted = results.iter().filter(|r| r.accepted).count();
                let dropped = results.len().saturating_sub(accepted);
                tracing::debug!(
                    channel = %msg.channel,
                    accepted,
                    dropped,
                    "ingestion pipeline processed channel message batch"
                );
            }
            Err(error) => {
                tracing::warn!(%error, "ingestion pipeline failed for channel message");
            }
        }
    }
}

async fn build_execution_context(
    rt: &ChannelRuntime,
    msg: &ChannelMessage,
    autosave_entity_id: String,
    effective_autonomy: AutonomyLevel,
    tool_allowlist: Option<HashSet<String>>,
) -> ExecutionContext {
    let tenant_context = channel_runtime_policy_context();
    let workspace_dir = if tenant_context.tenant_mode_enabled {
        let tenant_id = tenant_context.tenant_id.as_deref().unwrap_or("default");
        let scoped = rt.config.workspace_dir.join("tenants").join(tenant_id);
        if let Err(error) = tokio::fs::create_dir_all(&scoped).await {
            tracing::warn!(
                error = %error,
                tenant_id,
                "failed to create tenant scoped workspace"
            );
        }
        scoped
    } else {
        rt.config.workspace_dir.clone()
    };
    ExecutionContext {
        security: Arc::clone(&rt.security),
        autonomy_level: effective_autonomy,
        entity_id: autosave_entity_id,
        turn_number: 0,
        workspace_dir,
        allowed_tools: tool_allowlist,
        permission_store: Some(Arc::clone(&rt.permission_store)),
        rate_limiter: Arc::clone(&rt.rate_limiter),
        tenant_context,
        approval_broker: Some(broker_for_channel(
            &msg.channel,
            &approval_context_for_message(&rt.config, msg),
        )),
    }
}

async fn process_tool_loop_result(
    rt: &ChannelRuntime,
    msg: &ChannelMessage,
    result: Result<ToolLoopResult>,
    stream_forward_handle: Option<JoinHandle<()>>,
) {
    match result {
        Ok(result) => {
            if let Some(handle) = stream_forward_handle
                && let Err(error) = handle.await
            {
                tracing::warn!(%error, "stream forward task panicked");
            }
            if let LoopStopReason::Error(error) = &result.stop_reason {
                eprintln!("  ✗ {}", t!("channels.llm_error", error = error));
                if let Err(error) = reply_to_origin(
                    &rt.channels,
                    &msg.channel,
                    &format!("! Error: {error}"),
                    &msg.sender,
                )
                .await
                {
                    tracing::warn!(%error, "failed to send channel error reply");
                }
                return;
            }
            match result.stop_reason {
                LoopStopReason::MaxIterations => {
                    tracing::warn!(channel = %msg.channel, sender = %msg.sender, "tool loop hit max iterations");
                }
                LoopStopReason::RateLimited => {
                    tracing::warn!(channel = %msg.channel, sender = %msg.sender, "tool loop halted by rate limiting");
                }
                LoopStopReason::Completed | LoopStopReason::ApprovalDenied => {}
                LoopStopReason::Error(_) => unreachable!("error stop reason handled above"),
            }
            println!(
                "  › {} {}",
                t!("channels.reply"),
                truncate_with_ellipsis(&result.final_text, 80)
            );
            if let Err(error) =
                reply_to_origin(&rt.channels, &msg.channel, &result.final_text, &msg.sender).await
            {
                eprintln!(
                    "  ✗ {}",
                    t!("channels.reply_fail", channel = msg.channel, error = error)
                );
            }
            for attachment in &result.attachments {
                tracing::trace!(
                    channel = %msg.channel,
                    sender = %msg.sender,
                    mime_type = %attachment.mime_type,
                    filename = ?attachment.filename,
                    has_path = attachment.path.is_some(),
                    has_url = attachment.url.is_some(),
                    "processing tool output attachment"
                );

                let Some(channel_attachment) =
                    output_attachment_to_media_attachment(attachment).await
                else {
                    continue;
                };

                if let Err(error) = send_media_to_origin(
                    &rt.channels,
                    &msg.channel,
                    &channel_attachment,
                    &msg.sender,
                )
                .await
                {
                    tracing::trace!(
                        channel = %msg.channel,
                        sender = %msg.sender,
                        error = %error,
                        "channel does not support sending tool output media"
                    );
                }
            }
        }
        Err(error) => {
            if let Some(handle) = stream_forward_handle
                && let Err(error) = handle.await
            {
                tracing::warn!(%error, "stream forward task panicked");
            }
            eprintln!("  ✗ {}", t!("channels.llm_error", error = error));
            if let Err(error) = reply_to_origin(
                &rt.channels,
                &msg.channel,
                &format!("! Error: {error}"),
                &msg.sender,
            )
            .await
            {
                tracing::warn!(%error, "failed to send channel error reply");
            }
        }
    }
}

pub(super) async fn handle_channel_message(rt: &ChannelRuntime, msg: &ChannelMessage) {
    println!(
        "  › {}",
        t!(
            "channels.message_in",
            channel = msg.channel,
            sender = msg.sender,
            content = truncate_with_ellipsis(&msg.content, 80)
        )
    );

    let (effective_autonomy, tool_allowlist) = resolve_channel_policy(rt, msg);

    let source = format!("channel:{}", msg.channel);
    let ingress = apply_external_ingress_policy(&source, &msg.content);
    let autosave_entity_id = channel_autosave_entity_id(&msg.channel, &msg.sender);

    autosave_and_ingest(rt, msg, &autosave_entity_id, &ingress.persisted_summary).await;

    if ingress.blocked {
        tracing::warn!(
            source,
            "blocked high-risk external content at channel ingress"
        );
        if let Err(error) = reply_to_origin(
            &rt.channels,
            &msg.channel,
            "⚠️ External content was blocked by safety policy.",
            &msg.sender,
        )
        .await
        {
            tracing::warn!(%error, "failed to send channel safety block reply");
        }
        return;
    }

    let ctx = build_execution_context(
        rt,
        msg,
        autosave_entity_id,
        effective_autonomy,
        tool_allowlist,
    )
    .await;
    let tool_loop = ToolLoop::new(
        Arc::clone(&rt.registry),
        rt.config.autonomy.max_tool_loop_iterations,
    );
    let media_processor = MediaProcessor::with_provider(Arc::clone(&rt.provider), rt.model.clone());
    let (message_input, image_blocks) = prepare_channel_input_and_images(
        &ingress.model_input,
        &msg.attachments,
        rt.media_store.as_ref(),
        &media_processor,
    )
    .await;
    let mut stream_forward_handle = None;
    let stream_sink: Option<Arc<dyn StreamSink>> = rt
        .channels
        .iter()
        .find(|channel| channel.name() == msg.channel)
        .map(|channel| {
            let (tx, mut rx) = tokio::sync::mpsc::channel::<String>(32);
            let channel = Arc::clone(channel);
            let recipient = msg.sender.clone();
            let channel_name = msg.channel.clone();
            stream_forward_handle = Some(tokio::spawn(async move {
                while let Some(chunk) = rx.recv().await {
                    if chunk.is_empty() {
                        continue;
                    }
                    if let Err(error) = channel.send(&chunk, &recipient).await {
                        tracing::warn!(
                            channel = %channel_name,
                            recipient = %recipient,
                            error = %error,
                            "failed to stream channel chunk"
                        );
                        break;
                    }
                }
            }));
            Arc::new(ChannelStreamSink::new(tx, 80)) as Arc<dyn StreamSink>
        });

    let result = tool_loop
        .run(ToolLoopRunParams {
            provider: rt.provider.as_ref(),
            system_prompt: &rt.system_prompt,
            user_message: &message_input,
            image_content: &image_blocks,
            model: &rt.model,
            temperature: rt.temperature,
            ctx: &ctx,
            stream_sink,
            conversation_history: &[],
        })
        .await;
    process_tool_loop_result(rt, msg, result, stream_forward_handle).await;
}

#[cfg(test)]
mod tests {
    use super::build_channel_ingestion_envelopes;
    use crate::transport::channels::traits::{ChannelMessage, MediaAttachment, MediaData};

    fn discord_message_with_attachments() -> ChannelMessage {
        ChannelMessage {
            id: "msg-1".to_string(),
            sender: "user-42".to_string(),
            content: "hello from discord".to_string(),
            channel: "discord".to_string(),
            conversation_id: Some("channel-77".to_string()),
            thread_id: Some("thread-9".to_string()),
            reply_to: None,
            message_id: Some("discord-msg-abc".to_string()),
            timestamp: 1_716_171_717,
            attachments: vec![
                MediaAttachment {
                    mime_type: "image/png".to_string(),
                    data: MediaData::Url("https://cdn.discord.test/img.png".to_string()),
                    filename: Some("img.png".to_string()),
                },
                MediaAttachment {
                    mime_type: "application/pdf".to_string(),
                    data: MediaData::Url("https://cdn.discord.test/doc.pdf".to_string()),
                    filename: Some("doc.pdf".to_string()),
                },
            ],
        }
    }

    #[test]
    fn discord_ingestion_envelopes_include_attachment_metadata_items() {
        let msg = discord_message_with_attachments();
        let envelopes =
            build_channel_ingestion_envelopes(&msg, "person:discord.user_42", "persisted summary");

        assert_eq!(envelopes.len(), 3);

        let base = &envelopes[0];
        assert_eq!(base.source_kind, crate::core::memory::SourceKind::Discord);
        assert_eq!(base.source_ref, "discord:user-42");
        assert_eq!(base.content, "persisted summary");
        assert_eq!(
            base.metadata.get("attachment_count").map(String::as_str),
            Some("2")
        );
        assert_eq!(
            base.metadata.get("conversation_id").map(String::as_str),
            Some("channel-77")
        );
        assert_eq!(
            base.metadata.get("thread_id").map(String::as_str),
            Some("thread-9")
        );
        assert_eq!(
            base.metadata.get("message_id").map(String::as_str),
            Some("discord-msg-abc")
        );

        let attachment_1 = &envelopes[1];
        assert_eq!(
            attachment_1.source_ref,
            "discord:user-42:attachment:img.png"
        );
        assert!(attachment_1.content.contains("file=img.png"));
        assert!(attachment_1.content.contains("mime=image/png"));
        assert_eq!(
            attachment_1.metadata.get("attachment").map(String::as_str),
            Some("true")
        );

        let attachment_2 = &envelopes[2];
        assert_eq!(
            attachment_2.source_ref,
            "discord:user-42:attachment:doc.pdf"
        );
        assert!(attachment_2.content.contains("file=doc.pdf"));
        assert!(attachment_2.content.contains("mime=application/pdf"));
    }

    #[test]
    fn non_discord_ingestion_envelope_does_not_expand_attachments() {
        let mut msg = discord_message_with_attachments();
        msg.channel = "telegram".to_string();

        let envelopes =
            build_channel_ingestion_envelopes(&msg, "person:telegram.user_42", "persisted summary");

        assert_eq!(envelopes.len(), 1);
        assert_eq!(
            envelopes[0].source_kind,
            crate::core::memory::SourceKind::Telegram
        );
        assert_eq!(
            envelopes[0]
                .metadata
                .get("attachment_count")
                .map(String::as_str),
            Some("2")
        );
    }

    #[test]
    fn discord_ingestion_envelopes_use_unnamed_fallback_for_missing_filename() {
        let mut msg = discord_message_with_attachments();
        msg.attachments[0].filename = None;

        let envelopes =
            build_channel_ingestion_envelopes(&msg, "person:discord.user_42", "persisted summary");

        assert_eq!(envelopes.len(), 3);
        let attachment = &envelopes[1];
        assert_eq!(attachment.source_ref, "discord:user-42:attachment:unnamed");
        assert!(attachment.content.contains("file=unnamed"));
    }

    #[test]
    fn discord_ingestion_envelopes_keep_single_base_when_no_attachments() {
        let mut msg = discord_message_with_attachments();
        msg.attachments.clear();

        let envelopes =
            build_channel_ingestion_envelopes(&msg, "person:discord.user_42", "persisted summary");

        assert_eq!(envelopes.len(), 1);
        assert_eq!(envelopes[0].source_ref, "discord:user-42");
        assert_eq!(
            envelopes[0]
                .metadata
                .get("attachment_count")
                .map(String::as_str),
            Some("0")
        );
    }
}
