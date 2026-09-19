//! 全量数据导出为 Markdown 归档（FR-7.6）：按周期分文件打包为 ZIP
//!
//! 归档结构：
//! ```text
//! mindmate-archive/
//!   README.md                 索引（统计概览）
//!   daily/2026-09-12.md       每日记录（节点 + 当日待办）
//!   weekly/2026-W37.md        每周汇总（按日组织）
//!   monthly/2026-09.md        每月汇总（按周/日组织）
//!   todos.md                  全部待办清单（按状态分组）
//!   reports/…                 AI/模板生成的报告归档
//! ```

use crate::db::{Db, Node, Report, Todo};
use anyhow::Result;
use chrono::{Datelike, NaiveDate};
use std::collections::BTreeMap;
use std::io::{Cursor, Write};
use zip::write::SimpleFileOptions;
use zip::CompressionMethod;

/// 生成 Markdown 归档（ZIP 字节）
pub fn generate_archive(db: &Db) -> Result<Vec<u8>> {
    let nodes = db.list_nodes_range("0000-01-01", "9999-12-31")?;
    let todos = db.list_todos(Some("全部"), Some("全部"), None, None, None)?;
    let reports = db.list_reports(None, 2000)?;

    let mut files: Vec<(String, String)> = Vec::new();

    // ── 按日分文件 ──
    let by_day: BTreeMap<&str, Vec<&Node>> = {
        let mut m: BTreeMap<&str, Vec<&Node>> = BTreeMap::new();
        for n in &nodes {
            m.entry(n.date.as_str()).or_default().push(n);
        }
        m
    };
    for (date, list) in &by_day {
        let day_todos: Vec<&Todo> = todos.iter().filter(|t| t.due_date == *date).collect();
        files.push((
            format!("daily/{date}.md"),
            render_day(date, list, &day_todos),
        ));
    }

    // ── 按周分文件（ISO 周）──
    let mut by_week: BTreeMap<String, Vec<&Node>> = BTreeMap::new();
    for n in &nodes {
        if let Ok(d) = NaiveDate::parse_from_str(&n.date, "%Y-%m-%d") {
            let iso = d.iso_week();
            by_week
                .entry(format!("{}-W{:02}", iso.year(), iso.week()))
                .or_default()
                .push(n);
        }
    }
    for (week, list) in &by_week {
        files.push((format!("weekly/{week}.md"), render_period(week, "周", list, &todos)));
    }

    // ── 按月分文件 ──
    let mut by_month: BTreeMap<String, Vec<&Node>> = BTreeMap::new();
    for n in &nodes {
        if n.date.len() >= 7 {
            by_month.entry(n.date[..7].to_string()).or_default().push(n);
        }
    }
    for (month, list) in &by_month {
        files.push((format!("monthly/{month}.md"), render_period(month, "月", list, &todos)));
    }

    // ── 待办总表 ──
    files.push(("todos.md".into(), render_todos(&todos)));

    // ── 报告归档 ──
    for r in &reports {
        let name = format!(
            "reports/{}-{}-{}.md",
            sanitize(&r.period),
            r.r#type,
            r.created_at.get(0..10).unwrap_or("unknown")
        );
        files.push((name, render_report(r)));
    }

    // ── 索引 ──
    files.insert(
        0,
        (
            "README.md".into(),
            render_index(&nodes, &todos, &by_day, &by_week, &by_month, &reports),
        ),
    );

    // ── 打包为 ZIP ──
    let mut buf = Cursor::new(Vec::new());
    {
        let mut zip = zip::ZipWriter::new(&mut buf);
        let opts = SimpleFileOptions::default()
            .compression_method(CompressionMethod::Deflated)
            .unix_permissions(0o644);
        for (path, content) in &files {
            zip.start_file(format!("mindmate-archive/{path}"), opts)?;
            zip.write_all(content.as_bytes())?;
        }
        zip.finish()?;
    }
    Ok(buf.into_inner())
}

fn render_index(
    nodes: &[Node],
    todos: &[Todo],
    by_day: &BTreeMap<&str, Vec<&Node>>,
    by_week: &BTreeMap<String, Vec<&Node>>,
    by_month: &BTreeMap<String, Vec<&Node>>,
    reports: &[Report],
) -> String {
    let done = todos.iter().filter(|t| t.status == "已完成").count();
    let pending = todos.iter().filter(|t| t.status != "已完成").count();
    let overdue = todos.iter().filter(|t| t.status == "已逾期").count();
    let mut md = String::from("# 智伴 Mindmate 数据归档\n\n");
    md.push_str(&format!(
        "> 导出时间：{}\n\n",
        crate::db::now_string()
    ));
    md.push_str("## 概览\n\n");
    md.push_str(&format!(
        "- 记录节点：**{}** 条\n- 覆盖天数：**{}** 天\n- 待办：共 **{}** 件（已完成 {} · 未完成 {} · 逾期 {}）\n- 报告：**{}** 份\n\n",
        nodes.len(),
        by_day.len(),
        todos.len(),
        done,
        pending,
        overdue,
        reports.len()
    ));
    md.push_str("## 归档结构\n\n");
    md.push_str("| 目录 | 内容 | 文件数 |\n| --- | --- | --- |\n");
    md.push_str(&format!(
        "| `daily/` | 每日记录（按日期） | {} |\n| `weekly/` | 每周汇总（ISO 周） | {} |\n| `monthly/` | 每月汇总 | {} |\n| `reports/` | 生成的报告 | {} |\n| `todos.md` | 待办总表 | 1 |\n",
        by_day.len(),
        by_week.len(),
        by_month.len(),
        reports.len()
    ));
    md.push_str("\n## 时间范围\n\n");
    if let (Some(first), Some(last)) = (by_day.keys().next(), by_day.keys().last()) {
        md.push_str(&format!("- 最早记录：{first}\n- 最近记录：{last}\n"));
    } else {
        md.push_str("- 暂无记录\n");
    }
    md
}

fn render_day(date: &str, nodes: &[&Node], todos: &[&Todo]) -> String {
    let mut md = format!("# {date} 记录\n\n");
    md.push_str(&format!("> 共 {} 个节点\n\n", nodes.len()));
    md.push_str("## 记录节点（按时刻）\n\n");
    if nodes.is_empty() {
        md.push_str("- （无）\n");
    } else {
        for n in nodes {
            let time = n.created_at.get(11..16).unwrap_or("--:--");
            let tags = if n.tags.is_empty() {
                String::new()
            } else {
                format!(" `{}`", n.tags.join("` `"))
            };
            let back = if n.is_backfill { "（补录）" } else { "" };
            md.push_str(&format!("- **{time}**{tags}{back} {}\n", n.content));
        }
    }
    md.push_str("\n## 当日待办\n\n");
    if todos.is_empty() {
        md.push_str("- （无）\n");
    } else {
        for t in todos {
            let mark = if t.status == "已完成" { "x" } else { " " };
            let time = t
                .due_time
                .as_ref()
                .map(|x| format!("{x} "))
                .unwrap_or_default();
            let status = if t.status == "已完成" {
                String::new()
            } else {
                format!(" — {}", t.status)
            };
            md.push_str(&format!("- [{mark}] {time}{}{status}\n", t.title));
        }
    }
    md
}

fn render_period(label: &str, unit: &str, nodes: &[&Node], todos: &[Todo]) -> String {
    let mut by_day: BTreeMap<&str, Vec<&Node>> = BTreeMap::new();
    for n in nodes {
        by_day.entry(n.date.as_str()).or_default().push(n);
    }
    let mut md = format!("# {label} 汇总（{unit}）\n\n");
    md.push_str(&format!(
        "> 记录 {} 条，覆盖 {} 天\n\n",
        nodes.len(),
        by_day.len()
    ));
    for (date, list) in &by_day {
        let weekday = NaiveDate::parse_from_str(date, "%Y-%m-%d")
            .map(|d| match d.weekday() {
                chrono::Weekday::Mon => "周一",
                chrono::Weekday::Tue => "周二",
                chrono::Weekday::Wed => "周三",
                chrono::Weekday::Thu => "周四",
                chrono::Weekday::Fri => "周五",
                chrono::Weekday::Sat => "周六",
                chrono::Weekday::Sun => "周日",
            })
            .unwrap_or("");
        md.push_str(&format!("## {date} {weekday}\n\n"));
        for n in list {
            let time = n.created_at.get(11..16).unwrap_or("--:--");
            md.push_str(&format!("- `{time}` {}\n", n.content));
        }
        // 该日待办
        let day_todos: Vec<&Todo> = todos.iter().filter(|t| t.due_date == *date).collect();
        if !day_todos.is_empty() {
            md.push_str("\n待办：\n");
            for t in day_todos {
                let mark = if t.status == "已完成" { "x" } else { " " };
                md.push_str(&format!("- [{mark}] {}\n", t.title));
            }
        }
        md.push('\n');
    }
    md
}

fn render_todos(todos: &[Todo]) -> String {
    let mut md = String::from("# 待办总表\n\n");
    let groups: [(&str, Vec<&Todo>); 3] = [
        (
            "未完成",
            todos
                .iter()
                .filter(|t| t.status == "待处理" || t.status == "进行中")
                .collect(),
        ),
        ("已逾期", todos.iter().filter(|t| t.status == "已逾期").collect()),
        ("已完成", todos.iter().filter(|t| t.status == "已完成").collect()),
    ];
    for (name, list) in groups {
        md.push_str(&format!("## {name}（{}）\n\n", list.len()));
        if list.is_empty() {
            md.push_str("- （无）\n\n");
            continue;
        }
        md.push_str("| 标题 | 分类 | 截止 | 优先级 | 标签 | 状态 |\n| --- | --- | --- | --- | --- | --- |\n");
        for t in list {
            md.push_str(&format!(
                "| {} | {} | {}{} | {} | {} | {} |\n",
                t.title.replace('|', "\\|"),
                t.category,
                t.due_date,
                t.due_time
                    .as_ref()
                    .map(|x| format!(" {x}"))
                    .unwrap_or_default(),
                t.priority,
                t.tags.join("/"),
                t.status
            ));
        }
        md.push('\n');
    }
    md
}

fn render_report(r: &Report) -> String {
    format!(
        "> 类型：{} · 周期：{} · 生成方式：{} · 生成时间：{}\n\n{}\n",
        r.r#type,
        r.period,
        if r.is_ai { "AI 生成" } else { "本地模板" },
        r.created_at,
        r.content
    )
}

fn sanitize(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '-' })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{NewNode, NewTodo};

    fn seed() -> Db {
        let db = Db::open_memory().unwrap();
        db.seed_defaults().unwrap();
        db.create_node(NewNode {
            content: "完成 A 模块开发".into(),
            date: Some("2026-09-12".into()),
            tags: vec!["工作".into()],
            todo_id: None,
        })
        .unwrap();
        db.create_node(NewNode {
            content: "晨跑 5km".into(),
            date: Some("2026-09-13".into()),
            tags: vec!["健康".into()],
            todo_id: None,
        })
        .unwrap();
        let t = db
            .create_todo(NewTodo {
                title: "写技术方案".into(),
                description: String::new(),
                due_date: Some("2026-09-12".into()),
                due_time: None,
                priority: "高".into(),
                tags: vec!["工作".into()],
                remind_offset_min: None,
                remind_at: None,
                recur_type: String::new(),
                recur_until: String::new(),
                recur_interval: 1,
                recur_skip_rest: false,
            })
            .unwrap();
        db.complete_todo(t.id, true).unwrap();
        db.save_report("daily", "2026-09-12", "# 2026-09-12 日报\n\n内容", true)
            .unwrap();
        db
    }

    fn read_zip(bytes: &[u8]) -> BTreeMap<String, String> {
        let mut out = BTreeMap::new();
        let reader = std::io::Cursor::new(bytes);
        let mut zip = zip::ZipArchive::new(reader).unwrap();
        for i in 0..zip.len() {
            let mut f = zip.by_index(i).unwrap();
            let name = f.name().to_string();
            let mut s = String::new();
            std::io::Read::read_to_string(&mut f, &mut s).unwrap();
            out.insert(name, s);
        }
        out
    }

    #[test]
    fn 归档为合法zip且按周期分文件() {
        let db = seed();
        let bytes = generate_archive(&db).unwrap();
        let files = read_zip(&bytes);
        let names: Vec<&str> = files.keys().map(|s| s.as_str()).collect();

        assert!(names.iter().any(|n| n.ends_with("README.md")), "{names:?}");
        assert!(names.iter().any(|n| n.contains("daily/2026-09-12.md")), "{names:?}");
        assert!(names.iter().any(|n| n.contains("daily/2026-09-13.md")), "{names:?}");
        assert!(names.iter().any(|n| n.contains("weekly/2026-W37.md")), "{names:?}");
        assert!(names.iter().any(|n| n.contains("monthly/2026-09.md")), "{names:?}");
        assert!(names.iter().any(|n| n.ends_with("todos.md")), "{names:?}");
        assert!(names.iter().any(|n| n.starts_with("mindmate-archive/reports/")), "{names:?}");
    }

    #[test]
    fn 日文件包含节点时刻与待办完成态() {
        let db = seed();
        let files = read_zip(&generate_archive(&db).unwrap());
        let day = files
            .iter()
            .find(|(k, _)| k.ends_with("daily/2026-09-12.md"))
            .map(|(_, v)| v.clone())
            .expect("缺少日文件");
        assert!(day.contains("完成 A 模块开发"), "{day}");
        assert!(day.contains("`工作`"), "{day}");
        assert!(day.contains("- [x] 写技术方案"), "{day}");
    }

    #[test]
    fn 索引含概览统计() {
        let db = seed();
        let files = read_zip(&generate_archive(&db).unwrap());
        let index = files
            .iter()
            .find(|(k, _)| k.ends_with("README.md"))
            .map(|(_, v)| v.clone())
            .unwrap();
        assert!(index.contains("记录节点：**2**"), "{index}");
        assert!(index.contains("覆盖天数：**2**"), "{index}");
        assert!(index.contains("已完成 1"), "{index}");
    }

    #[test]
    fn 报告归档文件带元信息() {
        let db = seed();
        let files = read_zip(&generate_archive(&db).unwrap());
        let rep = files
            .iter()
            .find(|(k, _)| k.contains("reports/"))
            .map(|(_, v)| v.clone())
            .expect("缺少报告文件");
        assert!(rep.contains("AI 生成"), "{rep}");
        assert!(rep.contains("2026-09-12 日报"), "{rep}");
    }

    #[test]
    fn 空数据也能生成合法归档() {
        let db = Db::open_memory().unwrap();
        db.seed_defaults().unwrap();
        let bytes = generate_archive(&db).unwrap();
        assert!(bytes.len() > 100, "zip 过小: {}", bytes.len());
        let files = read_zip(&bytes);
        assert!(files.keys().any(|k| k.ends_with("README.md")));
        assert!(files.keys().any(|k| k.ends_with("todos.md")));
    }
}
