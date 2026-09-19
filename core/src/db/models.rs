//! 数据模型：与前端 TypeScript 类型一一对应（serde camelCase）

use serde::{Deserialize, Serialize};

fn now_local() -> String {
    chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

fn today_local() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

/// 记录节点：一次录入 = 一行
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Node {
    pub id: i64,
    pub content: String,
    pub date: String,
    pub created_at: String,
    pub updated_at: String,
    pub is_backfill: bool,
    pub tags: Vec<String>,
    pub todo_id: Option<i64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewNode {
    pub content: String,
    /// 缺省 = 今天
    pub date: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    pub todo_id: Option<i64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NodePatch {
    pub content: Option<String>,
    pub tags: Option<Vec<String>>,
    pub todo_id: Option<Option<i64>>,
}

/// 待办
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Todo {
    pub id: i64,
    pub title: String,
    pub description: String,
    pub due_date: String,
    pub due_time: Option<String>,
    pub remind_at: Option<String>,
    pub priority: String,
    pub tags: Vec<String>,
    pub status: String,
    pub category: String,
    pub sort_order: i64,
    pub created_at: String,
    pub updated_at: String,
    pub completed_at: Option<String>,
    /// 循环类型：''=普通 weekly=每周 monthly=每月
    pub recur_type: String,
    /// 循环锚点日期（首个实例的 due_date），用于固定周几/几号推算下一期
    pub recur_anchor: String,
    /// 循环截止日期（空=无限循环）
    pub recur_until: String,
    /// 周期间隔 N（每天=N 天、每周=N 周、每月=N 月）
    pub recur_interval: i64,
    /// 落在休息日（周末/法定休假）时顺延到下一个工作日
    pub recur_skip_rest: bool,
    /// 本实例由哪个根实例生成（用户手建的那条是根：NULL）
    pub recur_source_id: Option<i64>,
    /// 是否已逾期（服务端计算，仅展示用）
    pub overdue: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewTodo {
    pub title: String,
    #[serde(default)]
    pub description: String,
    pub due_date: Option<String>,
    pub due_time: Option<String>,
    #[serde(default = "default_priority")]
    pub priority: String,
    #[serde(default)]
    pub tags: Vec<String>,
    /// 提前 N 分钟提醒（缺省用全局设置）
    pub remind_offset_min: Option<i64>,
    /// 精确提醒时刻
    pub remind_at: Option<String>,
    /// 循环类型：''=不循环 daily=每天 weekly=每周 monthly=每月
    #[serde(default)]
    pub recur_type: String,
    /// 循环截止日期（空=无限）
    #[serde(default)]
    pub recur_until: String,
    /// 周期间隔 N（默认 1）
    #[serde(default = "default_one")]
    pub recur_interval: i64,
    /// 落在休息日顺延到下一个工作日
    #[serde(default)]
    pub recur_skip_rest: bool,
}

fn default_one() -> i64 {
    1
}

fn default_priority() -> String {
    "中".into()
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TodoPatch {
    pub title: Option<String>,
    pub description: Option<String>,
    pub due_date: Option<String>,
    pub due_time: Option<Option<String>>,
    pub remind_at: Option<Option<String>>,
    pub priority: Option<String>,
    pub tags: Option<Vec<String>>,
    pub status: Option<String>,
    pub category: Option<String>,
    pub sort_order: Option<i64>,
    /// 修改循环类型（weekly/monthly/空串=停止循环）。对根实例生效；实例自身的该字段随生成复制。
    pub recur_type: Option<String>,
    pub recur_until: Option<String>,
    pub recur_interval: Option<i64>,
    pub recur_skip_rest: Option<bool>,
}

/// 报告
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub id: i64,
    pub r#type: String,
    pub period: String,
    pub content: String,
    pub is_ai: bool,
    pub created_at: String,
}

/// 聊天消息
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatMessage {
    pub id: i64,
    pub session_id: String,
    pub role: String,
    pub content: String,
    pub created_at: String,
}

/// 每日统计
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DailyStats {
    pub date: String,
    pub node_count: i64,
    pub daily_goal: i64,
    pub goal_enabled: bool,
    pub streak_days: i64,
    pub total_todos: i64,
    pub done_todos: i64,
    pub overdue_todos: i64,
    pub today_todos: i64,
    pub today_done_todos: i64,
}

/// 周期统计（周/月视图）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PeriodStats {
    pub from: String,
    pub to: String,
    pub days: Vec<DayStat>,
    pub node_count: i64,
    pub days_with_records: i64,
    pub total_days: i64,
    pub total_todos: i64,
    pub done_todos: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DayStat {
    pub date: String,
    pub node_count: i64,
    pub node_summaries: Vec<String>,
    pub total_todos: i64,
    pub done_todos: i64,
}

/// 月度小结（FR-6.4）：月底展示「本月记录 X 天 / 完成待办 Y 件 / 连续最长 Z 天」
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MonthlySummary {
    /// 月份 'YYYY-MM'
    pub month: String,
    /// 本月记录条数
    pub node_count: i64,
    /// 本月有记录的天数
    pub days_with_records: i64,
    /// 本月总天数
    pub total_days: i64,
    /// 本月完成的待办数
    pub done_todos: i64,
    /// 本月到期待办总数
    pub total_todos: i64,
    /// 本月内最长连续记录天数
    pub longest_streak: i64,
    /// 日均记录条数（保留一位小数）
    pub avg_per_active_day: f64,
    /// 是否为当月最后一天（前端据此突出展示小结卡）
    pub is_month_end: bool,
}

/// 设置项
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Setting {
    pub key: String,
    pub value: String,
}

/// 成就
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Achievement {
    pub id: String,
    pub unlocked_at: String,
}

pub fn now_string() -> String {
    now_local()
}
pub fn today_string() -> String {
    today_local()
}

/// 标签统计（标签选择器数据源：记录 + 待办合并去重后的使用次数）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TagStat {
    pub name: String,
    pub count: i64,
}
