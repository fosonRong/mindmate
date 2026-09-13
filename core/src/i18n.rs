//! Rust 侧本地化：**用户可见文案**按 `ui_locale` 设置生成。
//!
//! 为什么需要它：界面已四语，但提醒通知、降级报告、徽章名、AI 提示词都是 Rust 生成的中文，
//! 切到英文后这些内容仍是中文（"半截子国际化"）。本模块统一收口这些文案。
//!
//! 设计：
//! - `Lang` 四语 + `from_setting()` 解析（zh-CN/en-US/ja-JP/ko-KR，缺省中文）
//! - `TABLE` 一张静态表：`(key, [中, 英, 日, 韩])`，`tr()` 查询、未命中回退中文并告警
//! - `tr_args()` 支持 `{name}` 占位符替换（与前端 `{a}` 风格一致）
//! - 单测保证：每条都有四种非空译文、插值可用、未知 key 安全回退

/// 支持的界面语言
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    Zh,
    En,
    Ja,
    Ko,
}

impl Lang {
    /// 由 `ui_locale` 设置值解析（前端写入的是 BCP-47，如 en-US）
    pub fn from_setting(v: &str) -> Lang {
        let s = v.trim().to_ascii_lowercase();
        if s.starts_with("en") {
            Lang::En
        } else if s.starts_with("ja") {
            Lang::Ja
        } else if s.starts_with("ko") {
            Lang::Ko
        } else {
            Lang::Zh
        }
    }

    pub const fn idx(self) -> usize {
        match self {
            Lang::Zh => 0,
            Lang::En => 1,
            Lang::Ja => 2,
            Lang::Ko => 3,
        }
    }
}

/// 文案表：(key, [中, 英, 日, 韩])
///
/// key 命名：`模块.用途[.字段]`，占位符写在译文里用 `{name}`。
pub const TABLE: &[(&str, [&str; 4])] = &[
    // ── 提醒：标题与正文（通知里直接展示，最高可见度）──
    ("rem.record.title", ["该记录一下了", "Time to log something", "そろそろ記録の時間です", "기록할 시간이에요"]),
    (
        "rem.record.body",
        ["今日已录 {done}/{goal} 条，点击速记", "Today {done}/{goal} — tap to log", "本日 {done}/{goal} 件 — タップで記録", "오늘 {done}/{goal}건 — 탭하여 기록"],
    ),
    ("rem.overdue.title", ["有 {n} 个待办已逾期", "{n} todos are overdue", "{n} 件の ToDo が期限超過", "{n}개의 할 일이 기한 초과"]),
    ("rem.overdue.more", [" 等 {n} 项", " and {n} more", " ほか {n} 件", " 외 {n}건"]),
    (
        "rem.overdue.body",
        ["{first}{more}，别忘啦，点击速记", "{first}{more} — don't forget, tap to log", "{first}{more}、忘れずに。タップで記録", "{first}{more} — 잊지 마세요. 탭하여 기록"],
    ),
    ("rem.care.title", ["这两天没见到你的记录了", "We haven't seen you in a couple of days", "ここ数日、記録がありません", "며칠째 기록이 없어요"]),
    ("rem.care.body", ["还好吗？花 30 秒记一笔吧", "All good? 30 seconds to log one", "お元気ですか？30 秒だけ記録しませんか", "괜찮으세요? 30초면 한 줄 기록할 수 있어요"]),
    ("rem.makeup.title", ["你错过了几次记录提醒", "You missed a few reminders", "いくつかのリマインダーを見送りました", "알림을 몇 번 놓쳤어요"]),
    (
        "rem.makeup.body",
        ["现在花 30 秒补记吧（今日已录 {done}/{goal} 条）", "Take 30 seconds now (today {done}/{goal})", "いま 30 秒で追記しましょう（本日 {done}/{goal} 件）", "지금 30초만 기록해요 (오늘 {done}/{goal}건)"],
    ),
    ("rem.makeup.template", ["错过了 {missed} 次记录提醒，现在花 30 秒补记吧（今日已录 {done}/{goal} 条）", "Missed {missed} reminders — 30 seconds to catch up (today {done}/{goal})", "{missed} 回のリマインダーを見送りました。30 秒で追記しましょう（本日 {done}/{goal} 件）", "알림 {missed}회를 놓쳤어요. 30초면 됩니다 (오늘 {done}/{goal}건)"]),
    ("rem.todo.title", ["待办即将到期", "A todo is due soon", "ToDo の期限が近づいています", "할 일 마감이 다가옵니다"]),
    ("rem.todo.fallback", ["查看待办", "View todos", "ToDo を見る", "할 일 보기"]),
    ("rem.brief.title", ["早安，今天的计划已就绪", "Good morning — today's plan is ready", "おはようございます。今日の予定ができました", "좋은 아침이에요 — 오늘 계획이 준비됐어요"]),
    ("rem.brief.body", ["晨间简报已生成，点击查看", "Your morning brief is ready — tap to view", "朝のブリーフができました。タップで表示", "아침 브리핑이 준비됐어요 — 탭하여 보기"]),
    ("rem.goodnight.title", ["今天辛苦了", "Nice work today", "今日もお疲れさまでした", "오늘도 수고했어요"]),
    ("rem.goodnight.body", ["晚安总结已生成，看看今天的收获", "Your goodnight summary is ready", "おやすみまとめができました", "잘 자요 요약이 준비됐어요"]),
    // ── 降级报告（未配置 AI 时的本地模板）──
    ("report.type.daily", ["日报", "Daily report", "日報", "일간 보고서"]),
    ("report.type.weekly", ["周报", "Weekly report", "週報", "주간 보고서"]),
    ("report.type.monthly", ["月报", "Monthly report", "月報", "월간 보고서"]),
    ("report.type.brief", ["晨间简报", "Morning brief", "朝のブリーフ", "아침 브리핑"]),
    ("report.type.goodnight", ["晚安总结", "Goodnight summary", "おやすみまとめ", "잘 자요 요약"]),
    ("report.type.review", ["周度复盘", "Weekly review", "週次レビュー", "주간 리뷰"]),
    ("report.notice", ["由本地模板生成（未配置 AI 模型）", "Generated with a local template (no AI model configured)", "ローカルテンプレートで生成（AI モデル未設定）", "로컬 템플릿으로 생성(AI 모델 미설정)"]),
    ("report.h.today_done", ["今日完成", "Done today", "今日の完了", "오늘 완료"]),
    ("report.h.progress", ["当前进度", "Current progress", "現在の進捗", "현재 진행률"]),
    ("report.h.today_todos", ["今日待办", "Today's todos", "今日の ToDo", "오늘 할 일"]),
    ("report.h.attention", ["需要注意", "Needs attention", "注意点", "주의 필요"]),
    ("report.h.overview", ["概览", "Overview", "概要", "개요"]),
    ("report.h.by_day", ["按日记录", "Entry by day", "日別の記録", "일자별 기록"]),
    ("report.empty.today", ["（今日暂无记录）", "(no entries today)", "（本日の記録はありません）", "(오늘 기록 없음)"]),
    ("report.empty.none", ["（无）", "(none)", "（なし）", "(없음)"]),
    ("report.empty.period", ["（该周期无记录）", "(no entries in this period)", "（この期間の記録はありません）", "(이 기간에 기록 없음)"]),
    ("report.overdue_line", ["有 {n} 个待办已逾期", "{n} todos are overdue", "{n} 件の ToDo が期限超過", "{n}개의 할 일이 기한 초과"]),
    ("report.summary_line", ["记录 {nodes} 条，覆盖 {days} / {total} 天", "{nodes} entries across {days} / {total} days", "{nodes} 件、{days} / {total} 日に記録", "{nodes}건, {days}/{total}일 기록"]),
    ("report.todos_done", ["待办完成 {done}/{all}", "Todos {done}/{all}", "ToDo 完了 {done}/{all}", "할 일 완료 {done}/{all}"]),
    ("report.week_label", ["{year} 第 {week} 周", "Week {week}, {year}", "{year}年 第{week}週", "{year}년 {week}주차"]),
    ("report.progress.recorded", ["已录 {done}/{goal} 条", "{done}/{goal} logged", "{done}/{goal} 件記録", "{done}/{goal}건 기록"]),
    ("report.progress.bonus", ["超额 {n} 条", "{n} over goal", "目標超過 {n} 件", "목표 초과 {n}건"]),
    // ── 徽章（名称/说明/解锁条件）──
    ("ach.first_node.name", ["起步", "First step", "はじめの一歩", "첫 걸음"]),
    ("ach.first_node.desc", ["完成首次速记", "Log your first entry", "初めての記録", "첫 기록 남기기"]),
    ("ach.first_node.cond", ["录入第一条记录", "Save the first entry", "最初の記録を保存", "첫 기록 저장"]),
    ("ach.first_todo.name", ["第一件事", "First task", "最初のタスク", "첫 할 일"]),
    ("ach.first_todo.desc", ["完成首件待办", "Finish your first todo", "最初の ToDo を完了", "첫 할 일 완료"]),
    ("ach.first_todo.cond", ["勾选完成任意待办", "Check off any todo", "任意の ToDo を完了", "할 일 하나 완료"]),
    ("ach.streak_3.name", ["三日之约", "Three-day streak", "3日連続", "3일 연속"]),
    ("ach.streak_3.desc", ["连续记录 3 天", "Record 3 days in a row", "3日連続で記録", "3일 연속 기록"]),
    ("ach.streak_3.cond", ["连续 3 天每天至少 1 条记录", "At least one entry for 3 days straight", "3日連続で毎日1件以上", "3일 연속 하루 1건 이상"]),
    ("ach.streak_7.name", ["一周之约", "Seven-day streak", "7日連続", "7일 연속"]),
    ("ach.streak_7.desc", ["连续记录 7 天", "Record 7 days in a row", "7日連続で記録", "7일 연속 기록"]),
    ("ach.streak_7.cond", ["连续 7 天每天至少 1 条记录", "At least one entry for 7 days straight", "7日連続で毎日1件以上", "7일 연속 하루 1건 이상"]),
    ("ach.streak_30.name", ["三十日之约", "Thirty-day streak", "30日連続", "30일 연속"]),
    ("ach.streak_30.desc", ["连续记录 30 天", "Record 30 days in a row", "30日連続で記録", "30일 연속 기록"]),
    ("ach.streak_30.cond", ["连续 30 天每天至少 1 条记录", "At least one entry for 30 days straight", "30日連続で毎日1件以上", "30일 연속 하루 1건 이상"]),
    ("ach.speed_10.name", ["手速达人", "Speed writer", "筆が早い人", "속필 달인"]),
    ("ach.speed_10.desc", ["单日录入 10 条", "Log 10 entries in one day", "1日に10件記録", "하루 10건 기록"]),
    ("ach.speed_10.cond", ["单日节点数达到 10 条", "Reach 10 entries in a single day", "1日の記録が10件に到達", "하루 기록 10건 달성"]),
    ("ach.over_goal_3.name", ["超额完成", "Overachiever", "目標超過", "목표 초과 달성"]),
    ("ach.over_goal_3.desc", ["连续 3 天超过每日目标", "Beat your goal 3 days running", "3日連続で目標超過", "3일 연속 목표 초과"]),
    ("ach.over_goal_3.cond", ["连续 3 天记录数超过每日目标", "Exceed the daily goal 3 days in a row", "3日連続で1日の目標を超過", "3일 연속 일일 목표 초과"]),
    ("ach.report_first.name", ["汇报达人", "Report master", "レポート名人", "리포트 달인"]),
    ("ach.report_first.desc", ["首次生成日报/周报/月报", "Generate your first report", "初めてのレポート生成", "첫 보고서 생성"]),
    ("ach.report_first.cond", ["成功生成任意一类周期报告", "Generate any periodic report", "いずれかの周期レポートを生成", "주기 보고서 생성"]),
    ("ach.review_first.name", ["复盘专家", "Review expert", "振り返りの達人", "리뷰 전문가"]),
    ("ach.review_first.desc", ["首次查看周度复盘", "Open your first weekly review", "初めての週次レビュー", "첫 주간 리뷰 확인"]),
    ("ach.review_first.cond", ["生成或查看一次周度智能复盘", "Generate or view a weekly review", "週次レビューを生成または表示", "주간 리뷰 생성 또는 열람"]),
    ("ach.month_25.name", ["记录满月", "Full month", "満月記録", "한 달 가득"]),
    ("ach.month_25.desc", ["当月 25 天以上有记录", "Record on 25+ days in a month", "1ヶ月で25日以上記録", "한 달에 25일 이상 기록"]),
    ("ach.month_25.cond", ["当月有记录的天数达到 25 天", "Reach 25 recorded days in a month", "1ヶ月の記録日数が25日に到達", "한 달 기록 일수 25일 달성"]),
];

/// 查表：未命中回退中文并告警（不 panic，保证功能不因漏词条而中断）
pub fn tr(lang: Lang, key: &str) -> String {
    match TABLE.iter().find(|(k, _)| *k == key) {
        Some((_, v)) => v[lang.idx()].to_string(),
        None => {
            tracing::warn!("缺少词条：{key}（已回退中文键名）");
            key.to_string()
        }
    }
}

/// 带 `{name}` 占位符的文案
pub fn tr_args(lang: Lang, key: &str, args: &[(&str, String)]) -> String {
    let mut s = tr(lang, key);
    for (k, v) in args {
        s = s.replace(&format!("{{{k}}}"), v);
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 语言解析_四语与回退() {
        assert_eq!(Lang::from_setting("zh-CN"), Lang::Zh);
        assert_eq!(Lang::from_setting("en-US"), Lang::En);
        assert_eq!(Lang::from_setting("ja-JP"), Lang::Ja);
        assert_eq!(Lang::from_setting("ko-KR"), Lang::Ko);
        assert_eq!(Lang::from_setting("en"), Lang::En);
        assert_eq!(Lang::from_setting(""), Lang::Zh);
        assert_eq!(Lang::from_setting("fr-FR"), Lang::Zh);
    }

    #[test]
    fn 文案表_每种语言都有非空译文() {
        let mut bad = Vec::new();
        for (k, v) in TABLE {
            for (i, s) in v.iter().enumerate() {
                if s.trim().is_empty() {
                    bad.push(format!("{k}[{i}]"));
                }
            }
        }
        assert!(bad.is_empty(), "存在空译文：{bad:?}");
    }

    #[test]
    fn 文案表_key_不重复() {
        let mut seen = std::collections::HashSet::new();
        for (k, _) in TABLE {
            assert!(seen.insert(*k), "重复 key：{k}");
        }
    }

    #[test]
    fn 文案表_四语占位符数量一致() {
        for (k, v) in TABLE {
            let counts: Vec<usize> = v.iter().map(|s| s.matches('{').count()).collect();
            assert!(
                counts.iter().all(|c| *c == counts[0]),
                "占位符数量不一致：{k} → {counts:?}"
            );
        }
    }

    #[test]
    fn 查表_四语可用且未知key安全回退() {
        assert_eq!(tr(Lang::Zh, "rem.record.title"), "该记录一下了");
        assert_eq!(tr(Lang::En, "rem.record.title"), "Time to log something");
        assert_eq!(tr(Lang::Ja, "rem.care.title"), "ここ数日、記録がありません");
        assert_eq!(tr(Lang::Ko, "rem.todo.title"), "할 일 마감이 다가옵니다");
        assert_eq!(tr(Lang::En, "不存在的key"), "不存在的key");
    }

    #[test]
    fn 插值_替换占位符() {
        let s = tr_args(Lang::En, "rem.record.body", &[("done", "2".into()), ("goal", "4".into())]);
        assert_eq!(s, "Today 2/4 — tap to log");
        let s2 = tr_args(Lang::Zh, "rem.makeup.template", &[("missed", "3".into()), ("done", "1".into()), ("goal", "4".into())]);
        assert!(s2.contains("3 次") && s2.contains("1/4"));
    }

    #[test]
    fn 徽章_十个都有四语名称与说明() {
        for id in ["first_node", "first_todo", "streak_3", "streak_7", "streak_30", "speed_10", "over_goal_3", "report_first", "review_first", "month_25"] {
            for suffix in ["name", "desc", "cond"] {
                let key = format!("ach.{id}.{suffix}");
                for lang in [Lang::Zh, Lang::En, Lang::Ja, Lang::Ko] {
                    let v = tr(lang, &key);
                    assert!(!v.is_empty() && v != key, "缺少词条 {key} ({lang:?})");
                }
            }
        }
    }
}
