//! 成就体系（FR-6.2 / 里程碑 M7）
//!
//! 设计要点：
//! - 徽章定义集中在此（id / 名称 / 描述 / 图标 / 条件说明），前端只做展示
//! - `check_all` 从数据侧统一评估全部条件（幂等），在关键动作后调用
//! - 本地、非社交、可一键关闭由前端负责；后端只负责判定与解锁记录

use crate::db::Db;
use crate::EventBus;
use anyhow::Result;
use serde::Serialize;

/// 一枚徽章
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AchievementDef {
    pub id: &'static str,
    /// 名称/说明/条件由 i18n 按界面语言解析（T1.4），故为 String
    pub name: String,
    pub description: String,
    pub icon: &'static str,
    pub condition: String,
    pub unlocked: bool,
    pub unlocked_at: Option<String>,
}

/// 全部徽章定义（顺序即展示顺序）
/// 徽章定义：名称/说明/条件存**词条 key**，展示时按界面语言解析（T1.4）
pub const CATALOG: [(&str, &str, &str, &str); 10] = [
    ("first_node", "✍️", "ach.first_node.name", "ach.first_node.desc"),
    ("first_todo", "✅", "ach.first_todo.name", "ach.first_todo.desc"),
    ("streak_3", "🔥", "ach.streak_3.name", "ach.streak_3.desc"),
    ("streak_7", "🔥", "ach.streak_7.name", "ach.streak_7.desc"),
    ("streak_30", "🔥", "ach.streak_30.name", "ach.streak_30.desc"),
    ("speed_10", "⚡", "ach.speed_10.name", "ach.speed_10.desc"),
    ("over_goal_3", "🎯", "ach.over_goal_3.name", "ach.over_goal_3.desc"),
    ("report_first", "📄", "ach.report_first.name", "ach.report_first.desc"),
    ("review_first", "📊", "ach.review_first.name", "ach.review_first.desc"),
    ("month_25", "🗓️", "ach.month_25.name", "ach.month_25.desc"),
];

/// 徽章目录 + 解锁状态
pub fn catalog(db: &Db) -> Result<Vec<AchievementDef>> {
    let unlocked = db.list_achievements()?;
    let map: std::collections::HashMap<String, String> = unlocked
        .into_iter()
        .map(|a| (a.id, a.unlocked_at))
        .collect();
    let lang = crate::i18n::Lang::from_setting(
        &db.get_setting("ui_locale").ok().flatten().unwrap_or_default(),
    );
    Ok(CATALOG
        .iter()
        .map(|(id, icon, name_key, desc_key)| AchievementDef {
            id,
            name: crate::i18n::tr(lang, name_key),
            description: crate::i18n::tr(lang, desc_key),
            icon,
            condition: crate::i18n::tr(lang, &format!("ach.{id}.cond")),
            unlocked: map.contains_key(*id),
            unlocked_at: map.get(*id).cloned(),
        })
        .collect())
}

/// 统一评估并解锁全部达成条件（幂等），返回本次新解锁的徽章
pub fn check_all(db: &Db, bus: &EventBus) -> Result<Vec<AchievementDef>> {
    let mut newly = Vec::new();

    let streak = db.streak_days()?;
    let today = crate::db::today_string();
    let month = &today[..7];

    // 单日 / 连续超额 / 记录满月：一次取范围内的每日计数
    let (from, to) = (format!("{month}-01"), format!("{month}-31"));
    let counts = db.node_counts_by_range(&from, &to)?;
    let today_count = counts.get(&today).copied().unwrap_or(0);
    let month_days_with_records = counts.values().filter(|c| **c > 0).count() as i64;

    let goal: i64 = db
        .get_setting("daily_goal")?
        .and_then(|v| v.parse().ok())
        .unwrap_or(4);
    let goal_enabled = db
        .get_setting("daily_goal_enabled")?
        .map(|v| v == "1")
        .unwrap_or(true);
    let over_goal_3 = if goal_enabled && goal > 0 {
        // 最近 3 个有记录的连续日是否都超过目标（以今天为终点回溯）
        let mut d = chrono::Local::now().date_naive();
        let mut ok = true;
        for _ in 0..3 {
            let key = d.format("%Y-%m-%d").to_string();
            if counts.get(&key).copied().unwrap_or(0) <= goal {
                ok = false;
                break;
            }
            d -= chrono::Duration::days(1);
        }
        ok
    } else {
        false
    };

    // 报告类：检查归档
    let has_period_report = !db.list_reports(Some("daily"), 1)?.is_empty()
        || !db.list_reports(Some("weekly"), 1)?.is_empty()
        || !db.list_reports(Some("monthly"), 1)?.is_empty();
    let has_review = !db.list_reports(Some("review"), 1)?.is_empty();
    let has_done_todo = db
        .list_todos(Some("全部"), Some("已完成"), None, None, None)?
        .len()
        > 0;

    let conditions: Vec<(&str, bool)> = vec![
        ("first_node", today_count > 0 || streak > 0 || month_days_with_records > 0),
        ("first_todo", has_done_todo),
        ("streak_3", streak >= 3),
        ("streak_7", streak >= 7),
        ("streak_30", streak >= 30),
        ("speed_10", today_count >= 10),
        ("over_goal_3", over_goal_3),
        ("report_first", has_period_report),
        ("review_first", has_review),
        ("month_25", month_days_with_records >= 25),
    ];

    let lang = crate::i18n::Lang::from_setting(
        &db.get_setting("ui_locale").ok().flatten().unwrap_or_default(),
    );
    for (id, ok) in conditions {
        if !ok {
            continue;
        }
        if db.unlock_achievement(id)? {
            let def = CATALOG
                .iter()
                .find(|(i, ..)| *i == id)
                .map(|(id, icon, name_key, desc_key)| AchievementDef {
                    id,
                    name: crate::i18n::tr(lang, name_key),
                    description: crate::i18n::tr(lang, desc_key),
                    icon,
                    condition: crate::i18n::tr(lang, &format!("ach.{id}.cond")),
                    unlocked: true,
                    unlocked_at: Some(crate::db::now_string()),
                });
            if let Some(def) = def {
                tracing::info!("解锁成就：{}（{}）", def.name, def.id);
                bus.publish(crate::Event::new(
                    "achievement.unlocked",
                    serde_json::json!({ "id": def.id, "title": def.name, "icon": def.icon }),
                ));
                newly.push(def);
            }
        }
    }
    Ok(newly)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{NewNode, NewTodo};
    use chrono::Datelike;

    fn db_with_days(days: &[&str], per_day: usize) -> Db {
        let db = Db::open_memory().unwrap();
        db.seed_defaults().unwrap();
        for d in days {
            for i in 0..per_day {
                db.create_node(NewNode {
                    content: format!("{d} 第{i}条"),
                    date: Some((*d).to_string()),
                    tags: vec![],
                    todo_id: None,
                })
                .unwrap();
            }
        }
        db
    }

    #[test]
    fn 目录包含需求列出的全部徽章() {
        let db = Db::open_memory().unwrap();
        db.seed_defaults().unwrap();
        let list = catalog(&db).unwrap();
        let ids: Vec<&str> = list.iter().map(|a| a.id).collect();
        for expect in [
            "first_node",
            "streak_3",
            "streak_7",
            "streak_30",
            "speed_10",
            "over_goal_3",
            "report_first",
            "review_first",
            "month_25",
        ] {
            assert!(ids.contains(&expect), "缺少徽章 {expect}：{ids:?}");
        }
        assert_eq!(list.len(), 10);
        assert!(list.iter().all(|a| !a.unlocked), "初始应全部未解锁");
        assert!(list.iter().all(|a| !a.name.is_empty() && !a.condition.is_empty()));
    }

    #[test]
    fn 首次记录解锁起步徽章() {
        let db = db_with_days(&[&crate::db::today_string()], 1);
        let bus = EventBus::new();
        let newly = check_all(&db, &bus).unwrap();
        let ids: Vec<&str> = newly.iter().map(|a| a.id).collect();
        assert!(ids.contains(&"first_node"), "{ids:?}");
        // 幂等：再次检查不重复解锁
        let again = check_all(&db, &bus).unwrap();
        assert!(again.is_empty(), "重复检查不应再次解锁：{:?}", again);
        let list = catalog(&db).unwrap();
        let first = list.iter().find(|a| a.id == "first_node").unwrap();
        assert!(first.unlocked && first.unlocked_at.is_some());
    }

    #[test]
    fn 单日十条解锁手速达人() {
        let db = db_with_days(&[&crate::db::today_string()], 10);
        let bus = EventBus::new();
        let ids: Vec<String> = check_all(&db, &bus)
            .unwrap()
            .into_iter()
            .map(|a| a.id.to_string())
            .collect();
        assert!(ids.contains(&"speed_10".to_string()), "{ids:?}");
    }

    #[test]
    fn 连续三天超额解锁超额完成() {
        // 每日目标 4，最近三天各 5 条
        let today = chrono::Local::now().date_naive();
        let days: Vec<String> = (0..3)
            .map(|i| (today - chrono::Duration::days(i)).format("%Y-%m-%d").to_string())
            .collect();
        let refs: Vec<&str> = days.iter().map(|s| s.as_str()).collect();
        let db = db_with_days(&refs, 5);
        let bus = EventBus::new();
        let ids: Vec<String> = check_all(&db, &bus)
            .unwrap()
            .into_iter()
            .map(|a| a.id.to_string())
            .collect();
        assert!(ids.contains(&"over_goal_3".to_string()), "{ids:?}");

        // 只有两天达标 → 不解锁
        let refs2: Vec<&str> = refs[..2].to_vec();
        let db2 = db_with_days(&refs2, 5);
        let bus2 = EventBus::new();
        let ids2: Vec<String> = check_all(&db2, &bus2)
            .unwrap()
            .into_iter()
            .map(|a| a.id.to_string())
            .collect();
        assert!(!ids2.contains(&"over_goal_3".to_string()), "{ids2:?}");
    }

    #[test]
    fn 生成报告与复盘解锁对应徽章() {
        let db = Db::open_memory().unwrap();
        db.seed_defaults().unwrap();
        let bus = EventBus::new();
        assert!(check_all(&db, &bus).unwrap().is_empty(), "无数据时不应解锁");

        db.save_report("daily", "2026-09-12", "# 日报", true).unwrap();
        let ids: Vec<String> = check_all(&db, &bus)
            .unwrap()
            .into_iter()
            .map(|a| a.id.to_string())
            .collect();
        assert!(ids.contains(&"report_first".to_string()), "{ids:?}");

        db.save_report("review", "2026 第37周", "# 复盘", true).unwrap();
        let ids2: Vec<String> = check_all(&db, &bus)
            .unwrap()
            .into_iter()
            .map(|a| a.id.to_string())
            .collect();
        assert!(ids2.contains(&"review_first".to_string()), "{ids2:?}");
    }

    #[test]
    fn 完成待办解锁第一件事() {
        let db = Db::open_memory().unwrap();
        db.seed_defaults().unwrap();
        let t = db
            .create_todo(NewTodo {
                title: "写方案".into(),
                description: String::new(),
                due_date: Some(crate::db::today_string()),
                due_time: None,
                priority: "中".into(),
                tags: vec![],
                remind_offset_min: None,
                remind_at: None,
                recur_type: String::new(),
                recur_until: String::new(),
                recur_interval: 1,
                recur_skip_rest: false,
            })
            .unwrap();
        let bus = EventBus::new();
        check_all(&db, &bus).unwrap();
        db.complete_todo(t.id, true).unwrap();
        let ids: Vec<String> = check_all(&db, &bus)
            .unwrap()
            .into_iter()
            .map(|a| a.id.to_string())
            .collect();
        assert!(ids.contains(&"first_todo".to_string()), "{ids:?}");
    }

    #[test]
    fn 当月二十五天记录解锁记录满月() {
        let today = chrono::Local::now().date_naive();
        let month_start = today.with_day(1).unwrap();
        let days: Vec<String> = (0..25)
            .map(|i| (month_start + chrono::Duration::days(i)).format("%Y-%m-%d").to_string())
            .collect();
        let refs: Vec<&str> = days.iter().map(|s| s.as_str()).collect();
        let db = db_with_days(&refs, 1);
        let bus = EventBus::new();
        let ids: Vec<String> = check_all(&db, &bus)
            .unwrap()
            .into_iter()
            .map(|a| a.id.to_string())
            .collect();
        assert!(ids.contains(&"month_25".to_string()), "{ids:?}");
    }
}
