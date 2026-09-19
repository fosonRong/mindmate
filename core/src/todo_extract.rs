//! 智能速记的自然语言拆解（v1.1.2）：从一句话里抽出「待办 + 日期 + 时间」。
//!
//! 双路设计：AI 已配置走模型（`parse_todo_array` 解析其 JSON 输出）；
//! 未配置/解析失败走本地规则（`extract_todos_local`，纯函数可单测）。
//! 本地规则覆盖中文口语常见写法：今天/明天/后天/周X/下周X/X月X日/X号/HH:MM/X点[X分]/X点半，
//! 以及「下午/晚上/中午」对小时的修正。刻意不做分词级 NLP——它的职责是兜底，
//! 让没配 AI 的用户也能一键把「明天下午3点开会」变成一条带日期时间的待办。

use chrono::{Datelike, NaiveDate};

/// 一条拆解出的待办（date=YYYY-MM-DD，time=HH:MM 或 None）
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtractedTodo {
    pub title: String,
    pub date: String,
    pub time: Option<String>,
}

/// 从模型回复里抠出 JSON 数组（容忍 ```json 包裹与前后废话），并清洗校验：
/// 标题非空 ≤60 字、日期合法（非法丢弃该条）、时间规整成 HH:MM、最多 5 条。
pub fn parse_todo_array(text: &str) -> Vec<ExtractedTodo> {
    let raw = match (text.find('['), text.rfind(']')) {
        (Some(s), Some(e)) if e > s => &text[s..=e],
        _ => return Vec::new(),
    };
    #[derive(serde::Deserialize)]
    struct RawTodo {
        #[serde(default)]
        title: serde_json::Value,
        #[serde(default)]
        date: serde_json::Value,
        #[serde(default)]
        time: serde_json::Value,
    }
    let Ok(arr) = serde_json::from_str::<Vec<RawTodo>>(raw) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for t in arr {
        let title = t.title.as_str().unwrap_or("").trim().to_string();
        if title.is_empty() || title.chars().count() > 60 {
            continue;
        }
        // 日期必须合法；模型没给就丢（本地兜底路径会用今天，模型路径给不出可信日期说明没读懂）
        let Some(date) = t.date.as_str().and_then(|s| normalize_date(s)) else {
            continue;
        };
        let time = t.time.as_str().and_then(normalize_time);
        out.push(ExtractedTodo { title, date, time });
        if out.len() >= 5 {
            break;
        }
    }
    out
}

/// YYYY-MM-DD 宽松校验/规整（接受 2026-9-3 → 2026-09-03）
fn normalize_date(s: &str) -> Option<String> {
    let d = NaiveDate::parse_from_str(s.trim(), "%Y-%m-%d").ok()?;
    Some(d.format("%Y-%m-%d").to_string())
}

/// HH:MM 规整（接受 9:5 / 09:05 / 9点05 之外的直接放弃）
fn normalize_time(s: &str) -> Option<String> {
    let s = s.trim();
    let (h, m) = s.split_once(':')?;
    let h: u32 = h.trim().parse().ok()?;
    let m: u32 = m.trim().parse().ok()?;
    if h > 23 || m > 59 {
        return None;
    }
    Some(format!("{:02}:{:02}", h, m))
}

/// 本地规则拆解（AI 未配置时的兜底）。
/// 先按句读（。；；换行）粗分句，每句独立抽日期/时间，剩下的就是标题。
pub fn extract_todos_local(content: &str, today: NaiveDate) -> Vec<ExtractedTodo> {
    let text = content.trim();
    if text.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    // 分句：保留「然后/接着/再」串起来的多个意图；逗号不切（一句话里日期时间常靠逗号衔接）
    let parts: Vec<&str> = text
        .split(['。', '；', ';', '\n'])
        .flat_map(|p| split_then(p))
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .collect();
    let parts = if parts.is_empty() { vec![text] } else { parts };
    for part in parts.into_iter().take(5) {
        if let Some(todo) = extract_one(part, today) {
            if !out.contains(&todo) {
                out.push(todo);
            }
        }
    }
    out
}

/// 按「然后/接着/，再」二次切分（"下午3点开会，然后晚上写周报"）
fn split_then(part: &str) -> Vec<&str> {
    let mut out: Vec<&str> = Vec::new();
    let mut rest = part;
    loop {
        // 找最早出现的连接词（按字节位置比较，长度即 k.len() 字节）
        let mut found: Option<(usize, usize)> = None;
        for k in ["然后", "接着", "，再"] {
            if let Some(pos) = rest.find(k) {
                if found.map_or(true, |(p, _)| pos < p) {
                    found = Some((pos, k.len()));
                }
            }
        }
        match found {
            Some((pos, len)) => {
                let seg = &rest[..pos];
                if !seg.trim().is_empty() {
                    out.push(seg);
                }
                rest = &rest[pos + len..];
            }
            None => break,
        }
    }
    if !rest.trim().is_empty() {
        out.push(rest);
    }
    if out.is_empty() {
        out.push(part);
    }
    out
}

/// 单句抽取：时间（含上午/下午修正）→ 日期 → 标题 = 去掉命中片段后的残余
fn extract_one(text: &str, today: NaiveDate) -> Option<ExtractedTodo> {
    let mut rest = text.to_string();
    let time = take_time(&mut rest);
    let date = take_date(&mut rest, today);
    let title = clean_title(&rest);
    if title.is_empty() {
        return None;
    }
    Some(ExtractedTodo {
        title,
        date: date.unwrap_or_else(|| today.format("%Y-%m-%d").to_string()),
        time,
    })
}

// ── 时间抽取 ──

/// 从字符串头开始找第一个时间表达并从原串删除，返回规整 HH:MM
fn take_time(s: &mut String) -> Option<String> {
    // HH:MM / HH：MM（分钟要两位数，避免把「比例3:1」当时间）
    let re = regex::Regex::new(r"([01]?\d|2[0-3])[:：]([0-5]\d)").unwrap();
    if let Some(c) = re.captures(s) {
        let h: u32 = c[1].parse().ok()?;
        let m: u32 = c[2].parse().ok()?;
        if m <= 59 {
            *s = re.replace(s, "").to_string();
            return Some(format!("{:02}:{:02}", h, m));
        }
    }
    // X点半 / X点[X分] / X点半分 不支持，X点半→:30，X点→:00，X点一刻→:15，X点三刻→:45
    let re = regex::Regex::new(r"(?:([上午下午晚中早]{1,2}))?([0-9一二两三四五六七八九十]{1,3})点(半|一刻|三刻|([0-5]?\d)分?)?").unwrap();
    if let Some(c) = re.captures(s) {
        let hour_raw = c[2].to_string();
        let hour_cn = cn_num(&hour_raw)?;
        let mut hour = hour_cn;
        let prefix = c.get(1).map(|m| m.as_str()).unwrap_or("");
        if (prefix.contains("下午") || prefix.contains("晚上")) && hour < 12 {
            hour += 12;
        }
        if prefix.contains("中午") && hour == 12 {
            hour = 12;
        }
        if hour > 23 {
            return None;
        }
        let minute = match c.get(3).map(|m| m.as_str()) {
            Some("半") => 30,
            Some("一刻") => 15,
            Some("三刻") => 45,
            Some(m) => m.parse::<u32>().ok().filter(|v| *v <= 59)?,
            None => 0,
        };
        let whole = c.get(0).unwrap().as_str().to_string();
        *s = s.replacen(&whole, "", 1);
        return Some(format!("{:02}:{:02}", hour, minute));
    }
    None
}

// ── 日期抽取 ──

/// 找日期表达并从原串删除，返回 YYYY-MM-DD
fn take_date(s: &mut String, today: NaiveDate) -> Option<String> {
    // 相对词
    let rel: &[(&str, i64)] = &[
        ("大后天", 3),
        ("后天", 2),
        ("明天", 1),
        ("明日", 1),
        ("今天", 0),
        ("今日", 0),
    ];
    for (word, delta) in rel {
        if let Some(pos) = s.find(word) {
            s.replace_range(pos..pos + word.len(), "");
            return Some((today + chrono::Duration::days(*delta)).format("%Y-%m-%d").to_string());
        }
    }
    // 下周X / 星期X / 周X / 礼拜X（下周限定下一周，其余取最近的未来，当天指今天）
    let re = regex::Regex::new(r"(下下周|下周|下星期)?(星期|礼拜|周)([一二三四五六日天])").unwrap();
    if let Some(c) = re.captures(s) {
        let target = cn_weekday(&c[3])?; // 周一=1..周日=7
        let prefix = c.get(1).map(|m| m.as_str().to_string()).unwrap_or_default();
        let whole = c.get(0).unwrap().as_str().to_string();
        let delta = if prefix.starts_with("下") {
            // 到下周一的天数 + （下下周再 +7）+ 周内偏移
            let to_next_mon = 7 - today.weekday().num_days_from_monday() as i64;
            let extra = if prefix == "下下周" { 7 } else { 0 };
            to_next_mon + extra + (target - 1)
        } else {
            (target - 1 - today.weekday().num_days_from_monday() as i64).rem_euclid(7) // 0 = 就是今天
        };
        let pos = s.find(&whole).unwrap();
        s.replace_range(pos..pos + whole.len(), "");
        return Some((today + chrono::Duration::days(delta)).format("%Y-%m-%d").to_string());
    }
    // X月X日/号
    let re = regex::Regex::new(r"([01]?\d)月([0-3]?\d)[日号]").unwrap();
    if let Some(c) = re.captures(s) {
        let month: u32 = c[1].parse().ok()?;
        let day: u32 = c[2].parse().ok()?;
        let year = today.year();
        let candidate = NaiveDate::from_ymd_opt(year, month, day)
            .or_else(|| NaiveDate::from_ymd_opt(year + 1, month, day))?;
        let whole = c.get(0).unwrap().as_str().to_string();
        s.replace_range(s.find(&whole).unwrap()..s.find(&whole).unwrap() + whole.len(), "");
        // 今年已过 → 明年
        let candidate = if candidate < today { candidate.with_year(year + 1).unwrap_or(candidate) } else { candidate };
        return Some(candidate.format("%Y-%m-%d").to_string());
    }
    // X号 / X日（当月，已过则下月）
    let re = regex::Regex::new(r"([0-3]?\d)[日号]").unwrap();
    if let Some(c) = re.captures(s) {
        let day: u32 = c[1].parse().ok()?;
        let (mut y, mut m) = (today.year(), today.month());
        let mut candidate = NaiveDate::from_ymd_opt(y, m, day);
        while candidate.is_none() {
            m += 1;
            if m > 12 {
                m = 1;
                y += 1;
            }
            candidate = NaiveDate::from_ymd_opt(y, m, day);
        }
        let candidate = candidate?;
        let whole = c.get(0).unwrap().as_str().to_string();
        s.replace_range(s.find(&whole).unwrap()..s.find(&whole).unwrap() + whole.len(), "");
        let candidate = if candidate < today {
            let (ny, nm) = if m == 12 { (y + 1, 1) } else { (y, m + 1) };
            NaiveDate::from_ymd_opt(ny, nm, day).unwrap_or(candidate)
        } else {
            candidate
        };
        return Some(candidate.format("%Y-%m-%d").to_string());
    }
    None
}

/// 标题清理：去掉日期/时间残留后的空白与首尾标点，以及明确的祈使引导词（记得/别忘了）。
/// 刻意不动「要/去/需要」——「要求客户确认」剥成「求客户确认」这类误伤比口语省字更亏。
fn clean_title(s: &str) -> String {
    let mut t = s.trim();
    for w in ["记得要", "记得", "别忘了"] {
        if let Some(stripped) = t.strip_prefix(w) {
            if stripped.chars().count() >= 2 {
                t = stripped.trim_start();
                break;
            }
        }
    }
    let t = t.trim_matches(|c: char| matches!(c, '，' | '、' | ' ' | '～' | '~' | '-' | '—'));
    t.chars().take(60).collect()
}

/// 中文数字（限 0-199 的口语范围足够）
fn cn_num(s: &str) -> Option<u32> {
    if let Ok(v) = s.parse::<u32>() {
        return Some(v);
    }
    let digits: &[(char, u32)] = &[
        ('〇', 0), ('零', 0), ('一', 1), ('二', 2), ('两', 2), ('三', 3), ('四', 4),
        ('五', 5), ('六', 6), ('七', 7), ('八', 8), ('九', 9),
    ];
    // 十X / X十 / X十Y / 二十四
    let mut vals: Vec<u32> = Vec::new();
    for ch in s.chars() {
        match digits.iter().find(|(c, _)| *c == ch) {
            Some((_, v)) => vals.push(*v),
            None if ch == '十' => vals.push(10),
            None => return None,
        }
    }
    if vals.is_empty() {
        return None;
    }
    if vals.len() == 1 {
        return Some(vals[0]);
    }
    // 简单展开：出现 10 视为乘法结合（二十→20，二十四→24）
    let mut total = 0u32;
    let mut i = 0;
    while i < vals.len() {
        if vals[i] == 10 {
            total = if total == 0 { 10 } else { total * 10 };
        } else if i + 1 < vals.len() && vals[i + 1] == 10 {
            total += vals[i] * 10;
            i += 1;
        } else {
            total += vals[i];
        }
        i += 1;
    }
    Some(total).filter(|v| *v <= 199)
}

/// 周几中文 → 周一=1..周日=7
fn cn_weekday(s: &str) -> Option<i64> {
    match s {
        "一" => Some(1),
        "二" => Some(2),
        "三" => Some(3),
        "四" => Some(4),
        "五" => Some(5),
        "六" => Some(6),
        "日" | "天" => Some(7),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    #[test]
    fn 本地拆解_相对日期加时间() {
        let today = d(2026, 9, 19); // 周六
        let out = extract_todos_local("明天下午3点开会", today);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].date, "2026-09-20");
        assert_eq!(out[0].time, Some("15:00".into()));
        assert_eq!(out[0].title, "开会");
    }

    #[test]
    fn 本地拆解_周几与半小时() {
        let today = d(2026, 9, 19);
        let out = extract_todos_local("周三早上8点半跑步", today);
        assert_eq!(out[0].date, "2026-09-23");
        assert_eq!(out[0].time, Some("08:30".into()));
        assert_eq!(out[0].title, "跑步");
    }

    #[test]
    fn 本地拆解_下周与晚上() {
        let today = d(2026, 9, 19); // 周六 → 下周一 = 09-21
        let out = extract_todos_local("下周一体检", today);
        assert_eq!(out[0].date, "2026-09-21");
        assert_eq!(out[0].time, None);

        let out2 = extract_todos_local("周五晚上10点写周报", today);
        assert_eq!(out2[0].date, "2026-09-25");
        assert_eq!(out2[0].time, Some("22:00".into()));
    }

    #[test]
    fn 本地拆解_月日与多句() {
        let today = d(2026, 9, 19);
        let out = extract_todos_local("10月1日看升旗。然后大后天交房租", today);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].date, "2026-10-01");
        assert_eq!(out[1].date, "2026-09-22");
        assert_eq!(out[1].title, "交房租");
    }

    #[test]
    fn 本地拆解_无日期退今天_无时间退全天() {
        let today = d(2026, 9, 19);
        let out = extract_todos_local("买菜做饭", today);
        assert_eq!(out[0].date, "2026-09-19");
        assert_eq!(out[0].time, None);
        assert_eq!(out[0].title, "买菜做饭");
    }

    #[test]
    fn 模型输出解析_代码块包裹与非法条目清洗() {
        let text = r#"好的：```json
        [{"title":"开会","date":"2026-09-20","time":"15:00"},
         {"title":"","date":"2026-09-20","time":null},
         {"title":"日期不对的","date":"9月20日","time":null}]
        ```"#;
        let out = parse_todo_array(text);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].title, "开会");
        assert_eq!(out[0].time, Some("15:00".into()));
    }

    #[test]
    fn 模型输出解析_空与废话() {
        assert!(parse_todo_array("没有可拆解的待办").is_empty());
        assert!(parse_todo_array("").is_empty());
    }
}
