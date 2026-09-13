//! 核心逻辑单元测试：智能提醒规则、待办归类、逾期判定、脱敏、周期计算
//!
//! 运行：cargo test -p mindmate-core

use mindmate_core::ai;
use mindmate_core::db::{classify, compute_remind_at, is_overdue, Db, NewNode, NewTodo, TodoPatch};
use mindmate_core::i18n::Lang;
use mindmate_core::reminder::{compose, decide, hhmm_to_minutes, ReminderKind};

// ───────────────────────── 上下文智能提醒规则（FR-4.3）─────────────────────────

const WINDOW: (i64, i64) = (9 * 60, 21 * 60); // 09:00 - 21:00

fn decide_default(
    now: i64,
    ago: i64,
    node_count: i64,
    goal: i64,
    goal_enabled: bool,
    streak: i64,
    overdue: &[String],
    dnd: bool,
) -> Option<ReminderKind> {
    decide(now, WINDOW, ago, 60, node_count, goal, goal_enabled, streak, overdue, dnd)
}

#[test]
fn 规则_未达标且无逾期_常规提醒() {
    let k = decide_default(10 * 60, 61, 1, 4, true, 3, &[], false);
    assert_eq!(k, Some(ReminderKind::Record));
}

#[test]
fn 规则_当日已达标_静默不打扰() {
    // 已录 4/4 → 不提醒
    assert_eq!(decide_default(10 * 60, 61, 4, 4, true, 3, &[], false), None);
    // 超额也静默
    assert_eq!(decide_default(10 * 60, 61, 7, 4, true, 3, &[], false), None);
    // 目标关闭时不静默
    assert_eq!(
        decide_default(10 * 60, 61, 9, 4, false, 3, &[], false),
        Some(ReminderKind::Record)
    );
}

#[test]
fn 规则_存在逾期待办_加压提醒() {
    let overdue = vec!["修复登录 bug".to_string()];
    assert_eq!(
        decide_default(10 * 60, 61, 2, 4, true, 3, &overdue, false),
        Some(ReminderKind::Overdue)
    );
}

#[test]
fn 规则_连续多日未记录_关怀提醒() {
    // streak = 0（今天与昨天都没记）
    assert_eq!(
        decide_default(10 * 60, 61, 0, 4, true, 0, &[], false),
        Some(ReminderKind::Care)
    );
}

#[test]
fn 规则_勿扰时段完全静默() {
    let overdue = vec!["逾期任务".to_string()];
    assert_eq!(decide_default(10 * 60, 61, 0, 4, true, 0, &overdue, true), None);
}

#[test]
fn 规则_生效时段外静默() {
    // 早上 7 点（窗口外）
    assert_eq!(decide_default(7 * 60, 61, 0, 4, true, 2, &[], false), None);
    // 晚上 23 点（窗口外）
    assert_eq!(decide_default(23 * 60, 61, 0, 4, true, 2, &[], false), None);
    // 窗口边界内
    assert_eq!(
        decide_default(9 * 60, 61, 0, 4, true, 2, &[], false),
        Some(ReminderKind::Record)
    );
}

#[test]
fn 规则_频率未到不重复提醒() {
    // 距上次 30 分钟（频率 60）→ 不提醒
    assert_eq!(decide_default(10 * 60, 30, 1, 4, true, 3, &[], false), None);
}

#[test]
fn 提醒文案_四类场景符合设计规范() {
    let n = compose(&ReminderKind::Record, 2, 4, &[], Lang::Zh);
    assert_eq!(n.title, "该记录一下了");
    assert!(n.body.contains("2/4"));
    assert_eq!(n.action, "quick_entry");

    let n = compose(&ReminderKind::Overdue, 1, 4, &["修复登录 bug".into()], Lang::Zh);
    assert!(n.title.contains("已逾期"));
    assert!(n.body.contains("修复登录 bug"));

    let n = compose(&ReminderKind::Care, 0, 4, &[], Lang::Zh);
    assert!(n.title.contains("没见到你的记录"));

    let n = compose(&ReminderKind::Makeup, 1, 4, &[], Lang::Zh);
    assert!(n.action == "quick_entry");

    let n = compose(&ReminderKind::Brief, 0, 0, &[], Lang::Zh);
    assert_eq!(n.action, "open_today");
    let n = compose(&ReminderKind::Todo, 0, 0, &["写方案".into()], Lang::Zh);
    assert_eq!(n.action, "open_todos");
    assert!(n.body.contains("写方案"));
}

#[test]
fn 时间解析_HHMM转分钟() {
    assert_eq!(hhmm_to_minutes("09:00"), Some(540));
    assert_eq!(hhmm_to_minutes("21:30"), Some(1290));
    assert_eq!(hhmm_to_minutes("00:00"), Some(0));
    assert_eq!(hhmm_to_minutes("bad"), None);
}

// ───────────────────────── 待办四类自动归类（FR-3.1/3.5）─────────────────────────

#[test]
fn 归类_含具体时间归入日程() {
    // 2026-09-12 是周六；有具体时间 → 日程
    assert_eq!(classify("2026-09-12", Some("10:00"), "2026-09-12"), "日程");
}

#[test]
fn 归类_今天与逾期归入今日() {
    assert_eq!(classify("2026-09-12", None, "2026-09-12"), "今日");
    assert_eq!(classify("2026-09-01", None, "2026-09-12"), "今日");
}

#[test]
fn 归类_本周内归入本周() {
    // 以 2026-09-09（周三）为今天 → 本周为 09-07(一) ~ 09-13(日)
    assert_eq!(classify("2026-09-11", None, "2026-09-09"), "本周");
    assert_eq!(classify("2026-09-13", None, "2026-09-09"), "本周");
    // 已过去的日期按设计归入「今日」（急需处理）
    assert_eq!(classify("2026-09-07", None, "2026-09-09"), "今日");
}

#[test]
fn 归类_本月内归入本月() {
    assert_eq!(classify("2026-09-20", None, "2026-09-12"), "本月");
    assert_eq!(classify("2026-09-30", None, "2026-09-12"), "本月");
}

#[test]
fn 归类_跨月归入日程() {
    assert_eq!(classify("2026-10-05", None, "2026-09-12"), "日程");
    assert_eq!(classify("2026-11-01", None, "2026-09-12"), "日程");
}

// ───────────────────────── 逾期判定（FR-3.3）─────────────────────────

#[test]
fn 逾期_已完成永不逾期() {
    assert!(!is_overdue("2020-01-01", Some("00:00"), "已完成"));
}

#[test]
fn 逾期_过去日期未完成即逾期() {
    assert!(is_overdue("2020-01-01", None, "待处理"));
    assert!(is_overdue("2020-01-01", None, "已逾期"));
}

#[test]
fn 提醒时刻_按提前量计算() {
    assert_eq!(
        compute_remind_at("2026-09-13", "10:00", 15),
        Some("2026-09-13 09:45".to_string())
    );
    assert_eq!(
        compute_remind_at("2026-09-13", "00:10", 30),
        Some("2026-09-12 23:40".to_string())
    );
    // 非法输入
    assert_eq!(compute_remind_at("bad", "10:00", 15), None);
}

// ───────────────────────── 脱敏（FR-5.16）─────────────────────────

#[test]
fn 脱敏_手机号身份证邮箱链接() {
    let input = "联系我 13812345678 或 a.b-c@example.com，身份证 11010119900307123X，见 https://example.com/secret";
    let out = ai::redact(input);
    assert!(!out.contains("13812345678"), "手机号未脱敏: {out}");
    assert!(!out.contains("a.b-c@example.com"), "邮箱未脱敏: {out}");
    assert!(!out.contains("11010119900307123X"), "身份证未脱敏: {out}");
    assert!(!out.contains("https://example.com/secret"), "链接未脱敏: {out}");
    assert!(out.contains("[隐私]"));
}

#[test]
fn 脱敏_普通内容不受影响() {
    let input = "完成 XX 模块开发，联调通过";
    assert_eq!(ai::redact(input), input);
}

// ───────────────────────── 周期计算（周报/月报范围）─────────────────────────

#[test]
fn 周期_周报范围为周一到周日() {
    let (from, to, label) = ai::period_range("weekly", "2026-09-12"); // 周六
    assert_eq!(from, "2026-09-07");
    assert_eq!(to, "2026-09-13");
    assert!(label.contains("37"), "ISO 周序错误: {label}");
}

#[test]
fn 周期_月报范围为首末日() {
    let (from, to, label) = ai::period_range("monthly", "2026-09-12");
    assert_eq!(from, "2026-09-01");
    assert_eq!(to, "2026-09-30");
    assert_eq!(label, "2026年9月");
}

#[test]
fn 周期_日报为当天() {
    let (from, to, label) = ai::period_range("daily", "2026-09-12");
    assert_eq!(from, "2026-09-12");
    assert_eq!(to, "2026-09-12");
    assert_eq!(label, "2026-09-12");
}

// ───────────────────────── Prompt 模板渲染（FR-5.13）─────────────────────────

#[test]
fn 模板_变量替换() {
    let tpl = "# {{date}} 日报\n{{nodes}}\n{{todos}}\n{{progress}}";
    let out = ai::render_template(
        tpl,
        &[
            ("date", "2026-09-12".into()),
            ("nodes", "- 10:15 完成模块".into()),
            ("todos", "- [x] 评审".into()),
            ("progress", "记录 4/4".into()),
        ],
    );
    assert!(out.contains("# 2026-09-12 日报"));
    assert!(out.contains("- 10:15 完成模块"));
    assert!(!out.contains("{{"));
}

#[test]
fn 模板_节点按日聚合格式正确() {
    let db = Db::open_memory().unwrap();
    db.create_node(NewNode {
        content: "完成 A 模块".into(),
        date: Some("2026-09-12".into()),
        tags: vec!["工作".into()],
        todo_id: None,
    })
    .unwrap();
    db.create_node(NewNode {
        content: "完成 B 模块".into(),
        date: Some("2026-09-13".into()),
        tags: vec![],
        todo_id: None,
    })
    .unwrap();
    let nodes = db.list_nodes_range("2026-09-12", "2026-09-13").unwrap();
    let text = ai::format_nodes_by_day(&nodes);
    assert!(text.contains("## 2026-09-12"));
    assert!(text.contains("## 2026-09-13"));
    assert!(text.contains("完成 A 模块"));
}

// ───────────────────────── 数据层：记录与统计 ─────────────────────────

#[test]
fn 数据层_一次录入生成一个节点() {
    let db = Db::open_memory().unwrap();
    for i in 0..4 {
        db.create_node(NewNode {
            content: format!("第 {i} 条记录"),
            date: Some("2026-09-12".into()),
            tags: vec![],
            todo_id: None,
        })
        .unwrap();
    }
    let nodes = db.list_nodes_by_date("2026-09-12").unwrap();
    assert_eq!(nodes.len(), 4, "录 4 条应产生 4 个节点（无模板干预）");
    // 时刻已记录
    assert!(nodes[0].created_at.len() >= 16);
}

#[test]
fn 数据层_补录标记() {
    let db = Db::open_memory().unwrap();
    let today = mindmate_core::db::today_string();
    let n = db
        .create_node(NewNode {
            content: "今天的".into(),
            date: Some(today),
            tags: vec![],
            todo_id: None,
        })
        .unwrap();
    assert!(!n.is_backfill);
    let b = db
        .create_node(NewNode {
            content: "历史补录".into(),
            date: Some("2020-01-01".into()),
            tags: vec![],
            todo_id: None,
        })
        .unwrap();
    assert!(b.is_backfill);
}

#[test]
fn 数据层_软删除不影响其它节点() {
    let db = Db::open_memory().unwrap();
    let a = db
        .create_node(NewNode {
            content: "A".into(),
            date: Some("2026-09-12".into()),
            tags: vec![],
            todo_id: None,
        })
        .unwrap();
    db.create_node(NewNode {
        content: "B".into(),
        date: Some("2026-09-12".into()),
        tags: vec![],
        todo_id: None,
    })
    .unwrap();
    assert!(db.delete_node(a.id).unwrap());
    assert_eq!(db.list_nodes_by_date("2026-09-12").unwrap().len(), 1);
}

#[test]
fn 数据层_全文检索记录() {
    let db = Db::open_memory().unwrap();
    db.create_node(NewNode {
        content: "完成登录模块联调".into(),
        date: Some("2026-09-12".into()),
        tags: vec![],
        todo_id: None,
    })
    .unwrap();
    let hits = db.search_nodes("登录", 10).unwrap();
    assert_eq!(hits.len(), 1);
    assert!(hits[0].content.contains("登录"));
}

#[test]
fn 数据层_待办完成与撤销流转() {
    let db = Db::open_memory().unwrap();
    let t = db
        .create_todo(NewTodo {
            title: "写技术方案".into(),
            description: String::new(),
            due_date: Some("2026-09-12".into()),
            due_time: None,
            priority: "高".into(),
            tags: vec![],
            remind_offset_min: None,
            remind_at: None,
        })
        .unwrap();
    assert_eq!(t.status, "待处理");
    assert_eq!(t.category, "今日");

    let done = db.complete_todo(t.id, true).unwrap().unwrap();
    assert_eq!(done.status, "已完成");
    assert!(done.completed_at.is_some());

    let reopened = db.complete_todo(t.id, false).unwrap().unwrap();
    assert_ne!(reopened.status, "已完成");
    assert!(reopened.completed_at.is_none());
}

#[test]
fn 数据层_逾期刷新标记() {
    let db = Db::open_memory().unwrap();
    db.create_todo(NewTodo {
        title: "过期任务".into(),
        description: String::new(),
        due_date: Some("2020-01-01".into()),
        due_time: None,
        priority: "中".into(),
        tags: vec![],
        remind_offset_min: None,
        remind_at: None,
    })
    .unwrap();
    let overdue = db.refresh_overdue().unwrap();
    assert_eq!(overdue.len(), 1);
    assert_eq!(overdue[0].status, "已逾期");
}

#[test]
fn 数据层_待办改期后重新归类() {
    let db = Db::open_memory().unwrap();
    let t = db
        .create_todo(NewTodo {
            title: "跨月任务".into(),
            description: String::new(),
            due_date: Some("2026-10-05".into()),
            due_time: None,
            priority: "中".into(),
            tags: vec![],
            remind_offset_min: None,
            remind_at: None,
        })
        .unwrap();
    assert_eq!(t.category, "日程");
    // 改到本月内 → 重新归类
    let updated = db
        .update_todo(
            t.id,
            TodoPatch {
                title: None,
                description: None,
                due_date: Some("2026-09-20".into()),
                due_time: None,
                remind_at: None,
                priority: None,
                tags: None,
                status: None,
                category: None,
                sort_order: None,
            },
        )
        .unwrap()
        .unwrap();
    assert_eq!(updated.category, "本月");
}

#[test]
fn 数据层_默认设置齐全() {
    let db = Db::open_memory().unwrap();
    db.seed_defaults().unwrap();
    let all = db.all_settings().unwrap();
    assert!(all.len() >= 20, "默认设置项不足: {}", all.len());
    let map: std::collections::HashMap<_, _> =
        all.into_iter().map(|s| (s.key, s.value)).collect();
    assert_eq!(map.get("daily_goal").map(|s| s.as_str()), Some("4"));
    assert_eq!(map.get("remind_freq_minutes").map(|s| s.as_str()), Some("60"));
    assert_eq!(map.get("theme").map(|s| s.as_str()), Some("system"));
    assert_eq!(map.get("ai_provider").map(|s| s.as_str()), Some("glm"));
    assert_eq!(map.get("ai_model").map(|s| s.as_str()), Some("glm-4-flash"));
}

#[test]
fn 数据层_备份导出包含全部分区() {
    let db = Db::open_memory().unwrap();
    db.seed_defaults().unwrap();
    db.create_node(NewNode {
        content: "备份测试".into(),
        date: Some("2026-09-12".into()),
        tags: vec![],
        todo_id: None,
    })
    .unwrap();
    db.save_report("daily", "2026-09-12", "# 日报", true).unwrap();
    let dump = db.export_all().unwrap();
    assert_eq!(dump["nodes"].as_array().unwrap().len(), 1);
    assert!(dump["settings"].as_array().unwrap().len() >= 20);
    assert_eq!(dump["reports"].as_array().unwrap().len(), 1);
}

// ───────────────────────── AI 无 Key 降级（FR-5.4）─────────────────────────

#[test]
fn 降级_日报模板拼装含记录与待办() {
    let db = Db::open_memory().unwrap();
    db.seed_defaults().unwrap();
    db.create_node(NewNode {
        content: "完成 XX 模块开发".into(),
        date: Some("2026-09-12".into()),
        tags: vec![],
        todo_id: None,
    })
    .unwrap();
    db.create_todo(NewTodo {
        title: "写技术方案".into(),
        description: String::new(),
        due_date: Some("2026-09-12".into()),
        due_time: None,
        priority: "高".into(),
        tags: vec![],
        remind_offset_min: None,
        remind_at: None,
    })
    .unwrap();

    let md = ai::fallback_report(&db, "daily", "2026-09-12").unwrap();
    assert!(md.contains("日报"));
    assert!(md.contains("完成 XX 模块开发"));
    assert!(md.contains("写技术方案"));
    assert!(md.contains("进度"));
    assert!(md.contains("未配置 AI"), "应标注为本地生成");
}

#[test]
fn 降级_周报按日聚合() {
    let db = Db::open_memory().unwrap();
    db.seed_defaults().unwrap();
    db.create_node(NewNode {
        content: "周一完成 A".into(),
        date: Some("2026-09-07".into()),
        tags: vec![],
        todo_id: None,
    })
    .unwrap();
    db.create_node(NewNode {
        content: "周二完成 B".into(),
        date: Some("2026-09-08".into()),
        tags: vec![],
        todo_id: None,
    })
    .unwrap();
    let md = ai::fallback_report(&db, "weekly", "2026-09-12").unwrap();
    assert!(md.contains("周报"));
    assert!(md.contains("2026-09-07"));
    assert!(md.contains("2026-09-08"));
    assert!(md.contains("周一完成 A"));
}

#[test]
fn 进度描述_记录与待办完成率() {
    let s = ai::format_progress(3, 4, true, 5, 3);
    assert!(s.contains("3/4"));
    assert!(s.contains("3/5"));
    assert!(s.contains("60%"));
}

// ───────────────────────── 月度小结（FR-6.4）─────────────────────────

#[test]
fn 月度小结_记录天数与完成待办与最长连续() {
    let db = Db::open_memory().unwrap();
    db.seed_defaults().unwrap();
    // 9/01、9/02 连续记录；9/05 单独一条 → 最长连续 2 天，记录天数 3
    for (date, content) in [
        ("2026-09-01", "第一天"),
        ("2026-09-02", "第二天"),
        ("2026-09-05", "第五天"),
    ] {
        db.create_node(NewNode {
            content: content.into(),
            date: Some(date.into()),
            tags: vec![],
            todo_id: None,
        })
        .unwrap();
    }
    // 本月 3 件待办，完成 2 件
    for (title, day) in [("任务A", "2026-09-03"), ("任务B", "2026-09-10"), ("任务C", "2026-09-28")] {
        let t = db
            .create_todo(NewTodo {
                title: title.into(),
                description: String::new(),
                due_date: Some(day.into()),
                due_time: None,
                priority: "中".into(),
                tags: vec![],
                remind_offset_min: None,
                remind_at: None,
            })
            .unwrap();
        if title != "任务C" {
            db.complete_todo(t.id, true).unwrap();
        }
    }

    let s = db.monthly_summary("2026-09-15").unwrap();
    assert_eq!(s.month, "2026-09");
    assert_eq!(s.node_count, 3);
    assert_eq!(s.days_with_records, 3);
    assert_eq!(s.total_days, 30);
    assert_eq!(s.done_todos, 2);
    assert_eq!(s.total_todos, 3);
    assert_eq!(s.longest_streak, 2, "9/01-9/02 应计为连续 2 天");
    assert!((s.avg_per_active_day - 1.0).abs() < 0.05);
    assert!(!s.is_month_end);
}

#[test]
fn 月度小结_月底标记与边界() {
    let db = Db::open_memory().unwrap();
    db.seed_defaults().unwrap();
    // 9 月最后一天 → is_month_end = true
    let s = db.monthly_summary("2026-09-30").unwrap();
    assert!(s.is_month_end);
    assert_eq!(s.total_days, 30);
    assert_eq!(s.longest_streak, 0, "无记录时连续为 0");
    assert_eq!(s.avg_per_active_day, 0.0);

    // 2 月（2026 非闰年）
    let f = db.monthly_summary("2026-02-10").unwrap();
    assert_eq!(f.total_days, 28);

    // 跨月不串数据
    db.create_node(NewNode {
        content: "十月的记录".into(),
        date: Some("2026-10-01".into()),
        tags: vec![],
        todo_id: None,
    })
    .unwrap();
    let sep = db.monthly_summary("2026-09-15").unwrap();
    assert_eq!(sep.node_count, 0, "9 月小结不应包含 10 月记录");
}

#[test]
fn 月度小结_月末当天连续跨月累计逻辑正确() {
    let db = Db::open_memory().unwrap();
    db.seed_defaults().unwrap();
    for d in ["2026-09-29", "2026-09-30"] {
        db.create_node(NewNode {
            content: format!("{d} 记录"),
            date: Some(d.into()),
            tags: vec![],
            todo_id: None,
        })
        .unwrap();
    }
    let s = db.monthly_summary("2026-09-30").unwrap();
    assert_eq!(s.longest_streak, 2);
    assert_eq!(s.days_with_records, 2);
    assert!(s.is_month_end);
}

// ───────────────────────── Rust 侧本地化（T1.4）─────────────────────────

#[test]
fn 降级报告_按语言生成标题与章节() {
    use mindmate_core::ai::fallback_report_lang;
    use mindmate_core::i18n::Lang;
    let db = Db::open_memory().unwrap();
    db.create_node(NewNode {
        content: "完成登录联调".into(),
        date: Some("2026-09-13".into()),
        tags: vec![],
        todo_id: None,
    })
    .unwrap();

    let zh = fallback_report_lang(&db, "daily", "2026-09-13", Lang::Zh).unwrap();
    assert!(zh.contains("日报") && zh.contains("今日完成") && zh.contains("由本地模板生成"));

    let en = fallback_report_lang(&db, "daily", "2026-09-13", Lang::En).unwrap();
    assert!(en.contains("Daily report") && en.contains("Done today"), "英文降级报告缺本地化：{en}");
    assert!(en.contains("local template"), "英文降级报告缺说明：{en}");
    assert!(!en.contains("今日完成"), "英文模式不应残留中文标题");

    let ja = fallback_report_lang(&db, "weekly", "2026-09-13", Lang::Ja).unwrap();
    assert!(ja.contains("週報") && ja.contains("概要"));

    let ko = fallback_report_lang(&db, "monthly", "2026-09-13", Lang::Ko).unwrap();
    assert!(ko.contains("월간 보고서") && ko.contains("개요"));
}

#[test]
fn 进度文案_按语言生成() {
    use mindmate_core::ai::format_progress_lang;
    use mindmate_core::i18n::Lang;
    assert!(format_progress_lang(2, 4, true, 5, 1, Lang::Zh).contains("2/4"));
    let en = format_progress_lang(2, 4, true, 5, 1, Lang::En);
    assert!(en.contains("2/4") && !en.contains("记录"), "英文进度文案残留中文：{en}");
    assert!(format_progress_lang(0, 0, false, 0, 0, Lang::Ko).contains("기록"));
}
