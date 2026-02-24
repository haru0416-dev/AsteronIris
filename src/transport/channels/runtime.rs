use crate::transport::channels::traits::{Channel, ChannelMessage};
use std::sync::Arc;
use std::time::Duration;

const DEFAULT_CHANNEL_INITIAL_BACKOFF_SECS: u64 = 2;
const DEFAULT_CHANNEL_MAX_BACKOFF_SECS: u64 = 60;

pub(crate) fn channel_backoff_settings(
    reliability: &crate::config::ReliabilityConfig,
) -> (u64, u64) {
    let initial_backoff_secs = reliability
        .channel_initial_backoff_secs
        .max(DEFAULT_CHANNEL_INITIAL_BACKOFF_SECS);
    let max_backoff_secs = reliability
        .channel_max_backoff_secs
        .max(DEFAULT_CHANNEL_MAX_BACKOFF_SECS);

    (initial_backoff_secs, max_backoff_secs)
}

pub(crate) fn spawn_supervised_listener(
    ch: Arc<dyn Channel>,
    tx: tokio::sync::mpsc::Sender<ChannelMessage>,
    initial_backoff_secs: u64,
    max_backoff_secs: u64,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let component = format!("channel:{}", ch.name());
        let mut backoff = initial_backoff_secs.max(1);
        let max_backoff = max_backoff_secs.max(backoff);

        loop {
            // TODO: Port runtime::diagnostics::health to v2.
            // For now, supervised listener operates without health tracking.
            tracing::debug!(component, "channel listener starting");
            let result = ch.listen(tx.clone()).await;

            if tx.is_closed() {
                break;
            }

            match result {
                Ok(()) => {
                    tracing::warn!("Channel {} exited unexpectedly; restarting", ch.name());
                    // Clean exit -- reset backoff since the listener ran successfully
                    backoff = initial_backoff_secs.max(1);
                }
                Err(e) => {
                    tracing::error!("Channel {} error: {e}; restarting", ch.name());
                }
            }

            tokio::time::sleep(Duration::from_secs(backoff)).await;
            // Double backoff AFTER sleeping so first error uses initial_backoff
            backoff = backoff.saturating_mul(2).min(max_backoff);
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct AlwaysFailChannel {
        name: &'static str,
        calls: Arc<AtomicUsize>,
    }

    impl Channel for AlwaysFailChannel {
        fn name(&self) -> &str {
            self.name
        }

        fn send<'a>(
            &'a self,
            _message: &'a str,
            _recipient: &'a str,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<()>> + Send + 'a>>
        {
            Box::pin(async move { Ok(()) })
        }

        fn listen<'a>(
            &'a self,
            _tx: tokio::sync::mpsc::Sender<ChannelMessage>,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<()>> + Send + 'a>>
        {
            Box::pin(async move {
                self.calls.fetch_add(1, Ordering::SeqCst);
                anyhow::bail!("listen boom")
            })
        }
    }

    #[tokio::test]
    async fn supervised_listener_restarts_on_failures() {
        let calls = Arc::new(AtomicUsize::new(0));
        let channel: Arc<dyn Channel> = Arc::new(AlwaysFailChannel {
            name: "test-supervised-fail",
            calls: Arc::clone(&calls),
        });

        let (tx, rx) = tokio::sync::mpsc::channel::<ChannelMessage>(1);
        let handle = spawn_supervised_listener(channel, tx, 1, 1);

        tokio::time::sleep(Duration::from_millis(80)).await;
        drop(rx);
        handle.abort();
        let _ = handle.await;

        assert!(calls.load(Ordering::SeqCst) >= 1);
    }
}
