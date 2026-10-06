use std::time::Duration;
use lettre::{
    AsyncSmtpTransport, AsyncTransport, Tokio1Executor,
    message::{header::ContentType, MultiPart, SinglePart},
    transport::smtp::authentication::Credentials,
    Message,
};
use thiserror::Error;
use crate::config::{AppEnv, Config};

#[derive(Debug, Error)]
pub enum EmailError {
    #[error("failed to build email message: {0}")]
    Build(String),
    #[error("failed to send email via SMTP: {0}")]
    Transport(String),
    #[error("timed out sending email")]
    Timeout,
}

#[derive(Clone)]
pub struct EmailService {
    config: Config,
}

impl EmailService {
    pub fn new(config: Config) -> Self {
        Self { config }
    }

    pub async fn send_two_factor_code(&self, to_email: &str, code: &str) -> Result<(), EmailError> {
        if self.config.env == AppEnv::Test || self.config.smtp_host.is_none() {
            if self.config.env == AppEnv::Development || self.config.env == AppEnv::Test {
                tracing::info!(to = %to_email, code = %code, "2FA verification code generated (local/mock delivery)");
            } else {
                tracing::info!(to = %to_email, "2FA verification code generated");
            }
            return Ok(());
        }

        let host = match &self.config.smtp_host {
            Some(h) if !h.trim().is_empty() => h.clone(),
            _ => return Ok(()),
        };

        let from_header = format!(
            "{} <{}>",
            self.config.smtp_from_name, self.config.smtp_from_email
        );

        let from_mailbox: lettre::message::Mailbox = from_header
            .parse()
            .map_err(|e: lettre::address::AddressError| EmailError::Build(e.to_string()))?;

        let to_mailbox: lettre::message::Mailbox = to_email
            .parse()
            .map_err(|e: lettre::address::AddressError| EmailError::Build(e.to_string()))?;

        let plain_body = format!(
            "Your verification code is: {}\n\nThis code expires in 5 minutes.\nIf you did not request this, please ignore this email.\n",
            code
        );

        let html_body = format!(
            r#"<!DOCTYPE html>
<html>
<head><meta charset="utf-8"></head>
<body style="font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif; background-color: #f8f6f1; color: #28251f; padding: 40px 20px; margin: 0;">
  <div style="max-width: 480px; margin: 0 auto; background: #ffffff; border: 1px solid #ded8ce; border-radius: 8px; padding: 32px; box-shadow: 0 4px 12px rgba(40,37,31,0.05);">
    <h2 style="margin: 0 0 16px; font-size: 20px; font-weight: 600; color: #28251f;">Verification Code</h2>
    <p style="margin: 0 0 24px; font-size: 14px; line-height: 1.5; color: #686056;">
      Use the following one-time code to sign in to your blog studio. This code is valid for 5 minutes.
    </p>
    <div style="background: #fdfcf9; border: 1px solid #ded8ce; border-radius: 6px; padding: 18px; text-align: center; margin: 0 0 24px;">
      <span style="font-family: 'SF Mono', Monaco, monospace; font-size: 32px; font-weight: 700; letter-spacing: 6px; color: #a74832;">{}</span>
    </div>
    <p style="margin: 0; font-size: 12px; color: #8c8273; line-height: 1.5;">
      If you did not request this code, please ignore this email or review your account security.
    </p>
  </div>
</body>
</html>"#,
            code
        );

        let email = Message::builder()
            .from(from_mailbox)
            .to(to_mailbox)
            .subject(format!("{} is your verification code", code))
            .multipart(
                MultiPart::alternative()
                    .singlepart(
                        SinglePart::builder()
                            .header(ContentType::TEXT_PLAIN)
                            .body(plain_body),
                    )
                    .singlepart(
                        SinglePart::builder()
                            .header(ContentType::TEXT_HTML)
                            .body(html_body),
                    ),
            )
            .map_err(|e| EmailError::Build(e.to_string()))?;

        let mut transport_builder = AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&host)
            .map_err(|e| EmailError::Transport(e.to_string()))?
            .port(self.config.smtp_port);

        if let (Some(user), Some(pass)) = (&self.config.smtp_username, &self.config.smtp_password) {
            transport_builder = transport_builder.credentials(Credentials::new(user.clone(), pass.clone()));
        }

        let transport = transport_builder.build();

        tokio::time::timeout(Duration::from_secs(10), transport.send(email))
            .await
            .map_err(|_| EmailError::Timeout)?
            .map_err(|e| EmailError::Transport(e.to_string()))?;

        tracing::info!(to = %to_email, "2FA verification email sent successfully");
        Ok(())
    }
}
