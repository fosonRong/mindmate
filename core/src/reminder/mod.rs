//! 提醒系统：服务端调度器 + 上下文智能提醒（L1 纯规则）+ 待办提醒 + 睡眠唤醒补偿
//!
//! 设计要点（技术设计文档 §6）：
//! - 定时 tick（默认 20s）检查是否到点；记录提醒按「间隔 + 生效时段」判断
//! - 上下文规则：已达标→静默；有逾期→加压；连续多日未记→关怀
//! - 睡眠/休眠后 tick 间隔异常增大 → 聚合补偿一条
//! - 所有提醒经事件总线广播（SSE），桌面端订阅后弹系统通知

use crate::{AppContext, Event};
use chrono::{Duration, Local, NaiveDate, NaiveDateTime, Timelike};
use serde_json::json;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;
use tokio::time::{interval, Duration as TokioDuration};

/// 提醒类型
#[derive(Debug, Clone, PartialEq)]
pub enum ReminderKind {
    /// 常规记录提醒
    Record,
    /// 逾期加压
    Overdue,
    /// 关怀（多日未记录）
    Care,
    /// 待办到期
    Todo,
    /// 补偿（唤醒后）
    Makeup,
    /// 晨间简报
    Brief,
    /// 晚安总结
    Goodnight,
}

impl ReminderKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            ReminderKind::Record => "record",
            ReminderKind::Overdue => "overdue",
            ReminderKind::Care => "care",
            ReminderKind::Todo => "todo",
            ReminderKind::Makeup => "makeup",
            ReminderKind::Brief => "brief",
            ReminderKind::Goodnight => "goodnight",
        }
    }
    /// 是否压缩（静默类）
    pub fn is_quiet(&self) -> bool {
        false
    }
}

/// 一次提醒决策
#[derive(Debug, Clone)]
pub struct Notification {
    pub kind: ReminderKind,
    pub title: String,
    pub body: String,
    /// 点击行为：quick_entry | open_todos | open_today
    pub action: String,
    pub sound: bool,
}

/// 是否处于提醒生效时段内
///
/// 支持跨零点时段（如夜猫子设 22:00–02:00）：start > end 表示时段跨越午夜
pub fn in_window(now_minutes: i64, start: i64, end: i64) -> bool {
    if start <= end {
        now_minutes >= start && now_minutes <= end
    } else {
        now_minutes >= start || now_minutes <= end
    }
}

/// 上下文规则：根据当前状态决定是否提醒、用哪种文案
pub fn decide(
    now_minutes: i64,
    window: (i64, i64),
    last_fired_minutes_ago: i64,
    freq_minutes: i64,
    node_count_today: i64,
    daily_goal: i64,
    goal_enabled: bool,
    streak: i64,
    overdue_titles: &[String],
    dnd_now: bool,
) -> Option<ReminderKind> {
    if dnd_now {
        return None;
    }
    // 生效时段（支持跨零点）
    if !in_window(now_minutes, window.0, window.1) {
        return None;
    }
    // 频率未到
    if last_fired_minutes_ago < freq_minutes {
        return None;
    }
    // 规则 ①：已达目标 → 静默
    if goal_enabled && node_count_today >= daily_goal && daily_goal > 0 {
        return None;
    }
    // 规则 ③：连续多日未记录 → 关怀（streak == 0 表示今天和昨天都没记）
    if streak == 0 {
        return Some(ReminderKind::Care);
    }
    // 规则 ②：有逾期待办 → 加压
    if !overdue_titles.is_empty() {
        return Some(ReminderKind::Overdue);
    }
    Some(ReminderKind::Record)
}

/// 生成提醒文案（对应需求文档 6.4 文案表）
pub fn compose(
    kind: &ReminderKind,
    node_count: i64,
    daily_goal: i64,
    overdue: &[String],
    lang: crate::i18n::Lang,
) -> Notification {
    use crate::i18n::{tr, tr_args};
    match kind {
        ReminderKind::Record => Notification {
            kind: kind.clone(),
            title: tr(lang, "rem.record.title"),
            body: tr_args(lang, "rem.record.body", &[("done", node_count.to_string()), ("goal", daily_goal.to_string())]),
            action: "quick_entry".into(),
            sound: true,
        },
        ReminderKind::Overdue => {
            let first = overdue.first().cloned().unwrap_or_default();
            let more = if overdue.len() > 1 {
                tr_args(lang, "rem.overdue.more", &[("n", overdue.len().to_string())])
            } else {
                String::new()
            };
            Notification {
                kind: kind.clone(),
                title: tr_args(lang, "rem.overdue.title", &[("n", overdue.len().to_string())]),
                body: tr_args(lang, "rem.overdue.body", &[("first", first), ("more", more)]),
                action: "quick_entry".into(),
                sound: true,
            }
        }
        ReminderKind::Care => Notification {
            kind: kind.clone(),
            title: tr(lang, "rem.care.title"),
            body: tr(lang, "rem.care.body"),
            action: "quick_entry".into(),
            sound: false,
        },
        ReminderKind::Makeup => Notification {
            kind: kind.clone(),
            title: tr(lang, "rem.makeup.title"),
            body: tr_args(lang, "rem.makeup.body", &[("done", node_count.to_string()), ("goal", daily_goal.to_string())]),
            action: "quick_entry".into(),
            sound: true,
        },
        ReminderKind::Todo => Notification {
            kind: kind.clone(),
            title: tr(lang, "rem.todo.title"),
            body: overdue.first().cloned().unwrap_or_else(|| tr(lang, "rem.todo.fallback")),
            action: "open_todos".into(),
            sound: true,
        },
        ReminderKind::Brief => Notification {
            kind: kind.clone(),
            title: tr(lang, "rem.brief.title"),
            body: tr(lang, "rem.brief.body"),
            action: "open_today".into(),
            sound: false,
        },
        ReminderKind::Goodnight => Notification {
            kind: kind.clone(),
            title: tr(lang, "rem.goodnight.title"),
            body: tr(lang, "rem.goodnight.body"),
            action: "open_today".into(),
            sound: false,
        },
    }
}

/// 解析 "HH:MM" → 当日分钟数
fn parse_hhmm(s: &str) -> Option<i64> {
    let (h, m) = s.split_once(':')?;
    let h: i64 = h.trim().parse().ok()?;
    let m: i64 = m.trim().parse().ok()?;
    Some(h * 60 + m)
}

fn now_minutes() -> i64 {
    let now = Local::now();
    now.hour() as i64 * 60 + now.minute() as i64
}

/// 勿扰规则匹配（纯函数，便于测试）
///
/// - 支持跨零点时段（与提醒生效时段一致的处理）
/// - `date` 非空时表示仅在该日生效（一次性勿扰）
pub fn in_dnd_rules(now_m: i64, rules: &[serde_json::Value], today: &str) -> bool {
    for r in rules {
        let Some(start) = r["start"].as_str().and_then(parse_hhmm) else {
            continue;
        };
        let Some(end) = r["end"].as_str().and_then(parse_hhmm) else {
            continue;
        };
        // 一次性规则：仅在指定日期生效
        if let Some(date) = r["date"].as_str() {
            if !date.is_empty() && date != today {
                continue;
            }
        }
        if in_window(now_m, start, end) {
            return true;
        }
    }
    false
}

/// 当前是否处于勿扰规则内
fn in_dnd(db: &crate::db::Db) -> bool {
    let rules = db
        .get_setting("dnd_rules")
        .ok()
        .flatten()
        .unwrap_or_else(|| "[]".into());
    let Ok(list) = serde_json::from_str::<Vec<serde_json::Value>>(&rules) else {
        return false;
    };
    let today = Local::now().date_naive().format("%Y-%m-%d").to_string();
    in_dnd_rules(now_minutes(), &list, &today)
}

/// 启动调度器（后台任务）
pub fn spawn(ctx: Arc<AppContext>) {
    // 上次记录提醒触发的时间戳（分钟级，用于频率判断）
    let last_record_fire = Arc::new(AtomicI64::new(0));
    let last_tick = Arc::new(AtomicI64::new(Local::now().timestamp()));
    let last_brief_day = Arc::new(std::sync::Mutex::new(String::new()));
    let last_goodnight_day = Arc::new(std::sync::Mutex::new(String::new()));

    tokio::spawn(async move {
        let mut ticker = interval(TokioDuration::from_secs(20));
        let mut rx = ctx.bus.subscribe();
        loop {
            tokio::select! {
                _ = ticker.tick() => {
                    if let Err(e) = tick(&ctx, &last_record_fire, &last_tick, &last_brief_day, &last_goodnight_day).await {
                        tracing::warn!("提醒调度 tick 失败: {e}");
                    }
                }
                Ok(ev) = rx.recv() => {
                    // 提醒相关设置变更后立即按新规则重新评估，无需等待旧频率窗口
                    if ev.kind == "settings.updated" && is_reminder_related(&ev.payload) {
                        last_record_fire.store(0, Ordering::SeqCst);
                        tracing::debug!("提醒设置变更，重置提醒计时");
                    }
                }
            }
        }
    });
}

async fn tick(
    ctx: &Arc<AppContext>,
    last_record_fire: &Arc<AtomicI64>,
    last_tick: &Arc<AtomicI64>,
    last_brief_day: &Arc<std::sync::Mutex<String>>,
    last_goodnight_day: &Arc<std::sync::Mutex<String>>,
) -> anyhow::Result<()> {
    let db = &ctx.db;
    let now_ts = Local::now().timestamp();
    let prev_ts = last_tick.swap(now_ts, Ordering::SeqCst);
    let gap_secs = (now_ts - prev_ts).max(0);

    // 1) 刷新逾期状态（会产生 todo 状态变更事件）
    let overdue_todos = db.refresh_overdue()?;
    let overdue_titles: Vec<String> = overdue_todos.iter().map(|t| t.title.clone()).collect();

    // 2) 待办到点提醒
    let now_str = Local::now().format("%Y-%m-%d %H:%M").to_string();
    let due = db.due_todo_reminders(&now_str)?;
    let todo_remind_enabled = db
        .get_setting("todo_remind_enabled")?
        .map(|v| v == "1")
        .unwrap_or(true);
    if todo_remind_enabled && !due.is_empty() && !in_dnd(db) {
        for t in &due {
            let n = Notification {
                kind: ReminderKind::Todo,
                title: format!("待办即将到期：{}", t.title),
                body: if let Some(dt) = &t.due_time {
                    format!("截止 {dt}，点击查看")
                } else {
                    "点击查看待办".into()
                },
                action: "open_todos".into(),
                sound: true,
            };
            publish(ctx, &n);
            db.mark_todo_reminded(t.id)?;
        }
    }

    // 3) 睡眠唤醒补偿：tick 间隔异常增大（>2 倍 tick 且 >2 分钟）
    let freq_minutes: i64 = db
        .get_setting("remind_freq_minutes")?
        .and_then(|v| v.parse().ok())
        .unwrap_or(60);
    let remind_enabled = db
        .get_setting("remind_enabled")?
        .map(|v| v == "1")
        .unwrap_or(true);

    if remind_enabled && should_compensate(gap_secs, freq_minutes) {
        let stats = db.daily_stats(&crate::db::today_string())?;
        let missed = (gap_secs / (freq_minutes * 60)).max(1);
        let goal = stats.daily_goal;
        let reached = stats.goal_enabled && stats.node_count >= goal && goal > 0;
        if !reached && !in_dnd(db) {
            let lang = ctx.lang();
            let mut n = compose(&ReminderKind::Makeup, stats.node_count, goal, &overdue_titles, lang);
            n.body = makeup_body(missed, stats.node_count, goal, lang);
            publish(ctx, &n);
        }
        last_record_fire.store(now_minutes(), Ordering::SeqCst);
    }

    // 4) 记录提醒（上下文智能）
    if remind_enabled {
        let window = (
            db.get_setting("remind_window_start")?
                .as_deref()
                .and_then(parse_hhmm)
                .unwrap_or(9 * 60),
            db.get_setting("remind_window_end")?
                .as_deref()
                .and_then(parse_hhmm)
                .unwrap_or(21 * 60),
        );
        let stats = db.daily_stats(&crate::db::today_string())?;
        let last = last_record_fire.load(Ordering::SeqCst);
        let ago = if last == 0 { i64::MAX } else { now_minutes() - last };
        let dnd = in_dnd(db);

        tracing::debug!(
            "记录提醒评估: now={} window={:?} ago={} freq={} 已录={} 目标={}(启用={}) streak={} 逾期={} 勿扰={}",
            now_minutes(), window, ago, freq_minutes, stats.node_count, stats.daily_goal,
            stats.goal_enabled, stats.streak_days, overdue_titles.len(), dnd
        );
        if let Some(kind) = decide(
            now_minutes(),
            window,
            ago,
            freq_minutes,
            stats.node_count,
            stats.daily_goal,
            stats.goal_enabled,
            stats.streak_days,
            &overdue_titles,
            dnd,
        ) {
            let n = compose(&kind, stats.node_count, stats.daily_goal, &overdue_titles, ctx.lang());
            publish(ctx, &n);
            last_record_fire.store(now_minutes(), Ordering::SeqCst);
        }
    }

    // 5) 晨间简报 / 晚安总结（到点触发一次，由前端拉起 AI 生成）
    let today = crate::db::today_string();
    let now_m = now_minutes();
    let brief_enabled = db.get_setting("smart_brief_enabled")?.map(|v| v == "1").unwrap_or(true);
    let brief_at: i64 = db.get_setting("brief_minutes")?.and_then(|v| v.parse().ok()).unwrap_or(540);
    if brief_enabled && now_m >= brief_at {
        let mut guard = last_brief_day.lock().unwrap();
        if *guard != today {
            *guard = today.clone();
            drop(guard);
            if !db.latest_report("brief", &today)?.is_some() {
                publish(ctx, &compose(&ReminderKind::Brief, 0, 0, &[], ctx.lang()));
            }
        }
    }

    let goodnight_enabled = db.get_setting("goodnight_enabled")?.map(|v| v == "1").unwrap_or(true);
    let gn_at: i64 = db.get_setting("goodnight_minutes")?.and_then(|v| v.parse().ok()).unwrap_or(1290);
    if goodnight_enabled && now_m >= gn_at {
        let mut guard = last_goodnight_day.lock().unwrap();
        if *guard != today {
            *guard = today.clone();
            drop(guard);
            if !db.latest_report("goodnight", &today)?.is_some() {
                publish(ctx, &compose(&ReminderKind::Goodnight, 0, 0, &[], ctx.lang()));
            }
        }
    }

    Ok(())
}

fn publish(ctx: &Arc<AppContext>, n: &Notification) {
    tracing::info!("提醒触发 [{}] {}", n.kind.as_str(), n.title);
    ctx.bus.publish(Event::new(
        "reminder.triggered",
        json!({
            "kind": n.kind.as_str(),
            "title": n.title,
            "body": n.body,
            "action": n.action,
            "sound": n.sound,
        }),
    ));

    // 远程推送渠道（邮件 / Telegram / 企业微信）：异步投递，不阻塞调度
    let cfg = crate::push::load_config(&ctx.db);
    if !cfg.channels.is_empty() {
        let msg = crate::push::OutgoingMessage {
            title: n.title.clone(),
            body: n.body.clone(),
            kind: n.kind.as_str().to_string(),
            at: Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
        };
        let db = ctx.db.clone();
        tokio::spawn(async move {
            let results = crate::push::dispatch(&db, &msg).await;
            for r in results {
                if r.ok {
                    tracing::info!("推送成功 [{}] {}", r.channel, r.detail);
                }
            }
        });
    }
}

/// 睡眠/休眠唤醒补偿判定（纯函数，便于测试）
///
/// 规则：tick 间隔超过 2 分钟且大于半个提醒周期 → 视为系统休眠后恢复，需要聚合补偿
pub fn should_compensate(gap_secs: i64, freq_minutes: i64) -> bool {
    gap_secs > 150 && gap_secs > freq_minutes * 60 / 2
}

/// 补偿提醒的聚合文案（FR-4.8）
pub fn makeup_body(missed: i64, node_count: i64, daily_goal: i64, lang: crate::i18n::Lang) -> String {
    crate::i18n::tr_args(
        lang,
        "rem.makeup.template",
        &[
            ("missed", missed.to_string()),
            ("done", node_count.to_string()),
            ("goal", daily_goal.to_string()),
        ],
    )
}

/// 判断设置变更是否影响提醒评估（用于重置计时）
fn is_reminder_related(payload: &serde_json::Value) -> bool {
    const KEYS: [&str; 8] = [
        "remind_freq_minutes",
        "remind_enabled",
        "remind_window_start",
        "remind_window_end",
        "dnd_rules",
        "daily_goal",
        "daily_goal_enabled",
        "todo_remind_offset_min",
    ];
    let empty = vec![];
    let list = payload["keys"].as_array().unwrap_or(&empty);
    list.iter().any(|k| {
        // 前端可能只发 "ai"/"reminder" 这类聚合键，全部信息类变更都重置更安全
        k.as_str().map(|s| KEYS.contains(&s) || s == "reminder" || s == "ai").unwrap_or(false)
    })
}

/// 目标小时是否已过（供简报/晚安设置的时间点判断，单位：当日分钟）
pub fn hhmm_to_minutes(s: &str) -> Option<i64> {
    parse_hhmm(s)
}

/// 当前时间字符串（用于测试）
pub fn now_string() -> String {
    Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

/// 计算下一次整点对齐的触发时间（保留给 Cron 模式使用）
pub fn next_aligned(now: NaiveDateTime, freq_minutes: i64) -> NaiveDateTime {
    let add = freq_minutes - (now.minute() as i64 % freq_minutes.max(1));
    now + Duration::minutes(add)
}
