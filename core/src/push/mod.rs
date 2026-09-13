//! 推送渠道（FR-4.10）：邮件（SMTP）、Telegram Bot、企业微信群机器人 Webhook
//!
//! 与桌面通知/浏览器横幅互补：当应用未运行（服务器模式）或用户不在电脑前时，
//! 仍可通过邮件/IM 收到提醒。所有凭据存系统安全存储，配置经 API 提交。

use crate::db::Db;
use crate::secrets;
use anyhow::Result;
use serde::{Deserialize, Serialize};

/// 渠道标识
pub const CH_EMAIL: &str = "email";
pub const CH_TELEGRAM: &str = "telegram";
pub const CH_WECOM: &str = "wecom";

pub fn all_channels() -> [&'static str; 3] {
    [CH_EMAIL, CH_TELEGRAM, CH_WECOM]
}

/// 推送配置（不含任何明文凭据，仅返回"是否已配置"）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PushConfig {
    /// 已启用的渠道列表，如 ["wecom","email"]
    pub channels: Vec<String>,
    // 邮件
    pub smtp_host: String,
    pub smtp_port: u16,
    pub smtp_user: String,
    pub email_from: String,
    pub email_to: String,
    /// starttls | tls | none
    pub smtp_security: String,
    pub has_smtp_password: bool,
    // Telegram
    pub telegram_chat_id: String,
    pub has_telegram_token: bool,
    // 企业微信
    pub wecom_webhook: String,
}

fn get(db: &Db, key: &str, default: &str) -> String {
    db.get_setting(key)
        .ok()
        .flatten()
        .unwrap_or_else(|| default.to_string())
}

pub fn load_config(db: &Db) -> PushConfig {
    let channels = db
        .get_setting("push_channels")
        .ok()
        .flatten()
        .and_then(|v| serde_json::from_str::<Vec<String>>(&v).ok())
        .unwrap_or_default();
    PushConfig {
        channels,
        smtp_host: get(db, "smtp_host", ""),
        smtp_port: get(db, "smtp_port", "587").parse().unwrap_or(587),
        smtp_user: get(db, "smtp_user", ""),
        email_from: get(db, "email_from", ""),
        email_to: get(db, "email_to", ""),
        smtp_security: get(db, "smtp_security", "starttls"),
        has_smtp_password: secrets::has_secret(secrets::SECRET_SMTP_PASSWORD),
        telegram_chat_id: get(db, "telegram_chat_id", ""),
        has_telegram_token: secrets::has_secret(secrets::SECRET_TELEGRAM_TOKEN),
        wecom_webhook: get(db, "wecom_webhook", ""),
    }
}

pub fn save_config(db: &Db, cfg: &PushConfig) -> Result<()> {
    db.set_setting("push_channels", &serde_json::to_string(&cfg.channels)?)?;
    db.set_setting("smtp_host", &cfg.smtp_host)?;
    db.set_setting("smtp_port", &cfg.smtp_port.to_string())?;
    db.set_setting("smtp_user", &cfg.smtp_user)?;
    db.set_setting("email_from", &cfg.email_from)?;
    db.set_setting("email_to", &cfg.email_to)?;
    db.set_setting("smtp_security", &cfg.smtp_security)?;
    db.set_setting("telegram_chat_id", &cfg.telegram_chat_id)?;
    db.set_setting("wecom_webhook", &cfg.wecom_webhook)?;
    Ok(())
}

/// 保存凭据（空字符串表示不修改）
pub fn save_credentials(
    smtp_password: Option<&str>,
    telegram_token: Option<&str>,
) -> Result<()> {
    if let Some(p) = smtp_password {
        if !p.is_empty() {
            secrets::save_secret(secrets::SECRET_SMTP_PASSWORD, p)?;
        }
    }
    if let Some(t) = telegram_token {
        if !t.is_empty() {
            secrets::save_secret(secrets::SECRET_TELEGRAM_TOKEN, t)?;
        }
    }
    Ok(())
}

/// 一条待发送的推送消息
#[derive(Debug, Clone)]
pub struct OutgoingMessage {
    pub title: String,
    pub body: String,
    /// 通知类型（record/overdue/care/todo/brief/goodnight）
    pub kind: String,
    pub at: String,
}

impl OutgoingMessage {
    /// 纯文本渲染（用于 IM/邮件正文）
    pub fn plain_text(&self) -> String {
        format!(
            "【智伴 Mindmate】{}\n{}\n\n—— {}",
            self.title, self.body, self.at
        )
    }
    /// 邮件主题
    pub fn subject(&self) -> String {
        format!("[智伴] {}", self.title)
    }
    /// 企业微信文本消息体
    pub fn wecom_payload(&self) -> serde_json::Value {
        serde_json::json!({
            "msgtype": "text",
            "text": { "content": self.plain_text() }
        })
    }
    /// Telegram sendMessage 请求体
    pub fn telegram_payload(&self, chat_id: &str) -> serde_json::Value {
        serde_json::json!({
            "chat_id": chat_id,
            "text": self.plain_text(),
            "disable_notification": false
        })
    }
}

/// 推送结果（逐渠道）
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PushResult {
    pub channel: String,
    pub ok: bool,
    pub detail: String,
}

/// 通过企业微信群机器人 Webhook 发送
pub async fn send_wecom(webhook: &str, msg: &OutgoingMessage) -> Result<String> {
    if webhook.trim().is_empty() {
        anyhow::bail!("未配置企业微信 Webhook 地址");
    }
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()?;
    let resp = client
        .post(webhook)
        .json(&msg.wecom_payload())
        .send()
        .await?;
    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        anyhow::bail!("企业微信返回 {status}：{}", truncate(&text, 200));
    }
    // 企业微信成功时返回 {"errcode":0,...}
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
        if let Some(code) = v.get("errcode").and_then(|c| c.as_i64()) {
            if code != 0 {
                anyhow::bail!(
                    "企业微信错误码 {code}：{}",
                    v.get("errmsg").and_then(|m| m.as_str()).unwrap_or("")
                );
            }
        }
    }
    Ok("已投递".into())
}

/// 通过 Telegram Bot API 发送
pub async fn send_telegram(token: &str, chat_id: &str, msg: &OutgoingMessage) -> Result<String> {
    if token.trim().is_empty() {
        anyhow::bail!("未配置 Telegram Bot Token");
    }
    if chat_id.trim().is_empty() {
        anyhow::bail!("未配置 Telegram Chat ID");
    }
    let url = format!("https://api.telegram.org/bot{token}/sendMessage");
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()?;
    let resp = client
        .post(&url)
        .json(&msg.telegram_payload(chat_id))
        .send()
        .await?;
    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        anyhow::bail!("Telegram 返回 {status}：{}", truncate(&text, 200));
    }
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
        if v.get("ok").and_then(|o| o.as_bool()) == Some(false) {
            anyhow::bail!(
                "Telegram 错误：{}",
                v.get("description").and_then(|d| d.as_str()).unwrap_or("unknown")
            );
        }
    }
    Ok("已投递".into())
}

/// 通过 SMTP 发送邮件
pub async fn send_email(cfg: &PushConfig, msg: &OutgoingMessage) -> Result<String> {
    use lettre::message::header::ContentType;
    use lettre::transport::smtp::authentication::Credentials;
    use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};

    if cfg.smtp_host.trim().is_empty() {
        anyhow::bail!("未配置 SMTP 服务器");
    }
    if cfg.email_to.trim().is_empty() {
        anyhow::bail!("未配置收件人邮箱");
    }
    let from = if cfg.email_from.trim().is_empty() {
        cfg.smtp_user.clone()
    } else {
        cfg.email_from.clone()
    };
    if from.trim().is_empty() {
        anyhow::bail!("未配置发件人邮箱");
    }

    let email = Message::builder()
        .from(from.parse()?)
        .to(cfg.email_to.parse()?)
        .subject(msg.subject())
        .header(ContentType::TEXT_PLAIN)
        .body(msg.plain_text())?;

    let mut builder = match cfg.smtp_security.as_str() {
        "tls" => AsyncSmtpTransport::<Tokio1Executor>::relay(&cfg.smtp_host)?,
        "none" => AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(&cfg.smtp_host),
        _ => AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&cfg.smtp_host)?,
    }
    .port(cfg.smtp_port);

    if !cfg.smtp_user.trim().is_empty() {
        let password = secrets::load_secret(secrets::SECRET_SMTP_PASSWORD)?
            .ok_or_else(|| anyhow::anyhow!("未配置 SMTP 密码/授权码"))?;
        builder = builder.credentials(Credentials::new(cfg.smtp_user.clone(), password));
    }

    let transport = builder.build();
    transport.send(email).await?;
    Ok("已投递".into())
}

/// 向所有已启用渠道推送（失败不影响其它渠道，逐个返回结果）
pub async fn dispatch(db: &Db, msg: &OutgoingMessage) -> Vec<PushResult> {
    let cfg = load_config(db);
    let mut results = Vec::new();
    for ch in &cfg.channels {
        let r = match ch.as_str() {
            CH_WECOM => send_wecom(&cfg.wecom_webhook, msg).await,
            CH_TELEGRAM => {
                let token = secrets::load_secret(secrets::SECRET_TELEGRAM_TOKEN).unwrap_or(None);
                send_telegram(token.as_deref().unwrap_or(""), &cfg.telegram_chat_id, msg).await
            }
            CH_EMAIL => send_email(&cfg, msg).await,
            other => Err(anyhow::anyhow!("未知渠道：{other}")),
        };
        let (ok, detail) = match r {
            Ok(d) => (true, d),
            Err(e) => {
                tracing::warn!("推送渠道 {ch} 失败: {e}");
                (false, e.to_string())
            }
        };
        results.push(PushResult {
            channel: ch.clone(),
            ok,
            detail,
        });
    }
    results
}

/// 单渠道测试（设置页「测试」按钮）
pub async fn test_channel(db: &Db, channel: &str) -> PushResult {
    let cfg = load_config(db);
    let msg = OutgoingMessage {
        title: "推送渠道测试".into(),
        body: "如果你收到这条消息，说明该渠道配置成功。".into(),
        kind: "test".into(),
        at: crate::db::now_string(),
    };
    let r = match channel {
        CH_WECOM => send_wecom(&cfg.wecom_webhook, &msg).await,
        CH_TELEGRAM => {
            let token = secrets::load_secret(secrets::SECRET_TELEGRAM_TOKEN).unwrap_or(None);
            send_telegram(token.as_deref().unwrap_or(""), &cfg.telegram_chat_id, &msg).await
        }
        CH_EMAIL => send_email(&cfg, &msg).await,
        other => Err(anyhow::anyhow!("未知渠道：{other}")),
    };
    match r {
        Ok(d) => PushResult {
            channel: channel.into(),
            ok: true,
            detail: d,
        },
        Err(e) => PushResult {
            channel: channel.into(),
            ok: false,
            detail: e.to_string(),
        },
    }
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(n).collect::<String>())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> OutgoingMessage {
        OutgoingMessage {
            title: "该记录一下了".into(),
            body: "今日已录 2/4 条".into(),
            kind: "record".into(),
            at: "2026-09-12 15:00:00".into(),
        }
    }

    #[test]
    fn 企业微信载荷格式正确() {
        let p = sample().wecom_payload();
        assert_eq!(p["msgtype"], "text");
        assert!(p["text"]["content"].as_str().unwrap().contains("该记录一下了"));
        assert!(p["text"]["content"].as_str().unwrap().contains("智伴"));
    }

    #[test]
    fn telegram载荷包含chat_id与文本() {
        let p = sample().telegram_payload("-100123");
        assert_eq!(p["chat_id"], "-100123");
        assert!(p["text"].as_str().unwrap().contains("2/4"));
    }

    #[test]
    fn 邮件主题与正文渲染() {
        let m = sample();
        assert_eq!(m.subject(), "[智伴] 该记录一下了");
        assert!(m.plain_text().contains("15:00:00"));
    }

    #[tokio::test]
    async fn 未配置webhook时给出可读错误() {
        let e = send_wecom("", &sample()).await.unwrap_err().to_string();
        assert!(e.contains("未配置企业微信"), "{e}");
    }

    #[tokio::test]
    async fn 未配置telegram时给出可读错误() {
        let e = send_telegram("", "123", &sample()).await.unwrap_err().to_string();
        assert!(e.contains("Token"), "{e}");
        let e = send_telegram("tok", "", &sample()).await.unwrap_err().to_string();
        assert!(e.contains("Chat ID"), "{e}");
    }

    #[tokio::test]
    async fn 未配置邮箱时给出可读错误() {
        let cfg = PushConfig {
            channels: vec![],
            smtp_host: "".into(),
            smtp_port: 587,
            smtp_user: "".into(),
            email_from: "".into(),
            email_to: "".into(),
            smtp_security: "starttls".into(),
            has_smtp_password: false,
            telegram_chat_id: "".into(),
            has_telegram_token: false,
            wecom_webhook: "".into(),
        };
        let e = send_email(&cfg, &sample()).await.unwrap_err().to_string();
        assert!(e.contains("SMTP"), "{e}");
    }

    #[test]
    fn 默认无启用渠道() {
        let db = Db::open_memory().unwrap();
        db.seed_defaults().unwrap();
        let cfg = load_config(&db);
        assert!(cfg.channels.is_empty(), "默认不应启用任何推送渠道");
        assert!(!cfg.has_smtp_password);
        assert!(!cfg.has_telegram_token);
    }

    #[test]
    fn 推送配置可保存与读回() {
        let db = Db::open_memory().unwrap();
        db.seed_defaults().unwrap();
        let cfg = PushConfig {
            channels: vec![CH_WECOM.into(), CH_EMAIL.into()],
            smtp_host: "smtp.example.com".into(),
            smtp_port: 465,
            smtp_user: "me@example.com".into(),
            email_from: "me@example.com".into(),
            email_to: "to@example.com".into(),
            smtp_security: "tls".into(),
            has_smtp_password: false,
            telegram_chat_id: "".into(),
            has_telegram_token: false,
            wecom_webhook: "https://qyapi.weixin.qq.com/cgi-bin/webhook/send?key=abc".into(),
        };
        save_config(&db, &cfg).unwrap();
        let back = load_config(&db);
        assert_eq!(back.channels, vec![CH_WECOM.to_string(), CH_EMAIL.to_string()]);
        assert_eq!(back.smtp_port, 465);
        assert_eq!(back.smtp_security, "tls");
        assert!(back.wecom_webhook.contains("webhook"));
    }
}
