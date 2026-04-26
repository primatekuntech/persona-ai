/// Resend email client for invite and password-reset emails.
/// Uses the Resend REST API directly via reqwest.
use crate::error::AppError;
use serde_json::json;

#[derive(Clone)]
pub struct ResendClient {
    api_key: String,
    from: String,
    http: reqwest::Client,
}

impl ResendClient {
    pub fn new(api_key: impl Into<String>, from: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            from: from.into(),
            http: reqwest::Client::new(),
        }
    }

    pub async fn send_invite(
        &self,
        to: &str,
        invite_url: &str,
        inviter_name: &str,
    ) -> Result<(), AppError> {
        let html = format!(
            r#"<p>Hello,</p>
<p><strong>{inviter_name}</strong> has invited you to join <strong>Persona AI</strong>.</p>
<p><a href="{invite_url}" style="display:inline-block;padding:10px 20px;background:#18181B;color:#FAFAFA;text-decoration:none;border-radius:4px;">Accept Invitation</a></p>
<p>Or paste this link into your browser:<br><a href="{invite_url}">{invite_url}</a></p>
<p>This link expires in 7 days.</p>"#
        );
        let text = format!(
            "You've been invited to Persona AI by {inviter_name}.\n\nAccept: {invite_url}\n\nThis link expires in 7 days."
        );

        self.send(to, "You've been invited to Persona AI", &html, &text)
            .await
    }

    pub async fn send_password_reset(&self, to: &str, reset_url: &str) -> Result<(), AppError> {
        let html = format!(
            r#"<p>You requested a password reset for your Persona AI account.</p>
<p><a href="{reset_url}" style="display:inline-block;padding:10px 20px;background:#18181B;color:#FAFAFA;text-decoration:none;border-radius:4px;">Reset Password</a></p>
<p>Or paste: <a href="{reset_url}">{reset_url}</a></p>
<p>This link expires in 30 minutes. If you did not request this, ignore this email.</p>"#
        );
        let text = format!(
            "Reset your Persona AI password: {reset_url}\n\nExpires in 30 minutes. If you did not request this, ignore this email."
        );

        self.send(to, "Reset your Persona AI password", &html, &text)
            .await
    }

    async fn send(&self, to: &str, subject: &str, html: &str, text: &str) -> Result<(), AppError> {
        let payload = json!({
            "from": self.from,
            "to": [to],
            "subject": subject,
            "html": html,
            "text": text
        });

        let resp = self
            .http
            .post("https://api.resend.com/emails")
            .bearer_auth(&self.api_key)
            .json(&payload)
            .send()
            .await
            .map_err(|e| AppError::Internal(anyhow::anyhow!("resend http error: {e}")))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(AppError::Internal(anyhow::anyhow!(
                "resend API error {status}: {body}"
            )));
        }

        Ok(())
    }
}
