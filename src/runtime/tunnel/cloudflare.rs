use super::{SharedProcess, Tunnel, TunnelProcess, kill_shared, new_shared_process};
use anyhow::{Result, bail};
use tokio::io::AsyncBufReadExt;
use tokio::process::Command;

/// Cloudflare Tunnel — wraps the `cloudflared` binary.
///
/// Requires `cloudflared` installed and a tunnel token from the
/// Cloudflare Zero Trust dashboard.
pub struct CloudflareTunnel {
    token: String,
    proc: SharedProcess,
}

impl CloudflareTunnel {
    pub fn new(token: String) -> Self {
        Self {
            token,
            proc: new_shared_process(),
        }
    }
}

impl Tunnel for CloudflareTunnel {
    fn name(&self) -> &str {
        "cloudflare"
    }

    fn start<'a>(
        &'a self,
        _local_host: &'a str,
        local_port: u16,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<String>> + Send + 'a>> {
        Box::pin(async move {
            // cloudflared tunnel --no-autoupdate run --token <TOKEN> --url http://localhost:<port>
            let mut child = Command::new("cloudflared")
                .args([
                    "tunnel",
                    "--no-autoupdate",
                    "run",
                    "--token",
                    &self.token,
                    "--url",
                    &format!("http://localhost:{local_port}"),
                ])
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .kill_on_drop(true)
                .spawn()?;

            // Read stderr to find the public URL (cloudflared prints it there)
            let stderr = child
                .stderr
                .take()
                .ok_or_else(|| anyhow::anyhow!("Failed to capture cloudflared stderr"))?;

            let mut reader = tokio::io::BufReader::new(stderr).lines();
            let mut public_url = String::new();

            // Wait up to 30s for the tunnel URL to appear
            let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(30);
            while tokio::time::Instant::now() < deadline {
                let line =
                    tokio::time::timeout(tokio::time::Duration::from_secs(5), reader.next_line())
                        .await;

                match line {
                    Ok(Ok(Some(l))) => {
                        tracing::debug!("cloudflared: {l}");
                        // Look for the URL pattern in cloudflared output
                        if let Some(idx) = l.find("https://") {
                            let url_part = &l[idx..];
                            let end = url_part
                                .find(|c: char| c.is_whitespace())
                                .unwrap_or(url_part.len());
                            public_url = url_part[..end].to_string();
                            break;
                        }
                    }
                    Ok(Ok(None)) => break,
                    Ok(Err(e)) => bail!("Error reading cloudflared output: {e}"),
                    Err(_) => {
                        tracing::trace!(
                            "cloudflared: waiting for tunnel URL (line read timed out)"
                        );
                    }
                }
            }

            if public_url.is_empty() {
                child.kill().await.ok();
                bail!("cloudflared did not produce a public URL within 30s. Is the token valid?");
            }

            let mut guard = self.proc.lock().await;
            *guard = Some(TunnelProcess {
                child,
                public_url: public_url.clone(),
            });

            Ok(public_url)
        })
    }

    fn stop(&self) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>> {
        Box::pin(async move { kill_shared(&self.proc).await })
    }

    fn health_check(
        &self,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = bool> + Send + '_>> {
        Box::pin(async move {
            let guard = self.proc.lock().await;
            guard.as_ref().is_some_and(|tp| tp.child.id().is_some())
        })
    }

    fn public_url(&self) -> Option<String> {
        // Can't block on async lock in a sync fn, so we try_lock
        self.proc
            .try_lock()
            .ok()
            .and_then(|g| g.as_ref().map(|tp| tp.public_url.clone()))
    }
}
