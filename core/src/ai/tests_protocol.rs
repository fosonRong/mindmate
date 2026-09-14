//! AI 协议层单元测试：Base URL 识别、请求体构造、SSE/响应解析
//!
//! 覆盖用户反馈的真实地址：
//!   智谱 GLM  Anthropic 兼容  https://open.bigmodel.cn/api/anthropic
//!   DeepSeek  Anthropic 兼容  https://api.deepseek.com/anthropic

use super::*;

fn cfg(base_url: &str, mode: &str) -> AiConfig {
    AiConfig {
        provider: "glm".into(),
        base_url: base_url.into(),
        model: "glm-4-flash".into(),
        temperature: 0.7,
        max_tokens: 1024,
        has_key: true,
        protocol_mode: mode.into(),
        detected_protocol: "auto".into(),
    }
}

// ── 协议识别 ──

#[test]
fn 协议识别_anthropic地址() {
    // 用户提供的两个地址都应识别为 Anthropic 兼容
    assert_eq!(
        detect_protocol("https://open.bigmodel.cn/api/anthropic", None),
        Protocol::Anthropic
    );
    assert_eq!(
        detect_protocol("https://api.deepseek.com/anthropic", None),
        Protocol::Anthropic
    );
    // 带斜杠结尾同样识别
    assert_eq!(
        detect_protocol("https://api.deepseek.com/anthropic/", Some("auto")),
        Protocol::Anthropic
    );
}

#[test]
fn 协议识别_openai兼容地址() {
    for url in [
        "https://open.bigmodel.cn/api/paas/v4",
        "https://api.deepseek.com",
        "https://api.openai.com/v1",
        "http://localhost:11434/v1",
    ] {
        assert_eq!(detect_protocol(url, None), Protocol::Openai, "{url}");
    }
}

#[test]
fn 协议识别_显式指定优先于自动识别() {
    // 即使地址含 /anthropic，显式 openai 也生效（用户可强制覆盖）
    assert_eq!(
        detect_protocol("https://api.deepseek.com/anthropic", Some("openai")),
        Protocol::Openai
    );
    assert_eq!(
        detect_protocol("https://api.deepseek.com", Some("anthropic")),
        Protocol::Anthropic
    );
}

// ── 请求地址拼接 ──

#[test]
fn 请求地址_openai协议追加chat_completions() {
    assert_eq!(
        endpoint_url("https://open.bigmodel.cn/api/paas/v4", Protocol::Openai),
        "https://open.bigmodel.cn/api/paas/v4/chat/completions"
    );
    assert_eq!(
        endpoint_url("https://api.deepseek.com/", Protocol::Openai),
        "https://api.deepseek.com/chat/completions"
    );
}

#[test]
fn 请求地址_anthropic协议追加v1messages() {
    assert_eq!(
        endpoint_url("https://open.bigmodel.cn/api/anthropic", Protocol::Anthropic),
        "https://open.bigmodel.cn/api/anthropic/v1/messages"
    );
    assert_eq!(
        endpoint_url("https://api.deepseek.com/anthropic/", Protocol::Anthropic),
        "https://api.deepseek.com/anthropic/v1/messages"
    );
}

// ── 请求体：两种协议的结构差异 ──

#[test]
fn 请求体_anthropic把system提为独立字段() {
    let c = cfg("https://api.deepseek.com/anthropic", "auto");
    let msgs = vec![
        ChatMsg::system("你是我的助手"),
        ChatMsg::user("今天做了什么"),
    ];
    let body = anthropic_body(&c, &msgs, true);
    assert_eq!(body["system"], "你是我的助手");
    assert_eq!(body["messages"].as_array().unwrap().len(), 1, "system 不应出现在 messages 中");
    assert_eq!(body["messages"][0]["role"], "user");
    assert_eq!(body["messages"][0]["content"], "今天做了什么");
    assert_eq!(body["stream"], true);
    assert!(body["max_tokens"].as_i64().unwrap() >= 1, "Anthropic 要求 max_tokens 必填");
}

#[test]
fn 请求体_openai保持messages内联system() {
    let c = cfg("https://api.deepseek.com", "auto");
    let msgs = vec![ChatMsg::system("sys"), ChatMsg::user("hi")];
    let body = openai_body(&c, &msgs, false);
    assert_eq!(body["messages"].as_array().unwrap().len(), 2);
    assert_eq!(body["messages"][0]["role"], "system");
    assert!(body.get("system").is_none());
    assert_eq!(body["stream"], false);
}

// ── 流式解析 ──

#[test]
fn 解析_openai增量帧() {
    let v: serde_json::Value =
        serde_json::from_str(r#"{"choices":[{"delta":{"content":"你好"}}]}"#).unwrap();
    assert_eq!(parse_sse_delta(&v).as_deref(), Some("你好"));
    // 空增量不产出
    let v: serde_json::Value =
        serde_json::from_str(r#"{"choices":[{"delta":{}}]}"#).unwrap();
    assert_eq!(parse_sse_delta(&v), None);
}

#[test]
fn 解析_anthropic增量帧() {
    let v: serde_json::Value = serde_json::from_str(
        r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"早上好"}}"#,
    )
    .unwrap();
    assert_eq!(parse_sse_delta(&v).as_deref(), Some("早上好"));
}

#[test]
fn 解析_无关帧返回空() {
    for raw in [
        r#"{"type":"message_start","message":{}}"#,
        r#"{"type":"message_stop"}"#,
        r#"{"type":"content_block_start","index":0}"#,
    ] {
        let v: serde_json::Value = serde_json::from_str(raw).unwrap();
        assert_eq!(parse_sse_delta(&v), None, "{raw}");
    }
}

// ── 非流式响应解析 ──

#[test]
fn 响应_openai结构取content() {
    let v: serde_json::Value =
        serde_json::from_str(r#"{"choices":[{"message":{"role":"assistant","content":"结果"}}]}"#)
            .unwrap();
    assert_eq!(extract_once_text(&v), "结果");
}

#[test]
fn 响应_anthropic结构拼接text块() {
    let v: serde_json::Value = serde_json::from_str(
        r#"{"id":"msg_1","content":[{"type":"text","text":"第一段"},{"type":"text","text":"第二段"}],"stop_reason":"end_turn"}"#,
    )
    .unwrap();
    assert_eq!(extract_once_text(&v), "第一段第二段");
    // 含非 text 块时忽略
    let v: serde_json::Value = serde_json::from_str(
        r#"{"content":[{"type":"thinking","thinking":"…"},{"type":"text","text":"答案"}]}"#,
    )
    .unwrap();
    assert_eq!(extract_once_text(&v), "答案");
}

// ── 预设与配置 ──

#[test]
fn 预设_包含用户提供的anthropic地址() {
    let list = presets();
    let glm = list.iter().find(|p| p.id == "glm").unwrap();
    let ds = list.iter().find(|p| p.id == "deepseek").unwrap();
    assert_eq!(glm.anthropic_url, "https://open.bigmodel.cn/api/anthropic");
    assert_eq!(ds.anthropic_url, "https://api.deepseek.com/anthropic");
    // 默认地址仍为 OpenAI 兼容端点（开箱即用）
    assert_eq!(glm.base_url, "https://open.bigmodel.cn/api/paas/v4");
    assert_eq!(ds.base_url, "https://api.deepseek.com");
}

#[test]
fn 配置_自定义地址保存后不被重置() {
    let db = Db::open_memory().unwrap();
    db.seed_defaults().unwrap();

    // 用户填写自定义地址（例如自建代理或 Anthropic 兼容端点）
    let custom = "https://my-proxy.internal/anthropic";
    save_config(&db, "glm", custom, "glm-4-flash", 0.7, 1024, Some("auto")).unwrap();
    let back = load_config(&db).unwrap();
    assert_eq!(back.base_url, custom);
    assert_eq!(back.detected_protocol, "anthropic", "自定义地址应被识别为 Anthropic 协议");

    // 仅切换模型（不改地址）→ 地址保持自定义值
    save_config(&db, "glm", &back.base_url, "glm-4-plus", 0.7, 1024, None).unwrap();
    let back2 = load_config(&db).unwrap();
    assert_eq!(back2.model, "glm-4-plus");
    assert_eq!(back2.base_url, custom, "切换模型不应恢复默认地址");

    // 切换到另一个提供商但保持地址 → 地址仍为自定义值
    save_config(&db, "deepseek", &back2.base_url, "deepseek-chat", 0.7, 1024, None).unwrap();
    let back3 = load_config(&db).unwrap();
    assert_eq!(back3.provider, "deepseek");
    assert_eq!(back3.base_url, custom, "切换提供商不应覆盖用户自定义地址");
}

#[test]
fn 配置_协议模式可显式覆盖并持久化() {
    let db = Db::open_memory().unwrap();
    db.seed_defaults().unwrap();
    save_config(&db, "glm", "https://api.deepseek.com/anthropic", "deepseek-chat", 0.7, 1024, Some("openai")).unwrap();
    let c = load_config(&db).unwrap();
    assert_eq!(c.protocol_mode, "openai");
    assert_eq!(c.detected_protocol, "openai", "显式模式应覆盖地址自动识别");
}

#[test]
fn 配置_地址前后空格被裁剪() {
    let db = Db::open_memory().unwrap();
    db.seed_defaults().unwrap();
    save_config(&db, "glm", "  https://api.deepseek.com/anthropic  ", " deepseek-chat ", 0.7, 1024, None).unwrap();
    let c = load_config(&db).unwrap();
    assert_eq!(c.base_url, "https://api.deepseek.com/anthropic");
    assert_eq!(c.model, "deepseek-chat");
}

// ── 温度序列化（真实 API 拒绝 f32 表示误差）──

#[test]
fn 温度_序列化不带浮点误差() {
    // 0.7f32 直接序列化会变成 0.699999988079071，被厂商拒绝
    assert_eq!(normalize_temperature(0.7), 0.7);
    assert_eq!(normalize_temperature(0.3), 0.3);
    assert_eq!(normalize_temperature(1.0), 1.0);
    assert_eq!(normalize_temperature(0.75), 0.75);
    // 越界收敛
    assert_eq!(normalize_temperature(-1.0), 0.0);
    assert_eq!(normalize_temperature(9.0), 2.0);
    assert_eq!(normalize_temperature(f32::NAN), 0.7);
}

#[test]
fn 温度_请求体中为两位小数() {
    let c = cfg("https://open.bigmodel.cn/api/paas/v4", "auto");
    let msgs = vec![ChatMsg::user("hi")];
    let body = openai_body(&c, &msgs, false);
    let text = body.to_string();
    assert!(text.contains("\"temperature\":0.7"), "实际请求体：{text}");
    assert!(!text.contains("0.6999"), "不应出现浮点误差：{text}");

    let a = anthropic_body(&c, &msgs, true);
    let atext = a.to_string();
    assert!(atext.contains("\"temperature\":0.7"), "实际请求体：{atext}");
}

// ── 问答本地检索范围（确定性验证，避免依赖模型措辞）──

fn d(s: &str) -> chrono::NaiveDate {
    chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
}

#[test]
fn 问答范围_明确日期走单日检索() {
    let today = d("2026-09-12");
    for q in ["我 2026-09-10 记了什么", "9月10日 的记录"] {
        let sc = qa_scope(q, today);
        assert_eq!(sc.single_day.as_deref(), Some("2026-09-10"), "{q}");
    }
    // 周三（2026-09-12 是周六 → 本周三为 09-09）
    let sc = qa_scope("周三我记了什么", today);
    assert_eq!(sc.single_day.as_deref(), Some("2026-09-09"), "周三");
}

#[test]
fn 问答范围_相对时间词() {
    let today = d("2026-09-12"); // 周六，本周 09-07 ~ 09-13
    let sc = qa_scope("昨天记了什么", today);
    assert_eq!((sc.from.as_str(), sc.to.as_str()), ("2026-09-11", "2026-09-11"));
    assert_eq!(sc.label, "昨天");

    let sc = qa_scope("本周完成了几件待办", today);
    assert_eq!((sc.from.as_str(), sc.to.as_str()), ("2026-09-07", "2026-09-12"));
    assert_eq!(sc.label, "本周");

    let sc = qa_scope("这个月有哪些进展", today);
    assert_eq!((sc.from.as_str(), sc.to.as_str()), ("2026-09-01", "2026-09-12"));
    assert_eq!(sc.label, "本月");
}

#[test]
fn 问答范围_无时间词默认最近七天() {
    let today = d("2026-09-12");
    let sc = qa_scope("我最近在忙什么", today);
    assert_eq!((sc.from.as_str(), sc.to.as_str()), ("2026-09-05", "2026-09-12"));
    assert!(sc.single_day.is_none());
    assert_eq!(sc.label, "最近 7 天");
}

// ── 模型配置引导（T1.6）──

#[test]
fn 预设_每个都给了领Key入口与是否必需Key() {
    for p in presets() {
        assert!(
            !p.key_url.is_empty(),
            "预设 {} 缺少 key_url（界面要引导用户去哪里领 Key）",
            p.id
        );
        assert!(
            p.key_url.starts_with("https://"),
            "预设 {} 的 key_url 必须是 https：{}",
            p.id,
            p.key_url
        );
        // 只有本地 Ollama 不需要 Key；其余必须要求填 Key，否则用户会以为不填也能用
        assert_eq!(
            p.requires_key,
            p.id != "ollama",
            "预设 {} 的 requires_key 判定不对",
            p.id
        );
        // 需要 Key 的必须给官方申请页，而不是下载页
        assert!(!p.base_url.is_empty() && !p.models.is_empty(), "预设 {} 信息不全", p.id);
    }
}

#[test]
fn 失败分类_按上游状态码给出可操作建议() {
    let up = |status: u16, body: &str| AiError::Upstream { status, body: body.into() };

    // 401/403 → Key 无效或无权限
    assert_eq!(classify(&up(401, "unauthorized")).0, FailKind::Auth);
    assert_eq!(classify(&up(403, "forbidden")).0, FailKind::Auth);
    // 402 → 余额不足
    assert_eq!(classify(&up(402, "insufficient balance")).0, FailKind::Quota);
    // 404 且不提模型 → 地址错（漏了 /v1 等）
    assert_eq!(classify(&up(404, "not found")).0, FailKind::Endpoint);
    // 404/400 且提到 model → 模型名不存在
    assert_eq!(classify(&up(404, "The model gpt-9 does not exist")).0, FailKind::Model);
    assert_eq!(
        classify(&up(400, r#"{"error":{"code":"model_not_found"}}"#)).0,
        FailKind::Model
    );
    // 429 → 限流
    assert_eq!(classify(&up(429, "rate limit")).0, FailKind::RateLimit);
    // 5xx 与其他 → 厂商侧异常
    assert_eq!(classify(&up(500, "internal")).0, FailKind::Upstream);
    assert_eq!(classify(&up(503, "unavailable")).0, FailKind::Upstream);
    // 网络 / 未配置 / 解析
    assert_eq!(classify(&AiError::Network("dns".into())).0, FailKind::Network);
    assert_eq!(classify(&AiError::NotConfigured).0, FailKind::NotConfigured);
    assert_eq!(classify(&AiError::Parse("html".into())).0, FailKind::Parse);
}

#[test]
fn 失败分类_保留上游状态码供界面显示() {
    let (kind, status) = classify(&AiError::Upstream { status: 401, body: "x".into() });
    assert_eq!(kind, FailKind::Auth);
    assert_eq!(status, Some(401));
    // 非上游错误没有状态码
    assert_eq!(classify(&AiError::Network("timeout".into())).1, None);
}

#[test]
fn 测试结论_成功带延迟失败带可读类别与原文() {
    let ok = TestVerdict::success("glm-4-flash", 321, "  你好，很高兴见到你  ");
    assert!(ok.ok);
    assert_eq!(ok.kind, "");
    assert_eq!(ok.latency_ms, 321);
    assert_eq!(ok.model, "glm-4-flash");
    assert_eq!(ok.reply, "你好，很高兴见到你"); // 已 trim

    let bad = TestVerdict::failure(
        &AiError::Upstream { status: 401, body: "invalid api key".into() },
        "glm-4-flash",
        88,
    );
    assert!(!bad.ok);
    assert_eq!(bad.kind, "auth");
    assert_eq!(bad.upstream_status, Some(401));
    assert!(bad.detail.contains("401") && bad.detail.contains("invalid api key"));
    assert!(bad.reply.is_empty());

    // 未配置也要有明确类别，界面据此提示「还没填 Key」
    let none = TestVerdict::failure(&AiError::NotConfigured, "glm-4-flash", 1);
    assert_eq!(none.kind, "not_configured");
    assert!(none.upstream_status.is_none());
}

#[test]
fn 预设_领Key入口是官方确认的管理页() {
    let find = |id: &str| presets().into_iter().find(|p| p.id == id).expect("预设缺失");
    // 真机踩过：智谱旧路径 /usercenter/apikeys 登录后会落到不存在的路由（页面 404），
    // 官方文档给的是 /usercenter/proj-mgmt/apikeys —— 别再改回去
    assert_eq!(
        find("glm").key_url,
        "https://bigmodel.cn/usercenter/proj-mgmt/apikeys"
    );
    assert_eq!(find("deepseek").key_url, "https://platform.deepseek.com/api_keys");
    assert_eq!(find("openai").key_url, "https://platform.openai.com/api-keys");
    assert_eq!(find("kimi").key_url, "https://platform.moonshot.cn/console/api-keys");
    assert_eq!(find("ollama").key_url, "https://ollama.com/download");
    // 百炼控制台用 hash 路由，直链要落到 API Key 页
    assert!(find("qwen").key_url.contains("/api-key"), "通义应直达 API Key 页");
}

// ── 晨间简报：必须汇总「今日记录 + 待办」（用户反馈：录入了却不在简报里）──

#[test]
fn 简报默认模板_包含今日记录与两类待办变量() {
    // 变量名写错会让模板渲染后残留 {{xxx}} 字面量，模型收到脏提示词，
    // 而界面看不出问题；所以这里逐项断言模板里有这些占位符。
    for var in ["{{date}}", "{{nodes}}", "{{todos}}", "{{today}}"] {
        assert!(
            DEFAULT_BRIEF.contains(var),
            "晨间简报模板缺少 {var}（用户要求：待办 + 今日记录汇总后润色）"
        );
    }
    assert!(
        DEFAULT_BRIEF.contains("今日进展") || DEFAULT_BRIEF.contains("已记录"),
        "要求里应说明如何呈现今日已记录内容"
    );
}

#[test]
fn 简报模板渲染_今日记录会真正进入提示词() {
    use crate::db::{Db, NewNode};

    let db = Db::open_memory().unwrap();
    db.migrate().unwrap();
    let node = db
        .create_node(NewNode {
            content: "编写北极星OAuth2授权接入千问办公方案".into(),
            date: Some("2026-09-14".into()),
            tags: vec!["工作".into()],
            todo_id: None,
        })
        .unwrap();
    let nodes = db.list_nodes_by_date("2026-09-14").unwrap();
    assert_eq!(nodes.len(), 1);

    let prompt = render_template(
        DEFAULT_BRIEF,
        &[
            ("date", "2026-09-14".into()),
            ("nodes", format_nodes(&nodes)),
            ("todos", "（无待办）".into()),
            ("today", "（无待办）".into()),
        ],
    );
    assert!(
        prompt.contains("编写北极星OAuth2授权接入千问办公方案"),
        "渲染后的提示词必须包含用户当天录入的内容"
    );
    assert!(!prompt.contains("{{nodes}}"), "占位符应已被替换，不能残留");
    // 记录格式化应带上时刻与标签（便于模型理解上下文）
    assert!(prompt.contains("[工作]"), "记录应带上标签：{prompt}");
    assert_eq!(node.tags, vec!["工作".to_string()]);
}

#[test]
fn 简报模板渲染_无记录时给出明确空值提示() {
    // 空列表不能变成空字符串：那会让模型看到「已记录内容：」后面什么都没有，容易编造
    let prompt = render_template(
        DEFAULT_BRIEF,
        &[
            ("date", "2026-09-14".into()),
            ("nodes", format_nodes(&[])),
            ("todos", "（无待办）".into()),
            ("today", "（无待办）".into()),
        ],
    );
    assert!(prompt.contains("（今日无记录）"), "空记录应有明确提示：{prompt}");
}

// ── 存量默认模板升级（v1.0.13 用户反馈：升级后简报仍用旧模板，看不到今日记录）──

#[test]
fn 模板升级_存量的旧默认会换成新默认() {
    use crate::db::Db;

    let db = Db::open_memory().unwrap();
    db.migrate().unwrap();
    // 模拟老用户：点过「保存模板/恢复默认」，库里固化了 V1 默认（无 {{nodes}}）
    let v1 = STOCK_TEMPLATES_HISTORY
        .iter()
        .find(|(k, _)| *k == "brief")
        .unwrap()
        .1[0];
    db.set_template("brief", v1).unwrap();
    assert!(!db.get_template("brief").unwrap().unwrap().contains("{{nodes}}"));

    let updated = upgrade_stock_templates(&db).unwrap();
    assert_eq!(updated, vec!["brief".to_string()], "应识别并升级 brief");
    let now = db.get_template("brief").unwrap().unwrap();
    assert!(now.contains("{{nodes}}"), "升级后应包含今日记录变量");
    // 幂等：再次启动不应重复报告
    assert!(upgrade_stock_templates(&db).unwrap().is_empty(), "已是最新不应再升级");
}

#[test]
fn 模板升级_用户真正自定义的内容绝不动() {
    use crate::db::Db;

    let db = Db::open_memory().unwrap();
    db.migrate().unwrap();
    let custom = "我的专属简报模板：{{nodes}} {{todos}} {{today}}，风格要简短。";
    db.set_template("brief", custom).unwrap();

    let updated = upgrade_stock_templates(&db).unwrap();
    assert!(updated.is_empty(), "自定义模板不应被升级：{updated:?}");
    assert_eq!(db.get_template("brief").unwrap().unwrap(), custom);
}

#[test]
fn 模板升级_历史默认清单与当前默认不同且可回溯() {
    // 防呆：清单里的"历史默认"绝不能等于当前默认（那样升级就是空转），
    // 且当前默认必须比历史版本多出新增能力（{{nodes}}）——锁住这次改进本身。
    let (_, history) = STOCK_TEMPLATES_HISTORY.iter().find(|(k, _)| *k == "brief").unwrap();
    let current = default_template("brief", crate::i18n::Lang::Zh);
    for h in *history {
        assert!(h.trim() != current.trim(), "历史默认不应与当前默认相同");
    }
    assert!(current.contains("{{nodes}}") && !history[0].contains("{{nodes}}"));
}

// ── 防围栏约束（真机踩过：模型把整份日报包在 ```markdown 里，界面显示源码）──

#[test]
fn 报告类模板_都带防围栏输出约束() {
    // 提示词约束只是降低概率，渲染层 unwrapMarkdownFence 才是兜底；两层都要在
    for (name, tpl) in [
        ("日报", DEFAULT_DAILY),
        ("周报", DEFAULT_WEEKLY),
        ("月报", DEFAULT_MONTHLY),
        ("简报", DEFAULT_BRIEF),
        ("晚安", DEFAULT_GOODNIGHT),
        ("复盘", DEFAULT_REVIEW),
    ] {
        assert!(
            tpl.contains("不要把整份内容包在"),
            "{name}模板缺少防围栏约束"
        );
    }
    // 历史清单里登记的旧版本不含该约束（这正是要升级它们的原因）
    for (kind, history) in STOCK_TEMPLATES_HISTORY {
        for h in *history {
            assert!(
                !h.contains("不要把整份内容包在"),
                "{kind} 的历史版本不应含新约束（它应该是改动前的旧版）"
            );
        }
    }
}
