use crate::channels::traits::{Channel, ChannelMessage};
use async_trait::async_trait;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::{Mutex, mpsc};

// Use tokio_rustls's re-export of rustls types
use tokio_rustls::rustls;

use super::auth::encode_sasl_plain;
use super::message::{IRC_STYLE_PREFIX, SENDER_PREFIX_RESERVE, split_message};
use super::parse::IrcMessage;
use super::tls::NoVerify;

/// Read timeout for IRC — if no data arrives within this duration, the
/// connection is considered dead. IRC servers typically PING every 60-120s.
const READ_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(300);

/// Monotonic counter to ensure unique message IDs under burst traffic.
static MSG_SEQ: AtomicU64 = AtomicU64::new(0);

/// IRC over TLS channel.
///
/// Connects to an IRC server using TLS, joins configured channels,
/// and forwards PRIVMSG messages to the `AsteronIris` message bus.
/// Supports both channel messages and private messages (DMs).
pub struct IrcChannel {
    pub(super) server: String,
    pub(super) port: u16,
    pub(super) nickname: String,
    pub(super) username: String,
    pub(super) channels: Vec<String>,
    pub(super) allowed_users: Vec<String>,
    pub(super) server_password: Option<String>,
    pub(super) nickserv_password: Option<String>,
    pub(super) sasl_password: Option<String>,
    pub(super) verify_tls: bool,
    /// Shared write half of the TLS stream for sending messages.
    writer: Arc<Mutex<Option<WriteHalf>>>,
}

type WriteHalf = tokio::io::WriteHalf<tokio_rustls::client::TlsStream<tokio::net::TcpStream>>;

impl IrcChannel {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        server: String,
        port: u16,
        nickname: String,
        username: Option<String>,
        channels: Vec<String>,
        allowed_users: Vec<String>,
        server_password: Option<String>,
        nickserv_password: Option<String>,
        sasl_password: Option<String>,
        verify_tls: bool,
    ) -> Self {
        let username = username.unwrap_or_else(|| nickname.clone());
        Self {
            server,
            port,
            nickname,
            username,
            channels,
            allowed_users,
            server_password,
            nickserv_password,
            sasl_password,
            verify_tls,
            writer: Arc::new(Mutex::new(None)),
        }
    }

    pub(super) fn is_user_allowed(&self, nick: &str) -> bool {
        if self.allowed_users.iter().any(|u| u == "*") {
            return true;
        }
        self.allowed_users
            .iter()
            .any(|u| u.eq_ignore_ascii_case(nick))
    }

    /// Create a TLS connection to the IRC server.
    async fn connect(
        &self,
    ) -> anyhow::Result<tokio_rustls::client::TlsStream<tokio::net::TcpStream>> {
        let addr = format!("{}:{}", self.server, self.port);
        let tcp = tokio::net::TcpStream::connect(&addr).await?;

        let tls_config = if self.verify_tls {
            let root_store: rustls::RootCertStore =
                webpki_roots::TLS_SERVER_ROOTS.iter().cloned().collect();
            rustls::ClientConfig::builder()
                .with_root_certificates(root_store)
                .with_no_client_auth()
        } else {
            rustls::ClientConfig::builder()
                .dangerous()
                .with_custom_certificate_verifier(Arc::new(NoVerify))
                .with_no_client_auth()
        };

        let connector = tokio_rustls::TlsConnector::from(Arc::new(tls_config));
        let domain = rustls::pki_types::ServerName::try_from(self.server.clone())?;
        let tls = connector.connect(domain, tcp).await?;

        Ok(tls)
    }

    /// Send a raw IRC line (appends \r\n).
    async fn send_raw(writer: &mut WriteHalf, line: &str) -> anyhow::Result<()> {
        let data = format!("{line}\r\n");
        writer.write_all(data.as_bytes()).await?;
        writer.flush().await?;
        Ok(())
    }
}

#[async_trait]
#[allow(clippy::too_many_lines)]
impl Channel for IrcChannel {
    fn name(&self) -> &str {
        "irc"
    }

    fn max_message_length(&self) -> usize {
        400
    }

    async fn send(&self, message: &str, recipient: &str) -> anyhow::Result<()> {
        let mut guard = self.writer.lock().await;
        let writer = guard
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("IRC not connected"))?;

        // Calculate safe payload size:
        // 512 - sender prefix (~64 bytes for :nick!user@host) - "PRIVMSG " - target - " :" - "\r\n"
        let overhead = SENDER_PREFIX_RESERVE + 10 + recipient.len() + 2;
        let max_payload = 512_usize.saturating_sub(overhead);
        let chunks = split_message(message, max_payload);

        for chunk in chunks {
            Self::send_raw(writer, &format!("PRIVMSG {recipient} :{chunk}")).await?;
        }

        Ok(())
    }

    async fn listen(&self, tx: mpsc::Sender<ChannelMessage>) -> anyhow::Result<()> {
        let mut current_nick = self.nickname.clone();
        tracing::info!(
            "IRC channel connecting to {}:{} as {}...",
            self.server,
            self.port,
            current_nick
        );

        let tls = self.connect().await?;
        let (reader, mut writer) = tokio::io::split(tls);

        // ── SASL negotiation ──
        if self.sasl_password.is_some() {
            Self::send_raw(&mut writer, "CAP REQ :sasl").await?;
        }

        // ── Server password ──
        if let Some(ref pass) = self.server_password {
            Self::send_raw(&mut writer, &format!("PASS {pass}")).await?;
        }

        // ── Nick/User registration ──
        Self::send_raw(&mut writer, &format!("NICK {current_nick}")).await?;
        Self::send_raw(
            &mut writer,
            &format!("USER {} 0 * :AsteronIris", self.username),
        )
        .await?;

        // Store writer for send()
        {
            let mut guard = self.writer.lock().await;
            *guard = Some(writer);
        }

        let mut buf_reader = BufReader::new(reader);
        let mut line = String::new();
        let mut registered = false;
        let mut sasl_pending = self.sasl_password.is_some();

        loop {
            line.clear();
            let n = tokio::time::timeout(READ_TIMEOUT, buf_reader.read_line(&mut line))
                .await
                .map_err(|_| {
                    anyhow::anyhow!("IRC read timed out (no data for {READ_TIMEOUT:?})")
                })??;
            if n == 0 {
                anyhow::bail!("IRC connection closed by server");
            }

            let Some(msg) = IrcMessage::parse(&line) else {
                continue;
            };

            match msg.command.as_str() {
                "PING" => {
                    let token = msg.params.first().map_or("", String::as_str);
                    let mut guard = self.writer.lock().await;
                    if let Some(ref mut w) = *guard {
                        Self::send_raw(w, &format!("PONG :{token}")).await?;
                    }
                }

                // CAP responses for SASL
                "CAP" => {
                    if sasl_pending && msg.params.iter().any(|p| p.contains("sasl")) {
                        if msg.params.iter().any(|p| p.contains("ACK")) {
                            // CAP * ACK :sasl — server accepted, start SASL auth
                            let mut guard = self.writer.lock().await;
                            if let Some(ref mut w) = *guard {
                                Self::send_raw(w, "AUTHENTICATE PLAIN").await?;
                            }
                        } else if msg.params.iter().any(|p| p.contains("NAK")) {
                            // CAP * NAK :sasl — server rejected SASL, proceed without it
                            tracing::warn!(
                                "IRC server does not support SASL, continuing without it"
                            );
                            sasl_pending = false;
                            let mut guard = self.writer.lock().await;
                            if let Some(ref mut w) = *guard {
                                Self::send_raw(w, "CAP END").await?;
                            }
                        }
                    }
                }

                "AUTHENTICATE" => {
                    // Server sends "AUTHENTICATE +" to request credentials
                    if sasl_pending && msg.params.first().is_some_and(|p| p == "+") {
                        let encoded = encode_sasl_plain(
                            &current_nick,
                            self.sasl_password.as_deref().unwrap_or(""),
                        );
                        let mut guard = self.writer.lock().await;
                        if let Some(ref mut w) = *guard {
                            Self::send_raw(w, &format!("AUTHENTICATE {encoded}")).await?;
                        }
                    }
                }

                // RPL_SASLSUCCESS (903) — SASL done, end CAP
                "903" => {
                    sasl_pending = false;
                    let mut guard = self.writer.lock().await;
                    if let Some(ref mut w) = *guard {
                        Self::send_raw(w, "CAP END").await?;
                    }
                }

                // SASL failure (904, 905, 906, 907)
                "904" | "905" | "906" | "907" => {
                    tracing::warn!("IRC SASL authentication failed ({})", msg.command);
                    sasl_pending = false;
                    let mut guard = self.writer.lock().await;
                    if let Some(ref mut w) = *guard {
                        Self::send_raw(w, "CAP END").await?;
                    }
                }

                // RPL_WELCOME — registration complete
                "001" => {
                    registered = true;
                    tracing::info!("IRC registered as {}", current_nick);

                    // NickServ authentication
                    if let Some(ref pass) = self.nickserv_password {
                        let mut guard = self.writer.lock().await;
                        if let Some(ref mut w) = *guard {
                            Self::send_raw(w, &format!("PRIVMSG NickServ :IDENTIFY {pass}"))
                                .await?;
                        }
                    }

                    // Join channels
                    for chan in &self.channels {
                        let mut guard = self.writer.lock().await;
                        if let Some(ref mut w) = *guard {
                            Self::send_raw(w, &format!("JOIN {chan}")).await?;
                        }
                    }
                }

                // ERR_NICKNAMEINUSE (433)
                "433" => {
                    let alt = format!("{current_nick}_");
                    tracing::warn!("IRC nickname {current_nick} is in use, trying {alt}");
                    let mut guard = self.writer.lock().await;
                    if let Some(ref mut w) = *guard {
                        Self::send_raw(w, &format!("NICK {alt}")).await?;
                    }
                    current_nick = alt;
                }

                "PRIVMSG" => {
                    if !registered {
                        continue;
                    }

                    let target = msg.params.first().map_or("", String::as_str);
                    let text = msg.params.get(1).map_or("", String::as_str);
                    let sender_nick = msg.nick().unwrap_or("unknown");

                    // Skip messages from NickServ/ChanServ
                    if sender_nick.eq_ignore_ascii_case("NickServ")
                        || sender_nick.eq_ignore_ascii_case("ChanServ")
                    {
                        continue;
                    }

                    if !self.is_user_allowed(sender_nick) {
                        continue;
                    }

                    // Determine reply target: if sent to a channel, reply to channel;
                    // if DM (target == our nick), reply to sender
                    let is_channel = target.starts_with('#') || target.starts_with('&');
                    let reply_to = if is_channel {
                        target.to_string()
                    } else {
                        sender_nick.to_string()
                    };
                    let content = if is_channel {
                        format!("{IRC_STYLE_PREFIX}<{sender_nick}> {text}")
                    } else {
                        format!("{IRC_STYLE_PREFIX}{text}")
                    };

                    let seq = MSG_SEQ.fetch_add(1, Ordering::Relaxed);
                    let channel_msg = ChannelMessage {
                        id: format!("irc_{}_{seq}", chrono::Utc::now().timestamp_millis()),
                        sender: reply_to,
                        content,
                        channel: "irc".to_string(),
                        timestamp: std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs(),
                        attachments: Vec::new(),
                    };

                    if tx.send(channel_msg).await.is_err() {
                        return Ok(());
                    }
                }

                // ERR_PASSWDMISMATCH (464) or other fatal errors
                "464" => {
                    anyhow::bail!("IRC password mismatch");
                }

                _ => {}
            }
        }
    }

    async fn health_check(&self) -> bool {
        // Lightweight connectivity check: TLS connect + QUIT
        match self.connect().await {
            Ok(tls) => {
                let (_, mut writer) = tokio::io::split(tls);
                let _ = Self::send_raw(&mut writer, "QUIT :health check").await;
                true
            }
            Err(_) => false,
        }
    }
}
