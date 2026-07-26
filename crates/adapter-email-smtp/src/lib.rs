//! SMTP implementation of the [`app::EmailSender`] port.
//!
//! Async delivery over STARTTLS using `lettre` with rustls (no OpenSSL, matching
//! the federation transport and sqlx). The single-box deployment points this at the
//! mail host's Postfix submission port (587) authenticating as a dedicated mailbox;
//! Postfix then relays outbound.

use app::EmailSender;
use async_trait::async_trait;
use lettre::message::header::ContentType;
use lettre::message::Mailbox;
use lettre::transport::smtp::authentication::Credentials;
use lettre::transport::smtp::client::{Tls, TlsParameters};
use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};

/// Connection + identity settings for the SMTP sender, read from the environment
/// by the composition root.
pub struct SmtpConfig {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: String,
    /// The `From` header/envelope, e.g. `democrachat <no-reply@example.com>`.
    pub from: String,
    /// Whether to upgrade the connection with STARTTLS (the submission-port norm).
    /// `false` sends over a plaintext connection (only sensible on a trusted LAN
    /// hop or for a local test relay).
    pub starttls: bool,
    /// Accept a TLS certificate that doesn't match the host (name mismatch) or
    /// isn't in the trust store. Needed for an internal hop whose submission cert
    /// is issued for a different name than the address we connect to. Off by
    /// default; enabling it trusts the network path to the relay. Only meaningful
    /// when [`starttls`](Self::starttls) is on.
    pub accept_invalid_certs: bool,
}

/// An [`EmailSender`] backed by an authenticated SMTP submission relay.
pub struct SmtpEmailSender {
    transport: AsyncSmtpTransport<Tokio1Executor>,
    from: Mailbox,
}

impl SmtpEmailSender {
    /// Build the transport. Errors on an unparseable `from` address or bad TLS
    /// parameters — a configuration fault the composition root turns into a
    /// fail-closed exit.
    pub fn new(cfg: SmtpConfig) -> Result<Self, String> {
        let from: Mailbox = cfg
            .from
            .parse()
            .map_err(|e| format!("invalid DEMOCRACHAT_SMTP_FROM address {:?}: {e}", cfg.from))?;

        // STARTTLS (submission-port norm): plaintext connection upgraded to TLS,
        // then AUTH. When disabled, connect in the clear (trusted LAN hop only).
        let tls = if cfg.starttls {
            let mut builder = TlsParameters::builder(cfg.host.clone());
            if cfg.accept_invalid_certs {
                builder = builder
                    .dangerous_accept_invalid_certs(true)
                    .dangerous_accept_invalid_hostnames(true);
            }
            Tls::Required(builder.build().map_err(|e| format!("SMTP TLS setup failed: {e}"))?)
        } else {
            Tls::None
        };
        let transport = AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(&cfg.host)
            .port(cfg.port)
            .tls(tls)
            .credentials(Credentials::new(cfg.username, cfg.password))
            .build();

        Ok(Self { transport, from })
    }
}

#[async_trait]
impl EmailSender for SmtpEmailSender {
    async fn send(&self, to: &str, subject: &str, body: &str) -> Result<(), String> {
        let to: Mailbox = to
            .parse()
            .map_err(|e| format!("invalid recipient {to:?}: {e}"))?;
        let email = Message::builder()
            .from(self.from.clone())
            .to(to)
            .subject(subject)
            .header(ContentType::TEXT_PLAIN)
            .body(body.to_string())
            .map_err(|e| format!("building email failed: {e}"))?;
        self.transport
            .send(email)
            .await
            .map_err(|e| format!("sending email failed: {e}"))?;
        Ok(())
    }
}
