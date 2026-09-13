//! 智伴 Mindmate 核心库
//!
//! 单一二进制同时承担：数据层（SQLite）、HTTP 服务（REST + SSE）、
//! AI 接入层（OpenAI 兼容）、提醒调度器。桌面壳（Tauri）与本库同进程运行，
//! 浏览器端通过同一 HTTP 端口访问 —— 双端共享同一份数据与逻辑。

pub mod achievements;
pub mod ai;
pub mod api;
pub mod config;
pub mod db;
pub mod events;
pub mod export;
pub mod i18n;
pub mod firstseen;
pub mod push;
pub mod reminder;
pub mod secrets;
pub mod system;

pub use config::{AppConfig, RunMode};
pub use db::Db;
pub use events::{Event, EventBus};

use std::path::PathBuf;
use std::sync::Arc;

/// 应用全局上下文：桌面壳与服务器模式共用
pub struct AppContext {
    pub db: Arc<Db>,
    pub bus: EventBus,
    pub cfg: AppConfig,
    /// 本地模式随机 token（桌面端注入 / 浏览器本机免登录）
    pub local_token: String,
    /// JWT 签名密钥
    pub jwt_secret: String,
}

impl AppContext {
    pub fn new(cfg: AppConfig) -> anyhow::Result<Arc<Self>> {
        let db = Arc::new(Db::open(&cfg.db_path())?);
        let local_token = secrets::load_or_create_token(&cfg.data_dir)?;
        let jwt_secret = secrets::load_or_create_jwt_secret(&cfg.data_dir)?;
        db.migrate()?;
        db.seed_defaults()?;
        // 首见证据埋点（商业化二期老用户识别的唯一来源，须在第一期就写下）
        match firstseen::ensure(&db, &cfg.data_dir, &jwt_secret, env!("CARGO_PKG_VERSION")) {
            Ok(v) => tracing::debug!("首见证据：{}（来源 {}）", v.at, v.source),
            Err(e) => tracing::warn!("首见证据写入失败（不影响使用）：{e}"),
        }
        Ok(Arc::new(Self {
            db,
            bus: EventBus::new(),
            cfg,
            local_token,
            jwt_secret,
        }))
    }

    /// 当前界面语言（读 ui_locale 设置；每次读取，切换语言后无需重启）
    pub fn lang(&self) -> i18n::Lang {
        i18n::Lang::from_setting(&self.db.get_setting("ui_locale").ok().flatten().unwrap_or_default())
    }

    pub fn data_dir(&self) -> PathBuf {
        self.cfg.data_dir.clone()
    }
}
