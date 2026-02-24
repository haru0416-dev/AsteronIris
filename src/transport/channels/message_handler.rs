use crate::agent::{
    IntegrationRuntimeTurnOptions, IntegrationTurnParams, LoopStopReason, ToolLoopResult,
    run_main_session_turn_for_runtime_with_policy,
};
use crate::llm::streaming::{ChannelStreamSink, StreamSink};
use crate::security::writeback_guard::enforce_external_autosave_write_policy;
use crate::tools::ExecutionContext;
use crate::utils::text::truncate_with_ellipsis;
use anyhow::Result;
use std::sync::Arc;

use super::attachments::output_attachment_to_media_attachment;
use super::ingress_policy::{
    apply_external_ingress_policy, channel_autosave_entity_id, channel_autosave_input,
    channel_runtime_policy_context,
};
use super::policy::min_autonomy;
use super::startup::ChannelRuntime;
use super::traits::{Channel, ChannelMessage, MediaAttachment};
use crate::security::policy::AutonomyLevel;
use std::collections::HashSet;
use tokio::task::JoinHandle;

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

    // TODO: Port ingestion pipeline envelope-based batch ingestion to v2.
    // The v2 ingestion pipeline exists (memory::ingestion) but SignalEnvelope
    // construction differs. For now, log the channel message metadata.
    if msg.channel != "cli" {
        tracing::debug!(
            channel = %msg.channel,
            sender = %msg.sender,
            attachment_count = msg.attachments.len(),
            "channel message received (ingestion pipeline pending v2 port)"
        );
    }
}

async fn build_execution_context(
    rt: &ChannelRuntime,
    _msg: &ChannelMessage,
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
        rate_limiter: Arc::clone(&rt.rate_limiter),
        tenant_context,
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
                eprintln!("  ! channel llm error: {error}");
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
                LoopStopReason::HookBlocked(reason) => {
                    tracing::info!(channel = %msg.channel, sender = %msg.sender, %reason, "tool loop blocked by prompt hook");
                }
                LoopStopReason::Error(_) => unreachable!("error stop reason handled above"),
            }
            println!(
                "  > channel reply: {}",
                truncate_with_ellipsis(&result.final_text, 80)
            );
            if let Err(error) =
                reply_to_origin(&rt.channels, &msg.channel, &result.final_text, &msg.sender).await
            {
                eprintln!("  ! channel reply failed ({}): {error}", msg.channel);
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
            eprintln!("  ! channel llm error: {error}");
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
        "  > channel message from {}/{}: {}",
        msg.channel,
        msg.sender,
        truncate_with_ellipsis(&msg.content, 80)
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
            "External content was blocked by safety policy.",
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
    // TODO: Port MediaProcessor to v2 for image attachment handling.
    // For now, use the text content directly without image block conversion.
    let message_input = ingress.model_input;

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

    let entity_id = ctx.entity_id.clone();
    let policy_context = ctx.tenant_context.clone();
    let result = run_main_session_turn_for_runtime_with_policy(
        IntegrationTurnParams {
            config: rt.config.as_ref(),
            security: rt.security.as_ref(),
            mem: Arc::clone(&rt.mem),
            answer_provider: rt.provider.as_ref(),
            reflect_provider: rt.provider.as_ref(),
            system_prompt: &rt.system_prompt,
            model_name: &rt.model,
            temperature: rt.temperature,
            entity_id: &entity_id,
            policy_context,
            user_message: &message_input,
        },
        IntegrationRuntimeTurnOptions {
            registry: Arc::clone(&rt.registry),
            max_tool_iterations: rt.config.autonomy.max_tool_loop_iterations,
            execution_context: ctx,
            stream_sink,
            conversation_history: &[],
            hooks: &[],
        },
    )
    .await;
    process_tool_loop_result(rt, msg, result, stream_forward_handle).await;
}
