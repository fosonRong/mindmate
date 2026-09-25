//! 智能整理（v1.4.1）：收集箱归类建议 / 跟进待办识别 / 行动项提炼。
//! 全部纯函数、本地规则、确定性可测——AI 只在上游增强质量，规则是兜底底线。

use chrono::NaiveDate;
use crate::todo_extract::{clean_title_public, ExtractedTodo, take_date_public, take_time_public};

/// 收集箱条目的归类建议：todo=待办（动作/日期）reference=参考资料（链接）note=纯记录
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KindSuggestion {
    pub kind: &'static str,
    pub confidence: f64,
}

/// 判断一段文字更可能是「待办 / 参考资料 / 纯记录」。
pub fn suggest_kind(text: &str) -> KindSuggestion {
    const ACTION_WORDS: &[&str] = &[
        "开会", "会议", "提交", "发送", "回复", "跟进", "催", "买", "约", "审批",
        "报销", "预约", "体检", "面试", "写", "整理", "确认", "安排", "交房租", "还款",
        "付款", "取件", "维修", "交接",
    ];
    const REL_WORDS: &[&str] = &["今天", "今日", "明天", "明日", "后天", "大后天"];
    let t = text.trim();
    if t.is_empty() {
        return KindSuggestion { kind: "note", confidence: 0.5 };
    }
    // 1) 含链接 → 参考资料（链接的价值在存档回看）
    if t.contains("http://") || t.contains("https://") {
        return KindSuggestion { kind: "reference", confidence: 0.9 };
    }
    // 2) 时间词（点/半/HH:MM）与具体日期（X月X日/周X）→ 待办强信号；
    //    相对词（今天/明天…）单独不算强信号（叙述「今天的夕阳」也含），需叠加动作词
    let mut probe = t.to_string();
    let has_time = take_time_public(&mut probe).is_some();
    let has_rel = REL_WORDS.iter().any(|w| t.contains(w));
    let has_week = regex::Regex::new(r"(星期|礼拜|周)[一二三四五六日天]")
        .map(|re| re.is_match(t))
        .unwrap_or(false);
    let has_month_day = regex::Regex::new(r"[0-3]?\d月[0-3]?\d[日号]")
        .map(|re| re.is_match(t))
        .unwrap_or(false);
    let has_action = ACTION_WORDS.iter().any(|w| t.contains(w));
    if has_time || has_month_day || has_week || (has_rel && has_action) {
        return KindSuggestion {
            kind: "todo",
            confidence: if has_time || has_month_day { 0.9 } else { 0.8 },
        };
    }
    // 3) 动作词短句 → 待办
    if t.chars().count() <= 40 && has_action {
        return KindSuggestion { kind: "todo", confidence: 0.75 };
    }
    KindSuggestion { kind: "note", confidence: 0.7 }
}

/// 跟进类识别：「等待/跟进/催 X 的回复/反馈/结果」→ 建议创建跟进待办（3 天后）。
pub fn follow_up_todo(content: &str, today: NaiveDate) -> Option<ExtractedTodo> {
    let t = content.trim();
    let re = regex::Regex::new(
        r#"(?:等待|跟进|催)(?:一下)?(?P<obj>[^，。；,;!?！？]{1,20}?)(?:的)?(?:回复|答复|反馈|消息|结果|进度)"#,
    )
    .ok()?;
    let caps = re.captures(t)?;
    let obj = caps.name("obj")?.as_str().trim().to_string();
    if obj.is_empty() {
        return None;
    }
    Some(ExtractedTodo {
        title: format!("跟进：{obj}"),
        date: (today + chrono::Duration::days(3)).format("%Y-%m-%d").to_string(),
        time: None,
        tag: Some("跟进".into()),
    })
}

/// 从多条记录提炼行动项（本地规则兜底）：切句 → 留含动作词/日期的 → 去日期做标题（无日期默认明天）。
pub fn suggest_action_items(contents: &[String], today: NaiveDate) -> Vec<ExtractedTodo> {
    const ACTION_HINTS: &[&str] = &[
        "下一步", "需要", "待", "负责", "跟进", "完成", "提交", "发送", "确认", "安排", "决定",
        "开会", "约", "交", "写", "修",
    ];
    let tomorrow = (today + chrono::Duration::days(1)).format("%Y-%m-%d").to_string();
    let mut out: Vec<ExtractedTodo> = Vec::new();
    for content in contents {
        for part in content.split(['。', '；', ';', '\n', '！', '？']) {
            let p = part.trim();
            if p.chars().count() < 4 {
                continue;
            }
            let mut probe = p.to_string();
            let has_date_in_p = take_date_public(&mut probe, today).is_some();
            let is_action = ACTION_HINTS.iter().any(|w| p.contains(w)) || has_date_in_p;
            if !is_action {
                continue;
            }
            let mut rest = p.to_string();
            let time = take_time_public(&mut rest);
            let date = take_date_public(&mut rest, today).unwrap_or_else(|| tomorrow.clone());
            let title = clean_title_public(&rest);
            if title.chars().count() < 2 {
                continue;
            }
            let item = ExtractedTodo {
                title: title.chars().take(40).collect(),
                date,
                time,
                tag: Some("行动项".into()),
            };
            if !out.contains(&item) {
                out.push(item);
            }
            if out.len() >= 8 {
                return out;
            }
        }
    }
    out
}
