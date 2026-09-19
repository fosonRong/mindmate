//! 全部查询与写入：节点、待办、设置、报告、聊天、统计、成就

use super::models::*;
use super::Db;
use anyhow::Result;
use chrono::{Datelike, Duration, Local, NaiveDate};
use rusqlite::{params, OptionalExtension};

fn parse_tags(s: &str) -> Vec<String> {
    serde_json::from_str(s).unwrap_or_default()
}

fn row_to_node(row: &rusqlite::Row) -> rusqlite::Result<Node> {
    Ok(Node {
        id: row.get(0)?,
        content: row.get(1)?,
        date: row.get(2)?,
        created_at: row.get(3)?,
        updated_at: row.get(4)?,
        is_backfill: row.get::<_, i64>(5)? != 0,
        tags: parse_tags(&row.get::<_, String>(6)?),
        todo_id: row.get(7)?,
    })
}

const NODE_COLS: &str = "id, content, date, created_at, updated_at, is_backfill, tags, todo_id";

impl Db {
    // ───────────────────────── 节点 ─────────────────────────

    pub fn create_node(&self, input: NewNode) -> Result<Node> {
        let now = now_string();
        let today = today_string();
        let date = input.date.clone().unwrap_or_else(|| today.clone());
        let is_backfill = if date != today { 1 } else { 0 };
        let tags = serde_json::to_string(&input.tags)?;
        let conn = self.lock();
        conn.execute(
            "INSERT INTO nodes(content, date, created_at, updated_at, is_backfill, tags, todo_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![input.content, date, now, now, is_backfill, tags, input.todo_id],
        )?;
        let id = conn.last_insert_rowid();
        let node = conn.query_row(
            &format!("SELECT {NODE_COLS} FROM nodes WHERE id = ?1"),
            params![id],
            row_to_node,
        )?;
        Ok(node)
    }

    pub fn list_nodes_by_date(&self, date: &str) -> Result<Vec<Node>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(&format!(
            "SELECT {NODE_COLS} FROM nodes WHERE date = ?1 AND deleted_at IS NULL ORDER BY created_at ASC"
        ))?;
        let rows = stmt.query_map(params![date], row_to_node)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// 周期内的节点（周/月视图一次拉取）
    pub fn list_nodes_range(&self, from: &str, to: &str) -> Result<Vec<Node>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(&format!(
            "SELECT {NODE_COLS} FROM nodes
             WHERE date >= ?1 AND date <= ?2 AND deleted_at IS NULL
             ORDER BY date ASC, created_at ASC"
        ))?;
        let rows = stmt.query_map(params![from, to], row_to_node)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn get_node(&self, id: i64) -> Result<Option<Node>> {
        let conn = self.lock();
        let node = conn
            .query_row(
                &format!("SELECT {NODE_COLS} FROM nodes WHERE id = ?1 AND deleted_at IS NULL"),
                params![id],
                row_to_node,
            )
            .optional()?;
        Ok(node)
    }

    pub fn update_node(&self, id: i64, patch: NodePatch) -> Result<Option<Node>> {
        let Some(mut node) = self.get_node(id)? else {
            return Ok(None);
        };
        if let Some(c) = patch.content {
            node.content = c;
        }
        if let Some(t) = patch.tags {
            node.tags = t;
        }
        if let Some(tid) = patch.todo_id {
            node.todo_id = tid;
        }
        let now = now_string();
        let conn = self.lock();
        conn.execute(
            "UPDATE nodes SET content=?1, tags=?2, todo_id=?3, updated_at=?4 WHERE id=?5",
            params![
                node.content,
                serde_json::to_string(&node.tags)?,
                node.todo_id,
                now,
                id
            ],
        )?;
        node.updated_at = now;
        Ok(Some(node))
    }

    pub fn delete_node(&self, id: i64) -> Result<bool> {
        let conn = self.lock();
        let n = conn.execute(
            "UPDATE nodes SET deleted_at = ?1 WHERE id = ?2 AND deleted_at IS NULL",
            params![now_string(), id],
        )?;
        Ok(n > 0)
    }

    /// 周期内每天的节点数（连续记录计算用）
    pub fn node_dates_desc(&self, limit_days: i64) -> Result<Vec<String>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT DISTINCT date FROM nodes WHERE deleted_at IS NULL ORDER BY date DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![limit_days], |r| r.get::<_, String>(0))?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// 范围内每日节点计数（成就判定用）
    pub fn node_counts_by_range(
        &self,
        from: &str,
        to: &str,
    ) -> Result<std::collections::BTreeMap<String, i64>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT date, COUNT(*) FROM nodes
             WHERE deleted_at IS NULL AND date >= ?1 AND date <= ?2
             GROUP BY date ORDER BY date",
        )?;
        let rows = stmt.query_map(params![from, to], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
        })?;
        let mut map = std::collections::BTreeMap::new();
        for row in rows {
            let (date, count) = row?;
            map.insert(date, count);
        }
        Ok(map)
    }

    /// 全文检索（智伴问答"我周三记了什么"用）
    ///
    /// 说明：SQLite FTS5 默认分词器（unicode61）不切分中文，无法做中文子串匹配；
    /// 个人数据量（万级）下 LIKE 扫描更快且中文友好，故此处使用 LIKE。
    pub fn search_nodes(&self, query: &str, limit: i64) -> Result<Vec<Node>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(&format!(
            "SELECT {NODE_COLS} FROM nodes
             WHERE deleted_at IS NULL AND content LIKE ?1
             ORDER BY date DESC, created_at DESC LIMIT ?2"
        ))?;
        let rows = stmt.query_map(params![format!("%{query}%"), limit], row_to_node)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    // ───────────────────────── 待办 ─────────────────────────

    fn row_to_todo(row: &rusqlite::Row) -> rusqlite::Result<Todo> {
        let due_date: String = row.get(3)?;
        let due_time: Option<String> = row.get(4)?;
        let status: String = row.get(8)?;
        Ok(Todo {
            id: row.get(0)?,
            title: row.get(1)?,
            description: row.get(2)?,
            due_date: due_date.clone(),
            due_time: due_time.clone(),
            remind_at: row.get(5)?,
            priority: row.get(6)?,
            tags: parse_tags(&row.get::<_, String>(7)?),
            status: status.clone(),
            category: row.get(9)?,
            sort_order: row.get(10)?,
            created_at: row.get(11)?,
            updated_at: row.get(12)?,
            completed_at: row.get(13)?,
            recur_type: row.get(14)?,
            recur_anchor: row.get(15)?,
            recur_source_id: row.get(16)?,
            recur_until: row.get(17)?,
            recur_interval: row.get(18)?,
            recur_skip_rest: row.get::<_, i64>(19)? != 0,
            overdue: is_overdue(&due_date, due_time.as_deref(), &status),
        })
    }

    const TODO_COLS: &'static str = "id, title, description, due_date, due_time, remind_at, priority, tags, status, category, sort_order, created_at, updated_at, completed_at, recur_type, recur_anchor, recur_source_id, recur_until, recur_interval, recur_skip_rest";

    pub fn create_todo(&self, input: NewTodo) -> Result<Todo> {
        let now = now_string();
        let today = today_string();
        let due_date = input.due_date.clone().unwrap_or_else(|| today.clone());
        let category = classify(&due_date, input.due_time.as_deref(), &today);
        // 提醒时刻：优先显式 remind_at；否则按提前 N 分钟（或全局默认）计算
        let remind_at = match (&input.remind_at, &input.due_time) {
            (Some(ra), _) => Some(ra.clone()),
            (None, Some(t)) => {
                let offset = input.remind_offset_min.unwrap_or_else(|| {
                    self.get_setting("todo_remind_offset_min")
                        .ok()
                        .flatten()
                        .and_then(|v| v.parse().ok())
                        .unwrap_or(30)
                });
                compute_remind_at(&due_date, t, offset)
            }
            _ => None,
        };
        let tags = serde_json::to_string(&input.tags)?;
        // 循环待办：recur_type 合法性收敛（只认 weekly/monthly），锚点=首个实例的截止日期
        let recur_type = normalize_recur_type(&input.recur_type);
        let conn = self.lock();
        let recur_until = normalize_date_opt(&input.recur_until);
        let recur_interval = if input.recur_interval < 1 { 1 } else { input.recur_interval };
        conn.execute(
            "INSERT INTO todos(title, description, due_date, due_time, remind_at, priority, tags, status, category, sort_order, created_at, updated_at, recur_type, recur_anchor, recur_until, recur_interval, recur_skip_rest)
             VALUES (?1,?2,?3,?4,?5,?6,?7,'待处理',?8,0,?9,?9,?10,?3,?11,?12,?13)",
            params![input.title, input.description, due_date, input.due_time, remind_at, input.priority, tags, category, now, recur_type, recur_until, recur_interval, input.recur_skip_rest as i64],
        )?;
        let id = conn.last_insert_rowid();
        let todo = conn.query_row(
            &format!("SELECT {} FROM todos WHERE id = ?1", Self::TODO_COLS),
            params![id],
            Self::row_to_todo,
        )?;
        Ok(todo)
    }

    pub fn list_todos(
        &self,
        category: Option<&str>,
        status: Option<&str>,
        priority: Option<&str>,
        tag: Option<&str>,
        q: Option<&str>,
    ) -> Result<Vec<Todo>> {
        let mut sql = format!(
            "SELECT {} FROM todos WHERE deleted_at IS NULL",
            Self::TODO_COLS
        );
        let mut args: Vec<String> = vec![];
        if let Some(c) = category {
            if !c.is_empty() && c != "全部" {
                sql.push_str(&format!(" AND category = ?{}", args.len() + 1));
                args.push(c.to_string());
            }
        }
        if let Some(s) = status {
            if !s.is_empty() && s != "全部" {
                sql.push_str(&format!(" AND status = ?{}", args.len() + 1));
                args.push(s.to_string());
            }
        }
        if let Some(p) = priority {
            if !p.is_empty() && p != "全部" {
                sql.push_str(&format!(" AND priority = ?{}", args.len() + 1));
                args.push(p.to_string());
            }
        }
        if let Some(t) = tag {
            if !t.is_empty() {
                sql.push_str(&format!(" AND tags LIKE ?{}", args.len() + 1));
                args.push(format!("%\"{t}\"%"));
            }
        }
        if let Some(kw) = q {
            if !kw.is_empty() {
                sql.push_str(&format!(
                    " AND (title LIKE ?{0} OR description LIKE ?{0})",
                    args.len() + 1
                ));
                args.push(format!("%{kw}%"));
            }
        }
        sql.push_str(" ORDER BY CASE status WHEN '已逾期' THEN 0 WHEN '进行中' THEN 1 WHEN '待处理' THEN 2 ELSE 3 END, due_date ASC, due_time ASC, sort_order ASC, id DESC");

        let conn = self.lock();
        let mut stmt = conn.prepare(&sql)?;
        let params_ref: Vec<&dyn rusqlite::ToSql> =
            args.iter().map(|s| s as &dyn rusqlite::ToSql).collect();
        let rows = stmt.query_map(params_ref.as_slice(), Self::row_to_todo)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn get_todo(&self, id: i64) -> Result<Option<Todo>> {
        let conn = self.lock();
        let t = conn
            .query_row(
                &format!(
                    "SELECT {} FROM todos WHERE id=?1 AND deleted_at IS NULL",
                    Self::TODO_COLS
                ),
                params![id],
                Self::row_to_todo,
            )
            .optional()?;
        Ok(t)
    }

    pub fn update_todo(&self, id: i64, patch: TodoPatch) -> Result<Option<Todo>> {
        let Some(mut t) = self.get_todo(id)? else {
            return Ok(None);
        };
        if let Some(v) = patch.title {
            t.title = v;
        }
        if let Some(v) = patch.description {
            t.description = v;
        }
        if let Some(v) = patch.due_date {
            t.due_date = v;
        }
        if let Some(v) = patch.due_time {
            t.due_time = v;
        }
        if let Some(v) = patch.remind_at {
            t.remind_at = v;
        }
        if let Some(v) = patch.priority {
            t.priority = v;
        }
        if let Some(v) = patch.tags {
            t.tags = v;
        }
        if let Some(v) = patch.status {
            t.status = v;
        }
        if let Some(v) = patch.sort_order {
            t.sort_order = v;
        }
        if let Some(v) = patch.recur_type {
            t.recur_type = normalize_recur_type(&v);
        }
        if let Some(v) = patch.recur_until {
            t.recur_until = normalize_date_opt(&v);
        }
        if let Some(v) = patch.recur_interval {
            t.recur_interval = if v < 1 { 1 } else { v };
        }
        if let Some(v) = patch.recur_skip_rest {
            t.recur_skip_rest = v;
        }
        let today = today_string();
        // 归类：显式指定优先，否则按截止日期重算
        t.category = patch
            .category
            .unwrap_or_else(|| classify(&t.due_date, t.due_time.as_deref(), &today));
        let now = now_string();
        let conn = self.lock();
        conn.execute(
            "UPDATE todos SET title=?1, description=?2, due_date=?3, due_time=?4, remind_at=?5,
                    priority=?6, tags=?7, status=?8, category=?9, sort_order=?10, updated_at=?11,
                    recur_type=?12, recur_until=?13, recur_interval=?14, recur_skip_rest=?15
             WHERE id=?16",
            params![
                t.title,
                t.description,
                t.due_date,
                t.due_time,
                t.remind_at,
                t.priority,
                serde_json::to_string(&t.tags)?,
                t.status,
                t.category,
                t.sort_order,
                now,
                t.recur_type,
                t.recur_until,
                t.recur_interval,
                t.recur_skip_rest as i64,
                id
            ],
        )?;
        t.updated_at = now;
        t.overdue = is_overdue(&t.due_date, t.due_time.as_deref(), &t.status);
        Ok(Some(t))
    }

    pub fn complete_todo(&self, id: i64, done: bool) -> Result<Option<Todo>> {
        let now = now_string();
        let conn = self.lock();
        if done {
            conn.execute(
                "UPDATE todos SET status='已完成', completed_at=?1, updated_at=?1 WHERE id=?2 AND deleted_at IS NULL",
                params![now, id],
            )?;
        } else {
            let today = today_string();
            let (due_date, due_time): (String, Option<String>) = conn
                .query_row(
                    "SELECT due_date, due_time FROM todos WHERE id=?1",
                    params![id],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()?
                .unwrap_or((today.clone(), None));
            let status = if is_overdue(&due_date, due_time.as_deref(), "待处理") {
                "已逾期"
            } else {
                "待处理"
            };
            conn.execute(
                "UPDATE todos SET status=?1, completed_at=NULL, updated_at=?2 WHERE id=?3 AND deleted_at IS NULL",
                params![status, now, id],
            )?;
        }
        drop(conn);
        self.get_todo(id)
    }

    pub fn delete_todo(&self, id: i64) -> Result<bool> {
        let conn = self.lock();
        let n = conn.execute(
            "UPDATE todos SET deleted_at=?1 WHERE id=?2 AND deleted_at IS NULL",
            params![now_string(), id],
        )?;
        Ok(n > 0)
    }

    /// 删除整条循环链（根 + 已生成的全部实例），并物理阻止补期引擎再生成：
    /// 软删后把 recur_type 置空，确保即使有漏网判断也不会再补期。
    /// 返回删除的条数；对非循环待办等价于删除单条。
    pub fn delete_todo_series(&self, id: i64) -> Result<usize> {
        let now = now_string();
        let conn = self.lock();
        let root: Option<i64> = conn
            .query_row(
                "SELECT CASE WHEN recur_source_id IS NULL THEN id ELSE recur_source_id END
                 FROM todos WHERE id=?1 AND deleted_at IS NULL",
                params![id],
                |r| r.get(0),
            )
            .optional()?;
        let Some(root) = root else { return Ok(0) };
        let n = conn.execute(
            "UPDATE todos SET deleted_at=?1, updated_at=?1, recur_type=''
             WHERE deleted_at IS NULL AND (id=?2 OR recur_source_id=?2)",
            params![now, root],
        )?;
        Ok(n)
    }

    /// 刷新逾期状态：把已过截止时间且未完成的待办标记为「已逾期」
    pub fn refresh_overdue(&self) -> Result<Vec<Todo>> {
        let now = Local::now();
        let today = now.format("%Y-%m-%d").to_string();
        let time = now.format("%H:%M").to_string();
        let conn = self.lock();
        conn.execute(
            "UPDATE todos SET status='已逾期', updated_at=?1
             WHERE deleted_at IS NULL AND status IN ('待处理','进行中')
               AND (due_date < ?2 OR (due_date = ?2 AND due_time IS NOT NULL AND due_time < ?3))",
            params![now_string(), today, time],
        )?;
        drop(conn);
        // 返回当前所有逾期项（供提醒文案使用）
        self.list_todos(Some("全部"), Some("已逾期"), None, None, None)
    }

    /// 某日【日程 + 待办】双栏数据
    pub fn schedule_for_date(&self, date: &str) -> Result<(Vec<Todo>, Vec<Todo>)> {
        let conn = self.lock();
        let mut stmt = conn.prepare(&format!(
            "SELECT {} FROM todos WHERE deleted_at IS NULL AND due_date=?1 AND due_time IS NOT NULL
             ORDER BY due_time ASC",
            Self::TODO_COLS
        ))?;
        let schedules = stmt
            .query_map(params![date], Self::row_to_todo)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        drop(stmt);
        let mut stmt2 = conn.prepare(&format!(
            "SELECT {} FROM todos WHERE deleted_at IS NULL AND due_date=?1 ORDER BY sort_order ASC, id ASC",
            Self::TODO_COLS
        ))?;
        let todos = stmt2
            .query_map(params![date], Self::row_to_todo)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok((schedules, todos))
    }

    /// 到期待提醒的待办（remind_at <= now 且未提醒过、未完成）
    pub fn due_todo_reminders(&self, now: &str) -> Result<Vec<Todo>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(&format!(
            "SELECT {} FROM todos
             WHERE deleted_at IS NULL AND status IN ('待处理','进行中','已逾期')
               AND remind_at IS NOT NULL AND remind_at <= ?1 AND reminded_at IS NULL
             ORDER BY remind_at ASC LIMIT 20",
            Self::TODO_COLS
        ))?;
        let rows = stmt.query_map(params![now], Self::row_to_todo)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn mark_todo_reminded(&self, id: i64) -> Result<()> {
        let conn = self.lock();
        conn.execute(
            "UPDATE todos SET reminded_at=?1 WHERE id=?2",
            params![now_string(), id],
        )?;
        Ok(())
    }

    // ───────────────────────── 循环待办（每周/每月自动生成） ─────────────────────────

    /// 为每条循环链保证「存在一个未完成且 due_date >= 今天的实例」，没有就生成下一期。
    /// 生成规则：以链上最新实例的 due_date 为基点按周期推进，直到不早于今天；
    /// 幂等（已有未来实例/同日实例就跳过），启动与每次完成/改动后都可安全调用。
    pub fn ensure_recurring(&self) -> Result<Vec<Todo>> {
        let today = today_string();
        let cols = Self::TODO_COLS;
        let mut created = Vec::new();

        let roots: Vec<Todo> = {
            let conn = self.lock();
            let mut stmt = conn.prepare(&format!(
                "SELECT {cols} FROM todos
                 WHERE deleted_at IS NULL AND recur_type != '' AND recur_source_id IS NULL"
            ))?;
            let rows: Vec<Todo> = stmt
                .query_map([], Self::row_to_todo)?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            rows
        };

        for root in roots {
            let (latest_due, _latest_status): (String, String) = {
                let conn = self.lock();
                let r = conn
                    .query_row(
                        &format!(
                            "SELECT due_date, status FROM todos
                             WHERE deleted_at IS NULL AND (id = ?1 OR recur_source_id = ?1)
                             ORDER BY due_date DESC, id DESC LIMIT 1"
                        ),
                        params![root.id],
                        |row| Ok((row.get(0)?, row.get(1)?)),
                    )
                    .optional()?;
                match r {
                    Some(v) => v,
                    None => continue,
                }
            };
            // 链上已存在未完成的未来实例 → 下一期已在，无需生成
            let has_pending = {
                let conn = self.lock();
                conn.query_row(
                    "SELECT EXISTS(SELECT 1 FROM todos
                      WHERE deleted_at IS NULL AND (id = ?1 OR recur_source_id = ?1)
                        AND status != '已完成' AND due_date >= ?2)",
                    params![root.id, today],
                    |row| row.get::<_, i64>(0),
                )? > 0
            };
            if has_pending {
                continue;
            }
            // 下一期从链上最新一条之后推（已完成的那条也算「最新」）
            let Some(next_date) = next_recur_date(
                &root.recur_type,
                &root.recur_anchor,
                &latest_due,
                &today,
                root.recur_interval,
                &root.recur_until,
                root.recur_skip_rest,
            ) else {
                continue;
            };
            // 同根同日已有实例（用户手动补建等）：跳过，避免重复
            let dup = {
                let conn = self.lock();
                conn.query_row(
                    "SELECT EXISTS(SELECT 1 FROM todos
                      WHERE deleted_at IS NULL AND (id = ?1 OR recur_source_id = ?1) AND due_date = ?2)",
                    params![root.id, next_date],
                    |row| row.get::<_, i64>(0),
                )? > 0
            };
            if dup {
                continue;
            }
            // 复制根实例的内容属性；提醒时刻按原规则重算
            let remind_at = root.due_time.as_deref().and_then(|tt| {
                let offset = self
                    .get_setting("todo_remind_offset_min")
                    .ok()
                    .flatten()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(30);
                compute_remind_at(&next_date, tt, offset)
            });
            let tags = serde_json::to_string(&root.tags)?;
            let category = classify(&next_date, root.due_time.as_deref(), &today);
            let now = now_string();
            let id = {
                let conn = self.lock();
                conn.execute(
                    "INSERT INTO todos(title, description, due_date, due_time, remind_at, priority, tags, status, category, sort_order, created_at, updated_at, recur_type, recur_anchor, recur_source_id, recur_until, recur_interval, recur_skip_rest)
                     VALUES (?1,?2,?3,?4,?5,?6,?7,'待处理',?8,0,?9,?9,?10,?11,?12,?13,?14,?15)",
                    params![
                        root.title,
                        root.description,
                        next_date,
                        root.due_time,
                        remind_at,
                        root.priority,
                        tags,
                        category,
                        now,
                        root.recur_type,
                        root.recur_anchor,
                        root.id,
                        root.recur_until,
                        root.recur_interval,
                        root.recur_skip_rest as i64
                    ],
                )?;
                conn.last_insert_rowid()
            };
            if let Some(todo) = self.get_todo(id)? {
                created.push(todo);
            }
        }
        Ok(created)
    }

    // ───────────────────────── 设置 ─────────────────────────

    /// 数据库里最早一条记录的时间（用于老用户取证回填：判断本机何时开始使用）
    pub fn earliest_record_at(&self) -> Result<Option<String>> {
        let conn = self.lock();
        let v: Option<String> = conn.query_row(
            "SELECT MIN(t) FROM (
                 SELECT MIN(created_at) AS t FROM nodes
                 UNION ALL SELECT MIN(created_at) FROM todos
                 UNION ALL SELECT MIN(created_at) FROM reports
                 UNION ALL SELECT MIN(updated_at) FROM settings
             )",
            [],
            |r| r.get(0),
        )?;
        Ok(v.filter(|s| !s.is_empty()))
    }

    pub fn get_setting(&self, key: &str) -> Result<Option<String>> {
        let conn = self.lock();
        let v = conn
            .query_row(
                "SELECT value FROM settings WHERE key=?1",
                params![key],
                |r| r.get::<_, String>(0),
            )
            .optional()?;
        Ok(v)
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        let conn = self.lock();
        conn.execute(
            "INSERT INTO settings(key, value, updated_at) VALUES (?1, ?2, ?3)
             ON CONFLICT(key) DO UPDATE SET value=excluded.value, updated_at=excluded.updated_at",
            params![key, value, now_string()],
        )?;
        Ok(())
    }

    pub fn all_settings(&self) -> Result<Vec<Setting>> {
        let conn = self.lock();
        let mut stmt = conn.prepare("SELECT key, value FROM settings ORDER BY key")?;
        let rows = stmt.query_map([], |r| {
            Ok(Setting {
                key: r.get(0)?,
                value: r.get(1)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    // ───────────────────────── 报告 ─────────────────────────

    pub fn save_report(&self, rtype: &str, period: &str, content: &str, is_ai: bool) -> Result<Report> {
        let conn = self.lock();
        conn.execute(
            "INSERT INTO reports(type, period, content, is_ai, created_at) VALUES (?1,?2,?3,?4,?5)",
            params![rtype, period, content, if is_ai { 1 } else { 0 }, now_string()],
        )?;
        let id = conn.last_insert_rowid();
        let r = conn.query_row(
            "SELECT id, type, period, content, is_ai, created_at FROM reports WHERE id=?1",
            params![id],
            |row| {
                Ok(Report {
                    id: row.get(0)?,
                    r#type: row.get(1)?,
                    period: row.get(2)?,
                    content: row.get(3)?,
                    is_ai: row.get::<_, i64>(4)? != 0,
                    created_at: row.get(5)?,
                })
            },
        )?;
        Ok(r)
    }

    pub fn list_reports(&self, rtype: Option<&str>, limit: i64) -> Result<Vec<Report>> {
        let conn = self.lock();
        let map = |row: &rusqlite::Row| {
            Ok(Report {
                id: row.get(0)?,
                r#type: row.get(1)?,
                period: row.get(2)?,
                content: row.get(3)?,
                is_ai: row.get::<_, i64>(4)? != 0,
                created_at: row.get(5)?,
            })
        };
        let mut out = vec![];
        match rtype {
            Some(t) if !t.is_empty() => {
                let mut stmt = conn.prepare(
                    "SELECT id, type, period, content, is_ai, created_at FROM reports
                     WHERE type=?1 ORDER BY created_at DESC LIMIT ?2",
                )?;
                out = stmt
                    .query_map(params![t, limit], map)?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
            }
            _ => {
                let mut stmt = conn.prepare(
                    "SELECT id, type, period, content, is_ai, created_at FROM reports
                     ORDER BY created_at DESC LIMIT ?1",
                )?;
                out = stmt
                    .query_map(params![limit], map)?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
            }
        }
        Ok(out)
    }

    pub fn latest_report(&self, rtype: &str, period: &str) -> Result<Option<Report>> {
        let conn = self.lock();
        let r = conn
            .query_row(
                "SELECT id, type, period, content, is_ai, created_at FROM reports
                 WHERE type=?1 AND period=?2 ORDER BY created_at DESC LIMIT 1",
                params![rtype, period],
                |row| {
                    Ok(Report {
                        id: row.get(0)?,
                        r#type: row.get(1)?,
                        period: row.get(2)?,
                        content: row.get(3)?,
                        is_ai: row.get::<_, i64>(4)? != 0,
                        created_at: row.get(5)?,
                    })
                },
            )
            .optional()?;
        Ok(r)
    }

    pub fn delete_report(&self, id: i64) -> Result<bool> {
        let conn = self.lock();
        Ok(conn.execute("DELETE FROM reports WHERE id=?1", params![id])? > 0)
    }

    // ───────────────────────── 模板 ─────────────────────────

    pub fn get_template(&self, ttype: &str) -> Result<Option<String>> {
        let conn = self.lock();
        let v = conn
            .query_row(
                "SELECT content FROM templates WHERE type=?1",
                params![ttype],
                |r| r.get::<_, String>(0),
            )
            .optional()?;
        Ok(v)
    }

    pub fn set_template(&self, ttype: &str, content: &str) -> Result<()> {
        let conn = self.lock();
        conn.execute(
            "INSERT INTO templates(type, content, updated_at) VALUES (?1,?2,?3)
             ON CONFLICT(type) DO UPDATE SET content=excluded.content, updated_at=excluded.updated_at",
            params![ttype, content, now_string()],
        )?;
        Ok(())
    }

    pub fn all_templates(&self) -> Result<Vec<(String, String)>> {
        let conn = self.lock();
        let mut stmt = conn.prepare("SELECT type, content FROM templates")?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    // ───────────────────────── 聊天 ─────────────────────────

    pub fn append_chat(&self, session: &str, role: &str, content: &str) -> Result<ChatMessage> {
        let now = now_string();
        let conn = self.lock();
        conn.execute(
            "INSERT INTO chat_messages(session_id, role, content, created_at) VALUES (?1,?2,?3,?4)",
            params![session, role, content, now],
        )?;
        let id = conn.last_insert_rowid();
        Ok(ChatMessage {
            id,
            session_id: session.to_string(),
            role: role.to_string(),
            content: content.to_string(),
            created_at: now,
        })
    }

    pub fn list_chat(&self, session: &str, limit: i64) -> Result<Vec<ChatMessage>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT id, session_id, role, content, created_at FROM chat_messages
             WHERE session_id=?1 ORDER BY id DESC LIMIT ?2",
        )?;
        let mut rows = stmt
            .query_map(params![session, limit], |r| {
                Ok(ChatMessage {
                    id: r.get(0)?,
                    session_id: r.get(1)?,
                    role: r.get(2)?,
                    content: r.get(3)?,
                    created_at: r.get(4)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        rows.reverse();
        Ok(rows)
    }

    pub fn clear_chat(&self, session: &str) -> Result<()> {
        let conn = self.lock();
        conn.execute(
            "DELETE FROM chat_messages WHERE session_id=?1",
            params![session],
        )?;
        Ok(())
    }

    // ───────────────────────── 用户（访问密码） ─────────────────────────

    pub fn upsert_owner(&self, username: &str, password_hash: &str) -> Result<()> {
        let conn = self.lock();
        conn.execute(
            "INSERT INTO users(username, password_hash, role) VALUES (?1, ?2, 'owner')
             ON CONFLICT(username) DO UPDATE SET password_hash=excluded.password_hash",
            params![username, password_hash],
        )?;
        Ok(())
    }

    pub fn owner_password_hash(&self, username: &str) -> Result<Option<String>> {
        let conn = self.lock();
        let v = conn
            .query_row(
                "SELECT password_hash FROM users WHERE username=?1",
                params![username],
                |r| r.get::<_, String>(0),
            )
            .optional()?;
        Ok(v)
    }

    pub fn has_owner(&self) -> Result<bool> {
        let conn = self.lock();
        let n: i64 = conn.query_row("SELECT COUNT(*) FROM users", [], |r| r.get(0))?;
        Ok(n > 0)
    }

    // ───────────────────────── 成就 ─────────────────────────

    pub fn unlock_achievement(&self, id: &str) -> Result<bool> {
        let conn = self.lock();
        let n = conn.execute(
            "INSERT OR IGNORE INTO achievements(id, unlocked_at) VALUES (?1, ?2)",
            params![id, now_string()],
        )?;
        Ok(n > 0)
    }

    pub fn list_achievements(&self) -> Result<Vec<Achievement>> {
        let conn = self.lock();
        let mut stmt = conn.prepare("SELECT id, unlocked_at FROM achievements ORDER BY unlocked_at ASC")?;
        let rows = stmt.query_map([], |r| {
            Ok(Achievement {
                id: r.get(0)?,
                unlocked_at: r.get(1)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    // ───────────────────────── 统计 ─────────────────────────

    pub fn daily_stats(&self, date: &str) -> Result<DailyStats> {
        let daily_goal: i64 = self
            .get_setting("daily_goal")?
            .and_then(|v| v.parse().ok())
            .unwrap_or(4);
        let goal_enabled = self
            .get_setting("daily_goal_enabled")?
            .map(|v| v == "1")
            .unwrap_or(true);

        let conn = self.lock();
        let node_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM nodes WHERE date=?1 AND deleted_at IS NULL",
            params![date],
            |r| r.get(0),
        )?;
        let total_todos: i64 = conn.query_row(
            "SELECT COUNT(*) FROM todos WHERE deleted_at IS NULL",
            [],
            |r| r.get(0),
        )?;
        let done_todos: i64 = conn.query_row(
            "SELECT COUNT(*) FROM todos WHERE deleted_at IS NULL AND status='已完成'",
            [],
            |r| r.get(0),
        )?;
        let overdue_todos: i64 = conn.query_row(
            "SELECT COUNT(*) FROM todos WHERE deleted_at IS NULL AND status='已逾期'",
            [],
            |r| r.get(0),
        )?;
        let today_todos: i64 = conn.query_row(
            "SELECT COUNT(*) FROM todos WHERE deleted_at IS NULL AND due_date=?1",
            params![date],
            |r| r.get(0),
        )?;
        let today_done_todos: i64 = conn.query_row(
            "SELECT COUNT(*) FROM todos WHERE deleted_at IS NULL AND due_date=?1 AND status='已完成'",
            params![date],
            |r| r.get(0),
        )?;
        drop(conn);

        Ok(DailyStats {
            date: date.to_string(),
            node_count,
            daily_goal,
            goal_enabled,
            streak_days: self.streak_days()?,
            total_todos,
            done_todos,
            overdue_todos,
            today_todos,
            today_done_todos,
        })
    }

    /// 连续记录天数（含今天；若今天没记录则从昨天起算）
    pub fn streak_days(&self) -> Result<i64> {
        let dates = self.node_dates_desc(400)?;
        if dates.is_empty() {
            return Ok(0);
        }
        let today = Local::now().date_naive();
        let set: std::collections::HashSet<NaiveDate> = dates
            .iter()
            .filter_map(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").ok())
            .collect();

        let mut cursor = today;
        if !set.contains(&today) {
            cursor = today - Duration::days(1);
            if !set.contains(&cursor) {
                return Ok(0);
            }
        }
        let mut count = 0i64;
        while set.contains(&cursor) {
            count += 1;
            cursor -= Duration::days(1);
        }
        Ok(count)
    }

    /// 周期统计（周/月视图一次拉取）
    pub fn period_stats(&self, from: &str, to: &str) -> Result<PeriodStats> {
        let from_d = NaiveDate::parse_from_str(from, "%Y-%m-%d")?;
        let to_d = NaiveDate::parse_from_str(to, "%Y-%m-%d")?;
        let total_days = (to_d - from_d).num_days() + 1;

        let nodes = self.list_nodes_range(from, to)?;
        let todos = self.list_todos(Some("全部"), Some("全部"), None, None, None)?;

        let mut days: Vec<DayStat> = Vec::new();
        let mut cursor = from_d;
        while cursor <= to_d {
            let key = cursor.format("%Y-%m-%d").to_string();
            let day_nodes: Vec<&Node> = nodes.iter().filter(|n| n.date == key).collect();
            let day_todos: Vec<&Todo> = todos.iter().filter(|t| t.due_date == key).collect();
            days.push(DayStat {
                date: key.clone(),
                node_count: day_nodes.len() as i64,
                // 摘要不再按 28 字预截断（用户反馈：文字没有随格子空间增减）——
                // 把足够完整的文本交给前端，由格子剩余空间决定显示多少（放不下自动隐藏）。
                // 400 字上限只为防超长单条记录撑大载荷，正常记录等于全文。
                node_summaries: day_nodes
                    .iter()
                    .take(6)
                    .map(|n| summarize(&n.content, 400))
                    .collect(),
                total_todos: day_todos.len() as i64,
                done_todos: day_todos.iter().filter(|t| t.status == "已完成").count() as i64,
            });
            cursor += Duration::days(1);
        }

        let period_todos: Vec<&Todo> = todos
            .iter()
            .filter(|t| t.due_date >= from.to_string() && t.due_date <= to.to_string())
            .collect();

        Ok(PeriodStats {
            from: from.to_string(),
            to: to.to_string(),
            node_count: nodes.len() as i64,
            days_with_records: days.iter().filter(|d| d.node_count > 0).count() as i64,
            total_days,
            total_todos: period_todos.len() as i64,
            done_todos: period_todos.iter().filter(|t| t.status == "已完成").count() as i64,
            days,
        })
    }

    /// 月度小结（FR-6.4）
    pub fn monthly_summary(&self, date: &str) -> Result<MonthlySummary> {
        let d = NaiveDate::parse_from_str(date, "%Y-%m-%d")?;
        let month = format!("{:04}-{:02}", d.year(), d.month());
        let first = NaiveDate::from_ymd_opt(d.year(), d.month(), 1).unwrap();
        let next_month = if d.month() == 12 {
            NaiveDate::from_ymd_opt(d.year() + 1, 1, 1).unwrap()
        } else {
            NaiveDate::from_ymd_opt(d.year(), d.month() + 1, 1).unwrap()
        };
        let last = next_month - Duration::days(1);
        let total_days = (last - first).num_days() + 1;

        let nodes = self.list_nodes_range(
            &first.format("%Y-%m-%d").to_string(),
            &last.format("%Y-%m-%d").to_string(),
        )?;
        let todos = self.list_todos(Some("全部"), Some("全部"), None, None, None)?;

        // 有记录的天集合
        let days: std::collections::HashSet<String> =
            nodes.iter().map(|n| n.date.clone()).collect();

        // 本月内最长连续记录天数
        let mut longest = 0i64;
        let mut run = 0i64;
        let mut cursor = first;
        while cursor <= last {
            if days.contains(&cursor.format("%Y-%m-%d").to_string()) {
                run += 1;
                longest = longest.max(run);
            } else {
                run = 0;
            }
            cursor += Duration::days(1);
        }

        let month_todos: Vec<&Todo> = todos
            .iter()
            .filter(|t| t.due_date.starts_with(&month))
            .collect();
        let done_todos = month_todos.iter().filter(|t| t.status == "已完成").count() as i64;
        let days_with_records = days.len() as i64;

        Ok(MonthlySummary {
            month,
            node_count: nodes.len() as i64,
            days_with_records,
            total_days,
            done_todos,
            total_todos: month_todos.len() as i64,
            longest_streak: longest,
            avg_per_active_day: if days_with_records > 0 {
                ((nodes.len() as f64 / days_with_records as f64) * 10.0).round() / 10.0
            } else {
                0.0
            },
            is_month_end: d == last,
        })
    }

    /// 备份导出：全量 JSON
    pub fn export_all(&self) -> Result<serde_json::Value> {
        let conn = self.lock();
        let nodes: Vec<serde_json::Value> = {
            let mut stmt = conn.prepare(&format!(
                "SELECT {NODE_COLS} FROM nodes WHERE deleted_at IS NULL ORDER BY id"
            ))?;
            let rows = stmt.query_map([], row_to_node)?;
            rows.collect::<rusqlite::Result<Vec<_>>>()?
                .into_iter()
                .map(|n| serde_json::to_value(n).unwrap_or(serde_json::Value::Null))
                .collect()
        };
        let todos: Vec<serde_json::Value> = {
            let mut stmt = conn.prepare(&format!(
                "SELECT {} FROM todos WHERE deleted_at IS NULL ORDER BY id",
                Self::TODO_COLS
            ))?;
            let rows = stmt.query_map([], Self::row_to_todo)?;
            rows.collect::<rusqlite::Result<Vec<_>>>()?
                .into_iter()
                .map(|t| serde_json::to_value(t).unwrap_or(serde_json::Value::Null))
                .collect()
        };
        // 注意：此处不能持有 conn 调用其它 &self 方法（Mutex 不可重入）
        drop(conn);
        let settings = self.all_settings()?;
        let reports = self.list_reports(None, 1000)?;
        Ok(serde_json::json!({
            "version": 1,
            "exportedAt": now_string(),
            "nodes": nodes,
            "todos": todos,
            "settings": settings,
            "reports": reports,
        }))
    }

    /// 数据清空（导入前）。
    ///
    /// **清空前一定先做一次整库备份**：这个方法会删掉用户的全部记录与待办，
    /// 一旦被误调用（真实事故：测试套件把用户的待办清掉了）就没有任何挽回余地。
    /// 备份与迁移前备份用同一套机制（`VACUUM INTO`），可用任意 SQLite 工具直接打开。
    pub fn wipe(&self) -> Result<()> {
        if let Some(backup) = self.backup_before_wipe()? {
            tracing::warn!("清空数据前已自动备份：{}", backup.display());
        }
        let conn = self.lock();
        conn.execute_batch(
            "DELETE FROM nodes; DELETE FROM todos; DELETE FROM reports; DELETE FROM chat_messages;
             DELETE FROM achievements;",
        )?;
        Ok(())
    }

    /// 清空数据前的整库备份（`data.db.bak.wipe-<时间戳>`，只保留最近 3 份）
    pub fn backup_before_wipe(&self) -> Result<Option<std::path::PathBuf>> {
        let Some(path) = self.path.as_ref() else {
            return Ok(None); // 内存库（测试）无需备份
        };
        if !path.exists() {
            return Ok(None);
        }
        let stamp = Local::now().format("%Y%m%d-%H%M%S");
        let bak = path.with_extension(format!("db.bak.wipe-{stamp}"));
        let conn = self.lock();
        let escaped = bak.to_string_lossy().replace('\'', "''");
        conn.execute_batch(&format!("VACUUM INTO '{escaped}'"))?;
        crate::db::prune_backups_with_prefix(path, &format!("{}.bak.wipe-", crate::db::file_name_of(path)), 3);
        Ok(Some(bak))
    }
}

// ───────────────────────── 辅助函数 ─────────────────────────

/// 待办归类：截止=今天→今日；本周内→本周；本月内→本月；其他/含具体时间→日程
pub fn classify(due_date: &str, due_time: Option<&str>, today: &str) -> String {
    if due_time.is_some() {
        return "日程".into();
    }
    let Ok(today_d) = NaiveDate::parse_from_str(today, "%Y-%m-%d") else {
        return "日程".into();
    };
    let Ok(due_d) = NaiveDate::parse_from_str(due_date, "%Y-%m-%d") else {
        return "日程".into();
    };
    if due_d < today_d {
        return "今日".into();
    }
    if due_d == today_d {
        return "今日".into();
    }
    // 本周（周一为起点）
    let weekday = today_d.weekday().num_days_from_monday() as i64;
    let week_start = today_d - Duration::days(weekday);
    let week_end = week_start + Duration::days(6);
    if due_d >= week_start && due_d <= week_end {
        return "本周".into();
    }
    // 本月
    if due_d.year() == today_d.year() && due_d.month() == today_d.month() {
        return "本月".into();
    }
    "日程".into()
}

pub fn is_overdue(due_date: &str, due_time: Option<&str>, status: &str) -> bool {
    if status == "已完成" {
        return false;
    }
    let now = Local::now();
    let today = now.format("%Y-%m-%d").to_string();
    if due_date < today.as_str() {
        return true;
    }
    if due_date == today {
        if let Some(t) = due_time {
            let now_hm = now.format("%H:%M").to_string();
            return t < now_hm.as_str();
        }
    }
    false
}

/// 提醒时刻 = 截止日期时间 - offset 分钟
pub fn compute_remind_at(due_date: &str, due_time: &str, offset_min: i64) -> Option<String> {
    let dt = chrono::NaiveDateTime::parse_from_str(
        &format!("{due_date} {due_time}"),
        "%Y-%m-%d %H:%M",
    )
    .ok()?;
    let r = dt - Duration::minutes(offset_min);
    Some(r.format("%Y-%m-%d %H:%M").to_string())
}

// ───────────────────────── 循环待办：日期推算 ─────────────────────────

/// 收敛循环类型：只认 daily / weekly / monthly，其余一律视为不循环
pub fn normalize_recur_type(v: &str) -> String {
    match v {
        "daily" => "daily".into(),
        "weekly" => "weekly".into(),
        "monthly" => "monthly".into(),
        _ => String::new(),
    }
}

fn parse_day(s: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").ok()
}

fn days_in_month(y: i32, m: u32) -> u32 {
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 {
                29
            } else {
                28
            }
        }
        _ => 30,
    }
}

/// 法定休假/补班表（与前端 lunar.ts 的 HOLIDAY_DATA 同源维护；国务院公报公布后更新）。
/// 只用于循环待办「跳过休息日」的顺延判断。
const HOLIDAY_OFF: &[(i32, &str, u32)] = &[
    (2025, "01-01", 1),
    (2025, "01-28", 8),
    (2025, "04-04", 3),
    (2025, "05-01", 5),
    (2025, "05-31", 3),
    (2025, "10-01", 8),
    (2026, "01-01", 3),
    (2026, "02-15", 8),
    (2026, "04-04", 3),
    (2026, "05-01", 5),
    (2026, "06-19", 3),
    (2026, "09-25", 3),
    (2026, "10-01", 7),
];

const HOLIDAY_WORK: &[(i32, &str)] = &[
    (2025, "01-26"),
    (2025, "02-08"),
    (2025, "04-27"),
    (2025, "09-28"),
    (2025, "10-11"),
    (2026, "02-14"),
    (2026, "02-28"),
    (2026, "04-26"),
    (2026, "09-20"),
    (2026, "10-10"),
];

/// 休息日 = 周末或法定休假（补班日算工作日）；表外年份只看周末
fn is_rest_day(d: NaiveDate) -> bool {
    use chrono::Datelike;
    let md = d.format("%m-%d").to_string();
    let y = d.year();
    if HOLIDAY_WORK
        .iter()
        .any(|(yy, md2)| *yy == y && *md2 == md)
    {
        return false; // 补班日=工作日
    }
    if matches!(d.weekday(), chrono::Weekday::Sat | chrono::Weekday::Sun) {
        return true;
    }
    HOLIDAY_OFF.iter().any(|(yy, start, n)| {
        if *yy != y {
            return false;
        }
        parse_day(&format!("{y}-{start}"))
            .map(|s| {
                let end = s + Duration::days((*n - 1) as i64);
                d >= s && d <= end
            })
            .unwrap_or(false)
    })
}

/// 空串/非法日期统一为空（循环截止等可选日期字段）
fn normalize_date_opt(s: &str) -> String {
    let s = s.trim();
    if s.is_empty() {
        return String::new();
    }
    parse_day(s)
        .map(|d| d.format("%Y-%m-%d").to_string())
        .unwrap_or_default()
}

/// 循环待办的下一期日期：从 `from_date` 起按周期推进，直到不早于 `not_before`。
/// - daily：每 interval 天；weekly：每 interval 周（保持星期几）；monthly：每 interval 月（锚点几号，月末截断）
/// - skip_rest：落点若为休息日（周末/法定休假），顺延到下一个工作日
/// - until 非空且下一期晚于它：返回 None（整条链到点停止）
pub fn next_recur_date(
    recur_type: &str,
    anchor: &str,
    from_date: &str,
    not_before: &str,
    interval: i64,
    until: &str,
    skip_rest: bool,
) -> Option<String> {
    let step = if interval < 1 { 1 } else { interval };
    let mut cur = parse_day(from_date)?;
    let not_before_d = parse_day(not_before).unwrap_or(cur);
    let until_d = parse_day(until);
    let candidate = |mut cur: NaiveDate| -> Option<NaiveDate> {
        loop {
            match recur_type {
                "daily" => cur += Duration::days(step),
                "weekly" => cur += Duration::days(7 * step),
                "monthly" => {
                    let (y, m) = (cur.year(), cur.month());
                    let (y2, m2) = if m == 12 { (y + 1, 1) } else { (y, m + 1) };
                    // 间隔 N 月：直接推进 N 个月（月末截断）
                    let total = (y2 * 12 + m2 as i32 - 1) + (step as i32 - 1);
                    let y3 = total / 12;
                    let m3 = (total % 12) as u32 + 1;
                    let day = parse_day(anchor)
                        .map(|a| a.day())
                        .or_else(|| parse_day(from_date).map(|d| d.day()))
                        .unwrap_or(1);
                    cur = NaiveDate::from_ymd_opt(y3, m3, day.min(days_in_month(y3, m3)))?;
                }
                _ => return None,
            }
            if cur >= not_before_d {
                return Some(cur);
            }
        }
    };
    let mut next = candidate(cur)?;
    if skip_rest {
        while is_rest_day(next) {
            next += Duration::days(1);
        }
    }
    if let Some(u) = until_d {
        if next > u {
            return None; // 超过循环截止：整条链停止生成
        }
    }
    Some(next.format("%Y-%m-%d").to_string())
}

/// 摘要截断
pub fn summarize(s: &str, max_chars: usize) -> String {
    let cleaned: String = s.chars().filter(|c| *c != '\n' && *c != '\r').collect();
    if cleaned.chars().count() <= max_chars {
        cleaned
    } else {
        format!("{}…", cleaned.chars().take(max_chars).collect::<String>())
    }
}
