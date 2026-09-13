//! AI 接入层：OpenAI 兼容客户端（流式）、双预设（智谱 GLM / DeepSeek）、
//! Prompt 模板渲染、脱敏、无 Key 本地模板降级。

use crate::db::{Db, Node, Report, Todo};
use anyhow::Result;
use chrono::{Datelike, Duration, Local, NaiveDate};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// 提供商预设
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Preset {
    pub id: &'static str,
    pub name: &'static str,
    /// 默认 Base URL（OpenAI 兼容端点）
    pub base_url: &'static str,
    /// 该提供商的 Anthropic 兼容端点（可作为自定义地址使用）
    pub anthropic_url: &'static str,
    pub models: Vec<&'static str>,
    pub note: &'static str,
    pub recommended: bool,
    pub free_model: Option<&'static str>,
}

/// 接口协议（由 Base URL 自动识别，也允许用户显式指定）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Protocol {
    /// OpenAI 兼容：POST {base}/chat/completions
    Openai,
    /// Anthropic 兼容：POST {base}/v1/messages
    Anthropic,
}

impl Protocol {
    pub fn as_str(&self) -> &'static str {
        match self {
            Protocol::Openai => "openai",
            Protocol::Anthropic => "anthropic",
        }
    }
}

/// 协议识别：
/// - 显式指定优先（设置项 ai_protocol = openai / anthropic）
/// - auto（默认）：地址包含 `/anthropic` → Anthropic 兼容，否则 OpenAI 兼容
pub fn detect_protocol(base_url: &str, override_protocol: Option<&str>) -> Protocol {
    match override_protocol.unwrap_or("auto").to_ascii_lowercase().as_str() {
        "openai" => Protocol::Openai,
        "anthropic" => Protocol::Anthropic,
        _ => {
            let u = base_url.to_ascii_lowercase();
            if u.contains("/anthropic") || u.contains("claude") {
                Protocol::Anthropic
            } else {
                Protocol::Openai
            }
        }
    }
}

/// 拼接请求地址（两种协议的路径规则不同）
pub fn endpoint_url(base_url: &str, protocol: Protocol) -> String {
    let base = base_url.trim_end_matches('/');
    match protocol {
        Protocol::Openai => {
            // 已带 /v1 的用户地址不再重复追加
            if base.ends_with("/v1") || base.ends_with("/v4") {
                format!("{base}/chat/completions")
            } else {
                format!("{base}/chat/completions")
            }
        }
        Protocol::Anthropic => format!("{base}/v1/messages"),
    }
}

pub fn presets() -> Vec<Preset> {
    vec![
        Preset {
            id: "glm",
            name: "智谱 GLM",
            base_url: "https://open.bigmodel.cn/api/paas/v4",
            anthropic_url: "https://open.bigmodel.cn/api/anthropic",
            models: vec!["glm-4-flash", "glm-4-air", "glm-4-plus"],
            note: "免费模型零成本跑通全部 AI 功能；国内直连快（OpenAI 兼容端点）",
            recommended: true,
            free_model: Some("glm-4-flash"),
        },
        Preset {
            id: "deepseek",
            name: "DeepSeek",
            base_url: "https://api.deepseek.com",
            anthropic_url: "https://api.deepseek.com/anthropic",
            models: vec!["deepseek-chat", "deepseek-reasoner"],
            note: "性价比标杆，长报告质量好（OpenAI 兼容端点）",
            recommended: true,
            free_model: None,
        },
        Preset {
            id: "openai",
            name: "OpenAI",
            base_url: "https://api.openai.com/v1",
            anthropic_url: "",
            models: vec!["gpt-4o-mini", "gpt-4o"],
            note: "国际/高质量需求",
            recommended: false,
            free_model: None,
        },
        Preset {
            id: "qwen",
            name: "通义千问",
            base_url: "https://dashscope.aliyuncs.com/compatible-mode/v1",
            anthropic_url: "https://dashscope.aliyuncs.com/api/v2/apps/claude-code-proxy",
            models: vec!["qwen-plus", "qwen-turbo"],
            note: "阿里云百炼",
            recommended: false,
            free_model: None,
        },
        Preset {
            id: "kimi",
            name: "Kimi",
            base_url: "https://api.moonshot.cn/v1",
            anthropic_url: "https://api.moonshot.cn/anthropic",
            models: vec!["moonshot-v1-8k", "moonshot-v1-32k"],
            note: "长上下文",
            recommended: false,
            free_model: None,
        },
        Preset {
            id: "ollama",
            name: "Ollama（本地）",
            base_url: "http://localhost:11434/v1",
            anthropic_url: "",
            models: vec!["llama3.2", "qwen2.5"],
            note: "完全离线，无需 Key",
            recommended: false,
            free_model: None,
        },
    ]
}

/// 运行时 AI 配置
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiConfig {
    pub provider: String,
    pub base_url: String,
    pub model: String,
    pub temperature: f32,
    pub max_tokens: i64,
    /// 是否已配置 Key（不返回 Key 本身）
    pub has_key: bool,
    /// 协议模式设置：auto（默认）/ openai / anthropic
    pub protocol_mode: String,
    /// 由 Base URL + 协议模式识别出的实际协议，供界面提示
    pub detected_protocol: String,
}

pub fn load_config(db: &Db) -> Result<AiConfig> {
    let get = |k: &str, d: &str| {
        db.get_setting(k)
            .ok()
            .flatten()
            .unwrap_or_else(|| d.to_string())
    };
    let has_key = crate::secrets::load_api_key().ok().flatten().is_some();
    let base_url = get("ai_base_url", "https://open.bigmodel.cn/api/paas/v4");
    let protocol_mode = get("ai_protocol", "auto");
    Ok(AiConfig {
        protocol_mode: protocol_mode.clone(),
        detected_protocol: detect_protocol(&base_url, Some(&protocol_mode))
            .as_str()
            .to_string(),
        provider: get("ai_provider", "glm"),
        base_url,
        model: get("ai_model", "glm-4-flash"),
        temperature: normalize_temperature(
            get("ai_temperature", "0.7").parse().unwrap_or(0.7),
        ) as f32,
        max_tokens: get("ai_max_tokens", "2048").parse().unwrap_or(2048),
        has_key,
    })
}

pub fn save_config(
    db: &Db,
    provider: &str,
    base_url: &str,
    model: &str,
    temperature: f32,
    max_tokens: i64,
    protocol_mode: Option<&str>,
) -> Result<()> {
    db.set_setting("ai_provider", provider)?;
    // Base URL 完全以用户输入为准（不因切换模型/提供商被重置）
    db.set_setting("ai_base_url", base_url.trim())?;
    db.set_setting("ai_model", model.trim())?;
    db.set_setting(
        "ai_temperature",
        &format!("{}", normalize_temperature(temperature)),
    )?;
    db.set_setting("ai_max_tokens", &max_tokens.to_string())?;
    if let Some(p) = protocol_mode {
        db.set_setting("ai_protocol", p)?;
    }
    Ok(())
}

/// 一条对话消息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMsg {
    pub role: String,
    pub content: String,
}

impl ChatMsg {
    pub fn system(c: impl Into<String>) -> Self {
        Self { role: "system".into(), content: c.into() }
    }
    pub fn user(c: impl Into<String>) -> Self {
        Self { role: "user".into(), content: c.into() }
    }
}

/// 可读错误
#[derive(Debug, thiserror::Error)]
pub enum AiError {
    #[error("未配置 AI 模型：请在「设置 → AI 模型」中选择预设并填入 API Key")]
    NotConfigured,
    #[error("AI 服务返回错误（{status}）：{body}")]
    Upstream { status: u16, body: String },
    #[error("网络请求失败：{0}")]
    Network(String),
    #[error("响应解析失败：{0}")]
    Parse(String),
}

/// 智伴问答的本地检索范围
#[derive(Debug, Clone, PartialEq)]
pub struct QaScope {
    /// 命中明确日期时只查该日
    pub single_day: Option<String>,
    /// 否则按范围检索
    pub from: String,
    pub to: String,
    /// 范围说明（"本周"/"本月"/"昨天"/"最近 7 天"/具体日期）
    pub label: String,
}

/// 从问句中解析明确日期（2026-09-12 / 9月12日 / 周三）
pub fn extract_date(q: &str, today: chrono::NaiveDate) -> Option<String> {
    use chrono::Datelike;
    use regex::Regex;
    if let Ok(re) = Regex::new(r"(\d{4})-(\d{1,2})-(\d{1,2})") {
        if let Some(c) = re.captures(q) {
            if let (Ok(y), Ok(m), Ok(d)) = (
                c[1].parse::<i32>(),
                c[2].parse::<u32>(),
                c[3].parse::<u32>(),
            ) {
                if let Some(date) = chrono::NaiveDate::from_ymd_opt(y, m, d) {
                    return Some(date.format("%Y-%m-%d").to_string());
                }
            }
        }
    }
    if let Ok(re) = Regex::new(r"(\d{1,2})月(\d{1,2})日") {
        if let Some(c) = re.captures(q) {
            if let (Ok(m), Ok(d)) = (c[1].parse::<u32>(), c[2].parse::<u32>()) {
                if let Some(date) = chrono::NaiveDate::from_ymd_opt(today.year(), m, d) {
                    return Some(date.format("%Y-%m-%d").to_string());
                }
            }
        }
    }
    // 周X → 本周内的该天（不晚于今天）
    let weekdays = ["周一", "周二", "周三", "周四", "周五", "周六", "周日"];
    for (i, name) in weekdays.iter().enumerate() {
        if q.contains(name) {
            let cur = today.weekday().num_days_from_monday() as i64;
            let diff = cur - i as i64;
            let date = today - chrono::Duration::days(diff);
            if date <= today {
                return Some(date.format("%Y-%m-%d").to_string());
            }
        }
    }
    None
}

/// 决定问答的检索范围（纯函数，便于测试）
pub fn qa_scope(question: &str, today: chrono::NaiveDate) -> QaScope {
    use chrono::Datelike;
    let q = question.to_lowercase();
    if let Some(day) = extract_date(question, today) {
        return QaScope {
            single_day: Some(day.clone()),
            from: day.clone(),
            to: day.clone(),
            label: day,
        };
    }
    let fmt = |d: chrono::NaiveDate| d.format("%Y-%m-%d").to_string();
    if q.contains("本周") || q.contains("这周") {
        let start = today - chrono::Duration::days(today.weekday().num_days_from_monday() as i64);
        return QaScope {
            single_day: None,
            from: fmt(start),
            to: fmt(today),
            label: "本周".into(),
        };
    }
    if q.contains("本月") || q.contains("这个月") {
        let start = chrono::NaiveDate::from_ymd_opt(today.year(), today.month(), 1).unwrap();
        return QaScope {
            single_day: None,
            from: fmt(start),
            to: fmt(today),
            label: "本月".into(),
        };
    }
    if q.contains("昨天") {
        let y = today - chrono::Duration::days(1);
        return QaScope {
            single_day: None,
            from: fmt(y),
            to: fmt(y),
            label: "昨天".into(),
        };
    }
    QaScope {
        single_day: None,
        from: fmt(today - chrono::Duration::days(7)),
        to: fmt(today),
        label: "最近 7 天".into(),
    }
}

/// 温度归一化：保留 2 位小数并限制在 [0, 2]
///
/// 修复：f32 → JSON 会暴露表示误差（0.7f32 序列化为 0.699999988079071），
/// 部分厂商（如智谱 GLM）会以「限制小数点[2]位」拒绝请求。
pub fn normalize_temperature(t: f32) -> f64 {
    let clamped = if t.is_finite() { t.clamp(0.0, 2.0) } else { 0.7 } as f64;
    (clamped * 100.0).round() / 100.0
}

/// 构造 OpenAI 兼容请求体（/chat/completions）
pub fn openai_body(cfg: &AiConfig, messages: &[ChatMsg], stream: bool) -> serde_json::Value {
    serde_json::json!({
        "model": cfg.model,
        "messages": messages,
        "temperature": normalize_temperature(cfg.temperature),
        "max_tokens": cfg.max_tokens,
        "stream": stream,
    })
}

/// 构造 Anthropic 兼容请求体（/v1/messages）
/// 协议差异：system 为独立字段、max_tokens 必填、messages 仅含 user/assistant
pub fn anthropic_body(cfg: &AiConfig, messages: &[ChatMsg], stream: bool) -> serde_json::Value {
    let mut system = String::new();
    let mut msgs: Vec<serde_json::Value> = Vec::new();
    for m in messages {
        if m.role == "system" {
            if !system.is_empty() {
                system.push_str("\n\n");
            }
            system.push_str(&m.content);
        } else {
            msgs.push(serde_json::json!({ "role": m.role, "content": m.content }));
        }
    }
    let mut body = serde_json::json!({
        "model": cfg.model,
        "max_tokens": cfg.max_tokens.max(1),
        "temperature": normalize_temperature(cfg.temperature),
        "messages": msgs,
        "stream": stream,
    });
    if !system.is_empty() {
        body["system"] = serde_json::Value::String(system);
    }
    body
}

/// 解析 SSE 增量文本（兼容 OpenAI 与 Anthropic 两种数据帧）
pub fn parse_sse_delta(v: &serde_json::Value) -> Option<String> {
    // OpenAI: choices[0].delta.content
    if let Some(s) = v["choices"][0]["delta"]["content"].as_str() {
        if !s.is_empty() {
            return Some(s.to_string());
        }
    }
    // Anthropic: content_block_delta → delta.text
    if let Some(s) = v["delta"]["text"].as_str() {
        if !s.is_empty() {
            return Some(s.to_string());
        }
    }
    None
}

/// Anthropic 错误帧：{"type":"error","error":{"message":"…"}}
fn anthropic_sse_error(v: &serde_json::Value) -> Option<String> {
    if v["type"].as_str() == Some("error") {
        return Some(
            v["error"]["message"]
                .as_str()
                .unwrap_or("Anthropic 兼容接口返回错误")
                .to_string(),
        );
    }
    None
}

/// 流式对话：按 Base URL 自动识别协议（OpenAI 兼容 / Anthropic 兼容）
pub async fn chat_stream(
    cfg: &AiConfig,
    messages: Vec<ChatMsg>,
    api_key: Option<String>,
) -> Result<impl futures_util::Stream<Item = Result<String, AiError>>, AiError> {
    if cfg.provider != "ollama" && api_key.is_none() {
        return Err(AiError::NotConfigured);
    }
    let protocol = detect_protocol(&cfg.base_url, Some(&cfg.protocol_mode));
    let url = endpoint_url(&cfg.base_url, protocol);
    let body = match protocol {
        Protocol::Openai => openai_body(cfg, &messages, true),
        Protocol::Anthropic => anthropic_body(cfg, &messages, true),
    };

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(300))
        .connect_timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(|e| AiError::Network(e.to_string()))?;

    let mut req = client.post(&url).json(&body);
    match protocol {
        Protocol::Openai => {
            if let Some(k) = api_key {
                req = req.bearer_auth(k);
            }
        }
        Protocol::Anthropic => {
            if let Some(k) = api_key {
                req = req.header("x-api-key", k);
            }
            req = req.header("anthropic-version", "2023-06-01");
        }
    }

    let resp = req.send().await.map_err(|e| AiError::Network(e.to_string()))?;
    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(AiError::Upstream {
            status: status.as_u16(),
            body: truncate(&body, 300),
        });
    }

    let mut buf = String::new();
    let stream = resp.bytes_stream().flat_map(move |chunk| {
        let mut out: Vec<Result<String, AiError>> = Vec::new();
        match chunk {
            Ok(bytes) => {
                buf.push_str(&String::from_utf8_lossy(&bytes));
                // 按行解析 SSE
                while let Some(idx) = buf.find('\n') {
                    let line = buf[..idx].trim().to_string();
                    buf.drain(..=idx);
                    if line.is_empty() || !line.starts_with("data:") {
                        continue;
                    }
                    let data = line.trim_start_matches("data:").trim();
                    if data == "[DONE]" {
                        continue;
                    }
                    if let Ok(v) = serde_json::from_str::<serde_json::Value>(data) {
                        if let Some(err) = anthropic_sse_error(&v) {
                            out.push(Err(AiError::Upstream { status: 200, body: err }));
                        } else if let Some(delta) = parse_sse_delta(&v) {
                            out.push(Ok(delta));
                        }
                    }
                }
            }
            Err(e) => out.push(Err(AiError::Network(e.to_string()))),
        }
        futures_util::stream::iter(out)
    });

    Ok(stream)
}

/// 非流式：用于「测试连接」
pub async fn chat_once(
    cfg: &AiConfig,
    messages: Vec<ChatMsg>,
    api_key: Option<String>,
) -> Result<String, AiError> {
    if cfg.provider != "ollama" && api_key.is_none() {
        return Err(AiError::NotConfigured);
    }
    let protocol = detect_protocol(&cfg.base_url, Some(&cfg.protocol_mode));
    let url = endpoint_url(&cfg.base_url, protocol);
    let body = match protocol {
        Protocol::Openai => openai_body(cfg, &messages, false),
        Protocol::Anthropic => anthropic_body(cfg, &messages, false),
    };
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .map_err(|e| AiError::Network(e.to_string()))?;
    let mut req = client.post(&url).json(&body);
    match protocol {
        Protocol::Openai => {
            if let Some(k) = api_key {
                req = req.bearer_auth(k);
            }
        }
        Protocol::Anthropic => {
            if let Some(k) = api_key {
                req = req.header("x-api-key", k);
            }
            req = req.header("anthropic-version", "2023-06-01");
        }
    }
    let resp = req.send().await.map_err(|e| AiError::Network(e.to_string()))?;
    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(AiError::Upstream {
            status: status.as_u16(),
            body: truncate(&text, 300),
        });
    }
    let v: serde_json::Value = serde_json::from_str(&text).map_err(|e| AiError::Parse(e.to_string()))?;
    Ok(extract_once_text(&v))
}

/// 从非流式响应提取文本（兼容 OpenAI 与 Anthropic 结构）
pub fn extract_once_text(v: &serde_json::Value) -> String {
    // OpenAI: choices[0].message.content
    if let Some(s) = v["choices"][0]["message"]["content"].as_str() {
        return s.to_string();
    }
    // Anthropic: content: [{ type: "text", text: "…" }]
    if let Some(arr) = v["content"].as_array() {
        let joined: String = arr
            .iter()
            .filter_map(|b| {
                if b["type"].as_str() == Some("text") {
                    b["text"].as_str()
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join("");
        if !joined.is_empty() {
            return joined;
        }
    }
    String::new()
}

/// AI 返回的 Token 用量（用于日志）
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Usage {
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
}

// ───────────────────────── 脱敏 ─────────────────────────

/// 默认脱敏规则（发送前过滤）
pub fn redact(input: &str) -> String {
    use regex::Regex;
    let mut out = input.to_string();
    let rules: Vec<(&str, &str)> = vec![
        (r"1[3-9]\d{9}", "[隐私]"),
        (r"\d{17}[\dXx]", "[隐私]"),
        (r"[\w.\-]+@[\w\-]+\.[A-Za-z]{2,}", "[隐私]"),
        (r"https?://[^\s]+", "[链接]"),
    ];
    for (pat, rep) in rules {
        if let Ok(re) = Regex::new(pat) {
            out = re.replace_all(&out, rep).to_string();
        }
    }
    out
}

// ───────────────────────── 数据 → Prompt 上下文组装 ─────────────────────────

/// 把节点格式化为 prompt 片段（按时刻顺序）
pub fn format_nodes(nodes: &[Node]) -> String {
    if nodes.is_empty() {
        return "（今日无记录）".into();
    }
    nodes
        .iter()
        .map(|n| {
            let time = n.created_at.get(11..16).unwrap_or("--:--");
            let tag = if n.tags.is_empty() {
                String::new()
            } else {
                format!(" [{}]", n.tags.join("/"))
            };
            format!("- {time}{tag} {}", redact(&n.content))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// 按日组织节点（周报/月报）
pub fn format_nodes_by_day(nodes: &[Node]) -> String {
    use std::collections::BTreeMap;
    let mut map: BTreeMap<&str, Vec<&Node>> = BTreeMap::new();
    for n in nodes {
        map.entry(n.date.as_str()).or_default().push(n);
    }
    if map.is_empty() {
        return "（该周期无记录）".into();
    }
    map.iter()
        .map(|(date, list)| {
            let items = list
                .iter()
                .map(|n| {
                    let time = n.created_at.get(11..16).unwrap_or("--:--");
                    format!("  · {time} {}", redact(&n.content))
                })
                .collect::<Vec<_>>()
                .join("\n");
            format!("## {date}\n{items}")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// 待办完成情况（已完成 / 进行中 / 逾期）
pub fn format_todos(todos: &[Todo]) -> String {
    if todos.is_empty() {
        return "（无待办）".into();
    }
    let done: Vec<&Todo> = todos.iter().filter(|t| t.status == "已完成").collect();
    let doing: Vec<&Todo> = todos
        .iter()
        .filter(|t| t.status == "进行中" || t.status == "待处理")
        .collect();
    let overdue: Vec<&Todo> = todos.iter().filter(|t| t.status == "已逾期").collect();

    let mut out = String::new();
    if !done.is_empty() {
        out.push_str("已完成：\n");
        for t in done {
            out.push_str(&format!("- {}", redact(&t.title)));
            if let Some(c) = &t.completed_at {
                out.push_str(&format!("（完成于 {}）", c.get(0..10).unwrap_or("")));
            }
            out.push('\n');
        }
    }
    if !doing.is_empty() {
        out.push_str("进行中/待处理：\n");
        for t in doing {
            out.push_str(&format!(
                "- {}{}{}\n",
                redact(&t.title),
                if t.priority != "中" {
                    format!("（优先级{}）", t.priority)
                } else {
                    String::new()
                },
                if t.due_date.is_empty() {
                    String::new()
                } else {
                    format!("（截止 {}）", t.due_date)
                }
            ));
        }
    }
    if !overdue.is_empty() {
        out.push_str("已逾期：\n");
        for t in overdue {
            out.push_str(&format!("- {}（截止 {}）\n", redact(&t.title), t.due_date));
        }
    }
    out
}

/// 进度描述
pub fn format_progress(
    node_count: i64,
    daily_goal: i64,
    goal_enabled: bool,
    todo_total: i64,
    todo_done: i64,
) -> String {
    format_progress_lang(
        node_count,
        daily_goal,
        goal_enabled,
        todo_total,
        todo_done,
        crate::i18n::Lang::Zh,
    )
}

/// 按语言生成进度文案（T1.4）
pub fn format_progress_lang(
    node_count: i64,
    daily_goal: i64,
    goal_enabled: bool,
    todo_total: i64,
    todo_done: i64,
    lang: crate::i18n::Lang,
) -> String {
    use crate::i18n::{tr_args, Lang};
    let record = if goal_enabled {
        match lang {
            Lang::Zh => tr_args(lang, "report.progress.recorded", &[("done", node_count.to_string()), ("goal", daily_goal.to_string())]),
            _ => tr_args(lang, "report.progress.recorded", &[("done", node_count.to_string()), ("goal", daily_goal.to_string())]),
        }
    } else {
        match lang {
            Lang::Zh => format!("记录 {node_count} 条"),
            Lang::En => format!("{node_count} logged"),
            Lang::Ja => format!("{node_count} 件記録"),
            Lang::Ko => format!("{node_count}건 기록"),
        }
    };
    let pct = if todo_total > 0 {
        (todo_done as f64 / todo_total as f64 * 100.0).round() as i64
    } else {
        0
    };
    format!("{record}；待办 {todo_done}/{todo_total}（完成率 {pct}%）")
}

// ───────────────────────── 模板渲染 ─────────────────────────

pub fn render_template(tpl: &str, vars: &[(&str, String)]) -> String {
    let mut out = tpl.to_string();
    for (k, v) in vars {
        out = out.replace(&format!("{{{{{k}}}}}"), v);
    }
    out
}

/// 输出语言指令：提示词正文保持中文（不改动既有输出结构），
/// 但明确要求模型**用目标语言撰写全部内容**，从而让 AI 报告跟随界面语言。
fn output_lang_directive(lang: crate::i18n::Lang) -> &'static str {
    match lang {
        crate::i18n::Lang::Zh => "",
        crate::i18n::Lang::En => {
            "\n\nIMPORTANT: Write the entire answer in English — including the title, all section headings and table headers."
        }
        crate::i18n::Lang::Ja => {
            "\n\n重要：タイトル・見出し・表のヘッダーを含め、出力はすべて日本語で書いてください。"
        }
        crate::i18n::Lang::Ko => {
            "\n\n중요: 제목, 모든 섹션 제목, 표 머리글을 포함해 전체 출력을 한국어로 작성하세요."
        }
    }
}

/// 按类型与语言取默认提示词（用户自定义过则用用户的，见 api 层）
pub fn default_template(kind: &str, lang: crate::i18n::Lang) -> String {
    let base = match kind {
        "weekly" => DEFAULT_WEEKLY,
        "monthly" => DEFAULT_MONTHLY,
        "brief" => DEFAULT_BRIEF,
        "goodnight" => DEFAULT_GOODNIGHT,
        "review" => DEFAULT_REVIEW,
        "qa" => DEFAULT_QA,
        _ => DEFAULT_DAILY,
    };
    let dir = output_lang_directive(lang);
    if dir.is_empty() {
        base.to_string()
    } else {
        format!("{base}{dir}")
    }
}

pub const DEFAULT_DAILY: &str = r#"你是我的个人工作助手。请根据以下今日记录，生成一份简洁的日报。

# 今日录入内容（按时刻顺序的节点）
{{nodes}}

# 今日待办完成情况
{{todos}}

# 今日进度
{{progress}}

# 要求
1. 输出 Markdown，标题为「{{date}} 日报」
2. 包含小节：今日完成、当前进度、遇到的问题（如无则省略）、明日计划（如无则说明）
3. 语言简洁、要点化，禁止编造未记录的内容"#;

pub const DEFAULT_WEEKLY: &str = r#"你是我的个人工作助手。请根据以下本周记录，生成一份周报。

# 本周每日录入内容
{{nodes}}

# 本周待办完成情况
{{todos}}

# 本周进度
{{progress}}

# 要求
1. 输出 Markdown，标题为「{{period}} 周报」
2. 包含小节：本周概述、每日要点、已完成待办清单、当前进度、下周计划
3. 简洁要点化，禁止编造未记录的内容"#;

pub const DEFAULT_MONTHLY: &str = r#"你是我的个人工作助手。请根据以下本月记录，生成一份月报。

# 本月每日录入内容
{{nodes}}

# 本月待办完成情况
{{todos}}

# 本月进度
{{progress}}

# 要求
1. 输出 Markdown，标题为「{{period}} 月报」
2. 包含小节：本月概述、分周要点、已完成待办清单、当前进度、下月计划
3. 简洁要点化，禁止编造未记录的内容"#;

pub const DEFAULT_BRIEF: &str = r#"你是我的个人工作助手。请生成今天的晨间简报。

# 今日日期
{{date}}

# 昨日未完成的待办
{{todos}}

# 今日日程与待办
{{today}}

# 要求
1. 输出 Markdown，标题「☀️ 今日简报」
2. 包含：昨日遗留、今日重点（按优先级给出 Top3 建议顺序）、一句话鼓励
3. 不超过 200 字，不编造内容"#;

pub const DEFAULT_GOODNIGHT: &str = r#"你是我的个人工作助手。请生成今天的晚安总结。

# 今日录入内容
{{nodes}}

# 今日待办
{{todos}}

# 要求
1. 输出 Markdown，标题「🌙 晚安总结」
2. 包含：今天完成了什么、没完成什么、今天值得记一笔的事、明日建议
3. 温和、克制，不超过 200 字，不编造内容"#;

pub const DEFAULT_REVIEW: &str = r#"你是我的个人数据分析师。请根据以下本周数据，输出周度复盘洞察。

# 本周每日记录量
{{days}}

# 本周待办明细
{{todos}}

# 本周进度
{{progress}}

# 要求
1. 输出 Markdown，标题「📊 本周复盘」
2. 给出 3-5 条洞察，每条一句话结论 + 一行支持数据（如"周三逾期率最高 43%"）
3. 最后给出 2-3 条下周改进建议
4. 只基于给定数据，不编造"#;

pub const DEFAULT_QA: &str = r#"你是智伴，用户的个人工作生活助手。根据下面的本地数据回答用户问题。

# 相关数据（来自用户自己的记录）
{{context}}

# 要求
1. 只依据上面的数据回答，数据里没有的信息要明确说"记录里没有相关内容"
2. 简洁、具体，可引用日期与时刻
3. 用中文回答"#;

// ───────────────────────── 无 Key 本地降级（模板拼装） ─────────────────────────

pub fn fallback_report(db: &Db, rtype: &str, date: &str) -> Result<String> {
    fallback_report_lang(db, rtype, date, crate::i18n::Lang::Zh)
}

/// 带语言的降级报告（T1.4）：未配置 AI 时用户看到的本地模板内容
pub fn fallback_report_lang(
    db: &Db,
    rtype: &str,
    date: &str,
    lang: crate::i18n::Lang,
) -> Result<String> {
    use crate::i18n::{tr, tr_args};
    match rtype {
        "daily" => {
            let nodes = db.list_nodes_by_date(date)?;
            let (_, todos) = db.schedule_for_date(date)?;
            let stats = db.daily_stats(date)?;
            let mut md = format!(
                "# {date} {}\n\n> {}\n\n## {}\n\n",
                tr(lang, "report.type.daily"),
                tr(lang, "report.notice"),
                tr(lang, "report.h.today_done")
            );
            if nodes.is_empty() {
                md.push_str(&format!("- {}\n", tr(lang, "report.empty.today")));
            } else {
                for n in &nodes {
                    let t = n.created_at.get(11..16).unwrap_or("--:--");
                    md.push_str(&format!("- **{t}** {}\n", n.content));
                }
            }
            md.push_str(&format!(
                "\n## {}\n\n- {}\n- {}\n",
                tr(lang, "report.h.progress"),
                format_progress_lang(
                    stats.node_count,
                    stats.daily_goal,
                    stats.goal_enabled,
                    stats.total_todos,
                    stats.done_todos,
                    lang
                ),
                tr_args(
                    lang,
                    "report.todos_done",
                    &[("done", stats.today_done_todos.to_string()), ("all", stats.today_todos.to_string())]
                )
            ));
            md.push_str(&format!("\n## {}\n\n", tr(lang, "report.h.today_todos")));
            if todos.is_empty() {
                md.push_str(&format!("- {}\n", tr(lang, "report.empty.none")));
            } else {
                for t in &todos {
                    let mark = if t.status == "已完成" { "x" } else { " " };
                    md.push_str(&format!("- [{mark}] {}\n", t.title));
                }
            }
            if stats.overdue_todos > 0 {
                md.push_str(&format!(
                    "\n## {}\n\n- {}\n",
                    tr(lang, "report.h.attention"),
                    tr_args(lang, "report.overdue_line", &[("n", stats.overdue_todos.to_string())])
                ));
            }
            Ok(md)
        }
        _ => {
            let (from, to, label) = period_range_lang(rtype, date, lang);
            let stats = db.period_stats(&from, &to)?;
            let nodes = db.list_nodes_range(&from, &to)?;
            let title = tr(lang, if rtype == "weekly" { "report.type.weekly" } else { "report.type.monthly" });
            let mut md = format!(
                "# {label} {title}\n\n> {}\n\n## {}\n\n- {}\n- {}\n\n## {}\n\n",
                tr(lang, "report.notice"),
                tr(lang, "report.h.overview"),
                tr_args(
                    lang,
                    "report.summary_line",
                    &[
                        ("nodes", stats.node_count.to_string()),
                        ("days", stats.days_with_records.to_string()),
                        ("total", stats.total_days.to_string())
                    ]
                ),
                tr_args(
                    lang,
                    "report.todos_done",
                    &[("done", stats.done_todos.to_string()), ("all", stats.total_todos.to_string())]
                ),
                tr(lang, "report.h.by_day")
            );
            if nodes.is_empty() {
                md.push_str(&format!("{}\n", tr(lang, "report.empty.period")));
            } else {
                md.push_str(&format_nodes_by_day(&nodes));
                md.push('\n');
            }
            Ok(md)
        }
    }
}

/// 计算周期起止（日/周/月）
pub fn period_range(rtype: &str, date: &str) -> (String, String, String) {
    period_range_lang(rtype, date, crate::i18n::Lang::Zh)
}

/// 带语言的周期标签（周/月）
pub fn period_range_lang(
    rtype: &str,
    date: &str,
    lang: crate::i18n::Lang,
) -> (String, String, String) {
    let d = NaiveDate::parse_from_str(date, "%Y-%m-%d").unwrap_or_else(|_| Local::now().date_naive());
    match rtype {
        "weekly" => {
            let weekday = d.weekday().num_days_from_monday() as i64;
            let start = d - Duration::days(weekday);
            let end = start + Duration::days(6);
            let iso_week = d.iso_week();
            (
                start.format("%Y-%m-%d").to_string(),
                end.format("%Y-%m-%d").to_string(),
                crate::i18n::tr_args(
                    lang,
                    "report.week_label",
                    &[("year", iso_week.year().to_string()), ("week", iso_week.week().to_string())],
                ),
            )
        }
        "monthly" => {
            let start = NaiveDate::from_ymd_opt(d.year(), d.month(), 1).unwrap();
            let next = if d.month() == 12 {
                NaiveDate::from_ymd_opt(d.year() + 1, 1, 1).unwrap()
            } else {
                NaiveDate::from_ymd_opt(d.year(), d.month() + 1, 1).unwrap()
            };
            let end = next - Duration::days(1);
            (
                start.format("%Y-%m-%d").to_string(),
                end.format("%Y-%m-%d").to_string(),
                match lang {
                    crate::i18n::Lang::Zh => format!("{}年{}月", d.year(), d.month()),
                    crate::i18n::Lang::Ja => format!("{}年{}月", d.year(), d.month()),
                    crate::i18n::Lang::Ko => format!("{}년 {}월", d.year(), d.month()),
                    crate::i18n::Lang::En => format!("{:02}/{}", d.month(), d.year()),
                },
            )
        }
        _ => (
            date.to_string(),
            date.to_string(),
            date.to_string(),
        ),
    }
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(n).collect::<String>())
    }
}

/// 生成报告的统一入口（含降级）：返回 (内容, 是否 AI)
pub async fn generate_report(
    db: &Arc<Db>,
    rtype: &str,
    date: &str,
) -> Result<(String, bool)> {
    let cfg = load_config(db)?;
    if !cfg.has_key && cfg.provider != "ollama" {
                let lang = crate::i18n::Lang::from_setting(
            &db.get_setting("ui_locale").ok().flatten().unwrap_or_default(),
        );
        return Ok((fallback_report_lang(db, rtype, date, lang)?, false));
    }
    let (from, to, label) = period_range(rtype, date);
    let vars: Vec<(&str, String)> = match rtype {
        "daily" => {
            let nodes = db.list_nodes_by_date(date)?;
            let (_, todos) = db.schedule_for_date(date)?;
            let stats = db.daily_stats(date)?;
            vec![
                ("date", date.to_string()),
                ("nodes", format_nodes(&nodes)),
                ("todos", format_todos(&todos)),
                (
                    "progress",
                    format_progress(
                        stats.node_count,
                        stats.daily_goal,
                        stats.goal_enabled,
                        stats.total_todos,
                        stats.done_todos,
                    ),
                ),
            ]
        }
        _ => {
            let nodes = db.list_nodes_range(&from, &to)?;
            let todos = db.list_todos(Some("全部"), Some("全部"), None, None, None)?;
            let period_todos: Vec<Todo> = todos
                .into_iter()
                .filter(|t| t.due_date >= from && t.due_date <= to)
                .collect();
            let stats = db.period_stats(&from, &to)?;
            vec![
                ("period", label.clone()),
                ("from", from.clone()),
                ("to", to.clone()),
                ("nodes", format_nodes_by_day(&nodes)),
                ("todos", format_todos(&period_todos)),
                (
                    "progress",
                    format!(
                        "记录 {} 条，覆盖 {} / {} 天；待办完成 {}/{}",
                        stats.node_count,
                        stats.days_with_records,
                        stats.total_days,
                        stats.done_todos,
                        stats.total_todos
                    ),
                ),
            ]
        }
    };

    let tpl_key = match rtype {
        "weekly" => "weekly",
        "monthly" => "monthly",
        _ => "daily",
    };
    let tpl = db
        .get_template(tpl_key)?
        .unwrap_or_else(|| match tpl_key {
            "weekly" => default_template("weekly", crate::i18n::Lang::Zh),
            "monthly" => default_template("monthly", crate::i18n::Lang::Zh),
            _ => default_template("daily", crate::i18n::Lang::Zh),
        });
    let prompt = render_template(&tpl, &vars);
    let key = crate::secrets::load_api_key()?;
    let content = chat_once(&cfg, vec![ChatMsg::user(prompt)], key)
        .await
        .map_err(|e| anyhow::anyhow!(e.to_string()))?;
    // 保存报告
    let period = match rtype {
        "weekly" | "monthly" => label.clone(),
        _ => date.to_string(),
    };
    db.save_report(rtype, &period, &content, true)?;
    Ok((content, true))
}

/// 报告存档（供 API 调用）
pub fn archive_report(db: &Db, rtype: &str, period: &str, content: &str, is_ai: bool) -> Result<Report> {
    db.save_report(rtype, period, content, is_ai)
}

#[cfg(test)]
mod tests_protocol;
