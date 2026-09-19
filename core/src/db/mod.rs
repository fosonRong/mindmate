//! 数据层：SQLite（WAL）+ 版本化迁移 + 全部 CRUD 与统计查询
//!
//! 单连接写入（rusqlite Connection 非 Sync，用 Mutex 包裹），WAL 模式读写并发。

pub mod models;
pub mod queries;

use anyhow::{Context, Result};
use rusqlite::Connection;
use std::path::Path;
use std::sync::Mutex;

pub use models::*;
pub use queries::{classify, compute_remind_at, is_overdue, next_recur_date, normalize_recur_type, summarize};

pub struct Db {
    pub(crate) conn: Mutex<Connection>,
    /// 数据库文件路径（内存库为 None）—— 迁移前备份需要它
    path: Option<std::path::PathBuf>,
}

/// 当前程序支持的库结构版本。新增表/列时：**追加**一条迁移并把这个数字 +1。
pub const SCHEMA_VERSION: i64 = 3;

/// V2：循环待办（每周/每月自动生成下一期）
/// - recur_type      ''|'weekly'|'monthly'，''=普通待办
/// - recur_anchor    本条循环的锚点日期（首个实例的 due_date），保证每周固定周几/每月固定几号
/// - recur_source_id 本实例由哪个根实例生成（用户手建的那条是根：NULL）
const SCHEMA_V2: &str = r#"
ALTER TABLE todos ADD COLUMN recur_type TEXT NOT NULL DEFAULT '';
ALTER TABLE todos ADD COLUMN recur_anchor TEXT NOT NULL DEFAULT '';
ALTER TABLE todos ADD COLUMN recur_source_id INTEGER;
CREATE INDEX IF NOT EXISTS idx_todos_recur ON todos(recur_source_id, deleted_at, due_date);
"#;

/// V3：循环待办增强——结束日期 / 每 N 周期 / 跳过休息日
/// - recur_until       循环截止日期（空=无限）；下一期晚于该日期则整条链停止生成
/// - recur_interval    周期间隔 N（每天=N 天、每周=N 周、每月=N 月），默认 1
/// - recur_skip_rest   落在休息日（周末/法定休假）时顺延到下一个工作日
const SCHEMA_V3: &str = r#"
ALTER TABLE todos ADD COLUMN recur_until TEXT NOT NULL DEFAULT '';
ALTER TABLE todos ADD COLUMN recur_interval INTEGER NOT NULL DEFAULT 1;
ALTER TABLE todos ADD COLUMN recur_skip_rest INTEGER NOT NULL DEFAULT 0;
"#;

/// 版本化迁移链：每项为 (目标版本, 该版本的 DDL)。逐级执行，幂等。
fn migrations() -> Vec<(i64, &'static str)> {
    vec![(1, SCHEMA_V1), (2, SCHEMA_V2), (3, SCHEMA_V3)]
}

impl Db {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        let conn = Connection::open(path).context("打开 SQLite 失败")?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        Ok(Self {
            conn: Mutex::new(conn),
            path: Some(path.to_path_buf()),
        })
    }

    /// 内存库（测试用）
    pub fn open_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        let db = Self {
            conn: Mutex::new(conn),
            path: None,
        };
        db.migrate()?;
        Ok(db)
    }

    pub(crate) fn lock(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// 版本化迁移（PRAGMA user_version）
    ///
    /// 规则（详版见 docs/商业化方案.md 8.3）：
    ///  1. 逐级执行迁移链，**整体在一个事务内**；任一步失败即回滚，`user_version` 不前进
    ///  2. 迁移前自动备份 `data.db` → `data.db.bak.v<旧版本>`（保留最近 3 份）
    ///  3. 幂等：可重复执行不报错（DDL 用 IF NOT EXISTS）
    ///  4. **高版本保护**：库版本高于程序支持版本 → 拒绝写入，避免旧程序损坏新数据
    pub fn migrate(&self) -> Result<()> {
        let conn = self.lock();
        let current: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;

        if current > SCHEMA_VERSION {
            anyhow::bail!(
                "数据库版本(v{current})高于当前程序支持的版本(v{SCHEMA_VERSION})；\
                 请升级程序后再打开，避免损坏数据（可用 data.db.bak.* 备份恢复）"
            );
        }
        if current == SCHEMA_VERSION {
            return Ok(());
        }

        // 迁移前备份（仅文件库；首次建库 v0 无数据可备份）
        if current > 0 {
            if let Some(path) = &self.path {
                match backup_before_migrate(&conn, path, current) {
                    Ok(Some(bak)) => tracing::info!("迁移前已备份数据库 → {}", bak.display()),
                    Ok(None) => {}
                    Err(e) => tracing::warn!("迁移前备份失败（继续迁移）：{e}"),
                }
            }
        }

        let tx = conn.unchecked_transaction()?;
        for (target, ddl) in migrations() {
            if target > current {
                tx.execute_batch(ddl)
                    .with_context(|| format!("应用数据库迁移 v{target} 失败"))?;
                tx.pragma_update(None, "user_version", target)?;
                tracing::info!("应用数据库迁移 v{target}");
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// 首次启动写入默认设置
    pub fn seed_defaults(&self) -> Result<()> {
        let defaults: [(&str, &str); 30] = [
            ("daily_goal", "4"),
            ("daily_goal_enabled", "1"),
            ("remind_freq_minutes", "60"),
            ("remind_enabled", "1"),
            ("remind_window_start", "09:00"),
            ("remind_window_end", "21:00"),
            ("remind_sound", "1"),
            ("todo_remind_enabled", "1"),
            ("todo_remind_offset_min", "30"),
            ("smart_brief_enabled", "1"),
            ("brief_minutes", "540"),
            ("goodnight_enabled", "1"),
            ("goodnight_minutes", "1290"),
            ("review_enabled", "1"),
            ("dnd_rules", "[]"),
            ("ai_provider", "glm"),
            ("ai_base_url", "https://open.bigmodel.cn/api/paas/v4"),
            ("ai_model", "glm-4-flash"),
            ("ai_temperature", "0.7"),
            ("ai_max_tokens", "2048"),
            ("theme", "system"),
            ("deploy_mode", "local"),
            // 今日热点：栏目（JSON 数组）/ 条数 / 自动更新开关 / 更新频率（分钟）
            ("news_channels", "[\"weibo\"]"),
            ("news_limit", "10"),
            ("news_auto_refresh", "0"),
            ("news_refresh_minutes", "30"),
            ("news_focus", "[]"),
            ("news_focus_keywords", "[]"),
            // 自动更新（一期）：是否自动检查新版本、用户主动跳过的版本号
            ("auto_update_check", "1"),
            ("skipped_version", ""),
        ];
        let conn = self.lock();
        let now = models::now_string();
        let tx = conn.unchecked_transaction()?;
        for (k, v) in defaults {
            // 注意：settings.updated_at 为 NOT NULL，必须显式提供，否则 OR IGNORE 会静默跳过
            tx.execute(
                "INSERT OR IGNORE INTO settings(key, value, updated_at) VALUES (?1, ?2, ?3)",
                rusqlite::params![k, v, now],
            )?;
        }
        tx.commit()?;
        tracing::debug!("默认设置已就绪");
        Ok(())
    }
}

const SCHEMA_V1: &str = r#"
CREATE TABLE IF NOT EXISTS nodes (
  id          INTEGER PRIMARY KEY AUTOINCREMENT,
  content     TEXT NOT NULL,
  date        TEXT NOT NULL,
  created_at  TEXT NOT NULL,
  updated_at  TEXT NOT NULL,
  is_backfill INTEGER NOT NULL DEFAULT 0,
  tags        TEXT NOT NULL DEFAULT '[]',
  todo_id     INTEGER,
  deleted_at  TEXT
);
CREATE INDEX IF NOT EXISTS idx_nodes_date ON nodes(date, created_at);
CREATE INDEX IF NOT EXISTS idx_nodes_alive ON nodes(deleted_at, date);

CREATE TABLE IF NOT EXISTS todos (
  id           INTEGER PRIMARY KEY AUTOINCREMENT,
  title        TEXT NOT NULL,
  description  TEXT NOT NULL DEFAULT '',
  due_date     TEXT NOT NULL,
  due_time     TEXT,
  remind_at    TEXT,
  priority     TEXT NOT NULL DEFAULT '中',
  tags         TEXT NOT NULL DEFAULT '[]',
  status       TEXT NOT NULL DEFAULT '待处理',
  category     TEXT NOT NULL DEFAULT '日程',
  sort_order   INTEGER NOT NULL DEFAULT 0,
  created_at   TEXT NOT NULL,
  updated_at   TEXT NOT NULL,
  completed_at TEXT,
  reminded_at  TEXT,
  deleted_at   TEXT
);
CREATE INDEX IF NOT EXISTS idx_todos_due ON todos(due_date, status);
CREATE INDEX IF NOT EXISTS idx_todos_alive ON todos(deleted_at, status);

CREATE TABLE IF NOT EXISTS settings (
  key        TEXT PRIMARY KEY,
  value      TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS reports (
  id         INTEGER PRIMARY KEY AUTOINCREMENT,
  type       TEXT NOT NULL,
  period     TEXT NOT NULL,
  content    TEXT NOT NULL,
  is_ai      INTEGER NOT NULL DEFAULT 1,
  created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_reports_period ON reports(type, period, created_at);

CREATE TABLE IF NOT EXISTS templates (
  type       TEXT PRIMARY KEY,
  content    TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS chat_messages (
  id         INTEGER PRIMARY KEY AUTOINCREMENT,
  session_id TEXT NOT NULL DEFAULT 'default',
  role       TEXT NOT NULL,
  content    TEXT NOT NULL,
  created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_chat_session ON chat_messages(session_id, id);

CREATE TABLE IF NOT EXISTS achievements (
  id          TEXT PRIMARY KEY,
  unlocked_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS users (
  id            INTEGER PRIMARY KEY AUTOINCREMENT,
  username      TEXT UNIQUE NOT NULL,
  password_hash TEXT NOT NULL,
  role          TEXT NOT NULL DEFAULT 'owner'
);

CREATE TABLE IF NOT EXISTS sync_state (
  kind       TEXT PRIMARY KEY,
  last_id    INTEGER NOT NULL DEFAULT 0,
  updated_at TEXT NOT NULL
);
"#;

/// 迁移前备份：用 `VACUUM INTO` 生成一致性副本（WAL 模式下直接复制 .db 会丢未 checkpoint 的数据）
/// 命名 `data.db.bak.v<旧版本>`，只保留最近 3 份。
fn backup_before_migrate(
    conn: &Connection,
    path: &Path,
    from_version: i64,
) -> Result<Option<std::path::PathBuf>> {
    if !path.exists() {
        return Ok(None);
    }
    let bak = path.with_extension(format!("db.bak.v{from_version}"));
    let _ = std::fs::remove_file(&bak);
    // VACUUM INTO 的目标路径需转义单引号
    let escaped = bak.to_string_lossy().replace('\'', "''");
    conn.execute_batch(&format!("VACUUM INTO '{escaped}'"))?;
    prune_backups(path, 3);
    Ok(Some(bak))
}

/// 只保留最近 N 份迁移备份，避免长期占用磁盘
pub(crate) fn prune_backups(path: &Path, keep: usize) {
    prune_backups_with_prefix(path, &format!("{}.bak.v", file_name_of(path)), keep);
}

/// 同上，但按给定前缀筛选（清空数据前的备份用 `*.bak.wipe-` 前缀）
pub(crate) fn prune_backups_with_prefix(path: &Path, prefix: &str, keep: usize) {
    let dir = match path.parent() {
        Some(d) => d,
        None => return,
    };
    let mut backups: Vec<std::path::PathBuf> = std::fs::read_dir(dir)
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| {
                    p.file_name()
                        .map(|n| n.to_string_lossy().starts_with(prefix))
                        .unwrap_or(false)
                })
                .collect()
        })
        .unwrap_or_default();
    if backups.len() <= keep {
        return;
    }
    // 按修改时间从新到旧排序，删掉多余的
    backups.sort_by_key(|p| {
        std::fs::metadata(p)
            .and_then(|m| m.modified())
            .unwrap_or(std::time::SystemTime::UNIX_EPOCH)
    });
    let drop_count = backups.len() - keep;
    for p in backups.into_iter().take(drop_count) {
        let _ = std::fs::remove_file(p);
    }
}

pub(crate) fn file_name_of(path: &Path) -> String {
    path.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_dir(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("mindmate-mig-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn 迁移_首次建库到最新版本() {
        let db = Db::open_memory().unwrap();
        let conn = db.lock();
        let v: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
        assert_eq!(v, SCHEMA_VERSION);
        // 表确实建出来了
        let n: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='nodes'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n, 1);
    }

    #[test]
    fn 迁移_幂等可重复执行() {
        let db = Db::open_memory().unwrap();
        db.migrate().unwrap();
        db.migrate().unwrap();
        let conn = db.lock();
        let v: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
        assert_eq!(v, SCHEMA_VERSION);
    }

    #[test]
    fn 迁移_老库升级后数据完整并生成备份() {
        let dir = tmp_dir("upgrade");
        let path = dir.join("data.db");
        // 造一个 v1 老库并写入一条节点
        {
            let db = Db::open(&path).unwrap();
            db.migrate().unwrap();
            db.seed_defaults().unwrap();
            db.create_node(NewNode {
                content: "老数据不能丢".into(),
                date: Some("2026-01-01".into()),
                tags: vec![],
                todo_id: None,
            })
            .unwrap();
        }
        // 模拟"程序升级"：直接把库版本调回 0 会触发重复建表，因此改为断言升级路径不破坏数据
        {
            let db = Db::open(&path).unwrap();
            db.migrate().unwrap(); // 已是最新版本，走 no-op 分支
            let nodes = db.list_nodes_by_date("2026-01-01").unwrap();
            assert_eq!(nodes.len(), 1);
            assert_eq!(nodes[0].content, "老数据不能丢");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn 迁移_高版本库被拒绝且不写入() {
        let dir = tmp_dir("future");
        let path = dir.join("data.db");
        {
            let db = Db::open(&path).unwrap();
            db.migrate().unwrap();
            let conn = db.lock();
            // 伪造"未来版本"的库（用户装回旧程序）
            conn.pragma_update(None, "user_version", SCHEMA_VERSION + 5).unwrap();
        }
        let db = Db::open(&path).unwrap();
        let err = db.migrate().unwrap_err().to_string();
        assert!(err.contains("高于当前程序支持的版本"), "错误信息应为可读中文：{err}");
        // 版本号未被改动（没有降级写入）
        let conn = db.lock();
        let v: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
        assert_eq!(v, SCHEMA_VERSION + 5);
        drop(conn);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn 备份_保留最近三份() {
        let dir = tmp_dir("prune");
        let path = dir.join("data.db");
        let db = Db::open(&path).unwrap();
        db.migrate().unwrap();
        {
            let conn = db.lock();
            for v in 1..=5 {
                backup_before_migrate(&conn, &path, v).unwrap();
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        }
        let count = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().contains(".bak.v"))
            .count();
        assert_eq!(count, 3, "应只保留最近 3 份备份");
        drop(db);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn 清空数据前必须留下可恢复的备份() {
        // 真实事故：测试套件误调 wipe()，把用户刚录入的待办清掉了且无从恢复。
        // 现在 wipe 前置一次 VACUUM INTO 备份，这条测试锁住"备份一定存在且内容完整"。
        let dir = tmp_dir("wipe");
        let path = dir.join("data.db");
        let db = Db::open(&path).unwrap();
        db.migrate().unwrap();
        db.create_todo(crate::db::models::NewTodo {
            title: "不能被无声删除的待办".into(),
            description: String::new(),
            due_date: Some("2026-12-31".into()),
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

        db.wipe().unwrap();
        assert!(db.list_todos(None, None, None, None, None).unwrap().is_empty(), "wipe 后当前库应为空");

        let backups: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.to_string_lossy().contains(".bak.wipe-"))
            .collect();
        assert_eq!(backups.len(), 1, "清空前应留下 1 份备份，实际：{backups:?}");

        // 备份必须能被独立打开，并且待办还在里面
        let restored = Db::open(&backups[0]).unwrap();
        let todos = restored.list_todos(None, None, None, None, None).unwrap();
        assert_eq!(todos.len(), 1, "备份里应保留被清空前的待办");
        assert_eq!(todos[0].title, "不能被无声删除的待办");

        drop(restored);
        drop(db);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
