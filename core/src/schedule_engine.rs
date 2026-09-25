//! 智能排期引擎（v1.5.0）：把收集箱 + 未排期待办按 优先级 × 截止 × 每日容量
//! 铺进未来 N 个工作日。只产**建议**（预览），用户确认后批量应用，可整体撤销——
//! AI/引擎永不直接改用户日程（信任底线）。

use chrono::{Datelike, NaiveDate};

/// 一条排期建议
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanItem {
    pub todo_id: i64,
    pub title: String,
    pub from_date: String,
    pub to_date: String,
    pub priority: String,
    /// 建议理由（给预览界面看）
    pub reason: String,
}

/// 排期计划（预览）
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SchedulePlan {
    pub items: Vec<PlanItem>,
    /// 每日容量
    pub daily_capacity: i64,
    /// 生成时间
    pub generated_at: String,
}

/// 优先级权重（高先排）
fn priority_weight(p: &str) -> i64 {
    match p {
        "高" => 0,
        "中" => 1,
        _ => 2,
    }
}

/// 是否工作日（周末不算；法定休假由调用方传入集合，引擎内简化为周末规则+提供顺延钩子）
fn is_workday(d: NaiveDate) -> bool {
    d.weekday().num_days_from_monday() < 5
}

/// 从未来 start 起，产出最多 horizon_days 天里的工作日列表
fn workdays_from(start: NaiveDate, horizon_days: i64, max_needed: usize) -> Vec<NaiveDate> {
    let mut out = Vec::new();
    let mut d = start;
    let mut scanned = 0;
    while scanned < horizon_days && out.len() < max_needed {
        if is_workday(d) {
            out.push(d);
        }
        d += chrono::Duration::days(1);
        scanned += 1;
    }
    out
}

/// 引擎输入：待办的(id/标题/优先级/当前日期)；容量与起点由调用方给
pub struct EngineInput<'a> {
    pub todos: &'a [(i64, String, String, String)], // (id, title, priority, current_date)
    pub daily_capacity: i64,
    pub start: NaiveDate,
    /// 有明确截止（current_date 晚于今天=已排期未来）的保持不动
    pub keep_future: bool,
}

/// 生成排期计划：排序（高优先→近截止）后按容量顺次填入工作日。
/// 已排在未来工作日的条目（keep_future=true）保留原位不重排。
pub fn build_plan(input: &EngineInput) -> SchedulePlan {
    let capacity = input.daily_capacity.max(1);
    // 待排 = 收集箱（无日期语义）+ 逾期/今天未完成；未来的不动
    let mut to_schedule: Vec<&(i64, String, String, String)> = input
        .todos
        .iter()
        .filter(|(_, _, _, date)| {
            if input.keep_future {
                // 未来日期的保留；过去与今天的进排期
                date.as_str() > input.start.format("%Y-%m-%d").to_string().as_str()
            } else {
                false
            }
        })
        .collect();
    let _ = &mut to_schedule;
    let mut pending: Vec<&(i64, String, String, String)> = input
        .todos
        .iter()
        .filter(|t| !input.keep_future || t.3.as_str() <= input.start.format("%Y-%m-%d").to_string().as_str())
        .collect();
    // 排序：高优先 → 截止近（当日期） → id
    pending.sort_by(|a, b| {
        priority_weight(&a.2)
            .cmp(&priority_weight(&b.2))
            .then(a.3.cmp(&b.3))
            .then(a.0.cmp(&b.0))
    });
    let _ = to_schedule.len();

    let workdays = workdays_from(input.start, 30, (pending.len() / capacity as usize) + 7);
    let mut items = Vec::new();
    let mut day_idx = 0usize;
    let mut used_today = 0i64;
    for t in &pending {
        // 找一个还有容量的工作日
        while day_idx < workdays.len() && used_today >= capacity {
            day_idx += 1;
            used_today = 0;
        }
        let Some(&day) = workdays.get(day_idx) else { break };
        used_today += 1;
        let from = t.3.clone();
        let to = day.format("%Y-%m-%d").to_string();
        let reason = if from == to {
            "保持原日期".to_string()
        } else if t.3.as_str() < input.start.format("%Y-%m-%d").to_string().as_str() {
            format!("逾期顺延（原 {}）", from)
        } else if from == input.start.format("%Y-%m-%d").to_string() {
            format!("今日未完成，顺延（{} 优先）", priority_label(&t.2))
        } else {
            format!("按{}优先级排入", priority_label(&t.2))
        };
        items.push(PlanItem {
            todo_id: t.0,
            title: t.1.clone(),
            from_date: from,
            to_date: to,
            priority: t.2.clone(),
            reason,
        });
    }
    SchedulePlan {
        items,
        daily_capacity: capacity,
        generated_at: crate::db::now_string(),
    }
}

fn priority_label(p: &str) -> &'static str {
    match p {
        "高" => "高",
        "中" => "中",
        _ => "低",
    }
}

/// 逾期顺延建议：最近工作日（跳过周末）
pub fn next_workday(from: NaiveDate) -> NaiveDate {
    let mut d = from;
    while !is_workday(d) {
        d += chrono::Duration::days(1);
    }
    d
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    fn todos(list: &[(i64, &str, &str, &str)]) -> Vec<(i64, String, String, String)> {
        list.iter()
            .map(|(id, t, p, dt)| (*id, t.to_string(), p.to_string(), dt.to_string()))
            .collect()
    }

    /// 2026-09-28 是周一
    #[test]
    fn 排期_高优先先排_容量切日() {
        let data = todos(&[
            (1, "低优先事项", "低", "2026-09-28"),
            (2, "高优先事项", "高", "2026-09-28"),
            (3, "中优先事项", "中", "2026-09-28"),
            (4, "高优先2", "高", "2026-09-28"),
        ]);
        let plan = build_plan(&EngineInput {
            todos: &data,
            daily_capacity: 2,
            start: d("2026-09-28"),
            keep_future: false,
        });
        assert_eq!(plan.items.len(), 4);
        // 前两条（容量 2）应是高优先
        assert_eq!(plan.items[0].todo_id, 2);
        assert_eq!(plan.items[1].todo_id, 4);
        // 第 3、4 条溢出到下一工作日
        assert_eq!(plan.items[2].to_date, "2026-09-29");
        // 低优先排最后
        assert_eq!(plan.items[3].todo_id, 1);
    }

    #[test]
    fn 排期_未来日期保持不动() {
        let data = todos(&[
            (1, "已排下周", "中", "2026-10-05"),
            (2, "逾期事项", "中", "2026-09-20"),
        ]);
        let plan = build_plan(&EngineInput {
            todos: &data,
            daily_capacity: 5,
            start: d("2026-09-28"),
            keep_future: true,
        });
        assert_eq!(plan.items.len(), 1, "只重排逾期/今天");
        assert_eq!(plan.items[0].todo_id, 2);
        assert_eq!(plan.items[0].from_date, "2026-09-20");
        assert!(plan.items[0].reason.contains("逾期顺延"));
    }

    #[test]
    fn 排期_跳过周末() {
        // 2026-09-26 周六：9/28 周一起排
        let data = todos(&[(1, "周六收集的事", "中", "2026-09-26")]);
        let plan = build_plan(&EngineInput {
            todos: &data,
            daily_capacity: 5,
            start: d("2026-09-26"),
            keep_future: false,
        });
        assert_eq!(plan.items[0].to_date, "2026-09-28", "周六排期应落到周一");
    }

    #[test]
    fn 顺延_最近工作日() {
        // 周六 2026-09-26 顺延 → 周一 2026-09-28
        assert_eq!(next_workday(d("2026-09-26")), d("2026-09-28"));
        // 工作日原样
        assert_eq!(next_workday(d("2026-09-28")), d("2026-09-28"));
    }

    #[test]
    fn 排期_理由包含原日期() {
        let data = todos(&[(1, "逾期事", "高", "2026-09-15")]);
        let plan = build_plan(&EngineInput {
            todos: &data,
            daily_capacity: 3,
            start: d("2026-09-28"),
            keep_future: false,
        });
        assert!(plan.items[0].reason.contains("2026-09-15"));
    }
}
