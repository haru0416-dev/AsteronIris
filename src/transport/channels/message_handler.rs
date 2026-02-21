use crate::config::Config;
use crate::core::agent::tool_loop::{LoopStopReason, ToolLoop};
use crate::core::providers::streaming::{ChannelStreamSink, StreamSink};
use crate::core::tools::middleware::ExecutionContext;
use crate::media::MediaProcessor;
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

#[allow(clippy::too_many_lines)]
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

    let source = format!("channel:{}", msg.channel);
    let ingress = apply_external_ingress_policy(&source, &msg.content);

    if rt.config.memory.auto_save {
        let policy_context = channel_runtime_policy_context();
        if let Err(error) = policy_context.enforce_recall_scope(channel_autosave_entity_id()) {
            tracing::warn!(error, "channel autosave skipped due to policy context");
        } else {
            let _ = rt
                .mem
                .append_event(channel_autosave_input(
                    &msg.channel,
                    &msg.sender,
                    ingress.persisted_summary.clone(),
                ))
                .await;
        }
    }

    if ingress.blocked {
        tracing::warn!(
            source,
            "blocked high-risk external content at channel ingress"
        );
        let _ = reply_to_origin(
            &rt.channels,
            &msg.channel,
            "⚠️ External content was blocked by safety policy.",
            &msg.sender,
        )
        .await;
        return;
    }

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
    let ctx = ExecutionContext {
        security: Arc::clone(&rt.security),
        autonomy_level: effective_autonomy,
        entity_id: match &msg.conversation_id {
            Some(conv_id) => format!("{}:{}:{}", msg.channel, conv_id, msg.sender),
            None => format!("{}:{}", msg.channel, msg.sender),
        },
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
    };
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

    match tool_loop
        .run(
            rt.provider.as_ref(),
            &rt.system_prompt,
            &message_input,
            &image_blocks,
            &rt.model,
            rt.temperature,
            &ctx,
            stream_sink,
        )
        .await
    {
        Ok(result) => {
            if let Some(handle) = stream_forward_handle {
                let _ = handle.await;
            }
            if let LoopStopReason::Error(error) = &result.stop_reason {
                eprintln!("  ✗ {}", t!("channels.llm_error", error = error));
                let _ = reply_to_origin(
                    &rt.channels,
                    &msg.channel,
                    &format!("! Error: {error}"),
                    &msg.sender,
                )
                .await;
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
            if let Some(handle) = stream_forward_handle {
                let _ = handle.await;
            }
            eprintln!("  ✗ {}", t!("channels.llm_error", error = error));
            let _ = reply_to_origin(
                &rt.channels,
                &msg.channel,
                &format!("! Error: {error}"),
                &msg.sender,
            )
            .await;
        }
    }
}
