//! 运行配置：部署模式（本地/局域网/服务器）、端口、数据目录、静态资源目录

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq)]
pub enum RunMode {
    /// 本地模式（默认）：仅监听 127.0.0.1，免登录
    Local,
    /// 局域网模式：监听 0.0.0.0，需访问密码
    Lan,
    /// 服务器模式：无窗口纯后端，需访问密码
    Server,
}

impl RunMode {
    pub fn parse(s: &str) -> Self {
        match s.to_ascii_lowercase().as_str() {
            "lan" => RunMode::Lan,
            "server" => RunMode::Server,
            _ => RunMode::Local,
        }
    }
    pub fn as_str(&self) -> &'static str {
        match self {
            RunMode::Local => "local",
            RunMode::Lan => "lan",
            RunMode::Server => "server",
        }
    }
    /// 是否监听所有网卡
    pub fn binds_all(&self) -> bool {
        !matches!(self, RunMode::Local)
    }
}

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub mode: RunMode,
    pub port: u16,
    pub data_dir: PathBuf,
    /// 前端静态资源目录（axum 托管；桌面端与浏览器端共用）
    pub web_dir: Option<PathBuf>,
    /// 是否开启局域网访问（本地模式下的可选开关）
    pub lan_enabled: bool,
}

impl AppConfig {
    /// 解析启动参数与默认值（优先级：参数 > 环境变量 > 默认）
    pub fn from_args_and_env() -> Self {
        let mut mode = std::env::var("MINDMATE_MODE").unwrap_or_else(|_| "local".into());
        let mut port: u16 = std::env::var("MINDMATE_PORT")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(17801);
        let mut data_dir = std::env::var("MINDMATE_DATA_DIR").ok().map(PathBuf::from);
        let mut lan_enabled = std::env::var("MINDMATE_LAN").map(|v| v == "1").unwrap_or(false);

        let args: Vec<String> = std::env::args().collect();
        let mut i = 1;
        while i < args.len() {
            match args[i].as_str() {
                "--mode" if i + 1 < args.len() => {
                    mode = args[i + 1].clone();
                    i += 2;
                }
                "--port" if i + 1 < args.len() => {
                    port = args[i + 1].parse().unwrap_or(port);
                    i += 2;
                }
                "--data-dir" if i + 1 < args.len() => {
                    data_dir = Some(PathBuf::from(&args[i + 1]));
                    i += 2;
                }
                "--lan" => {
                    lan_enabled = true;
                    i += 1;
                }
                "--headless" => {
                    if mode == "local" {
                        mode = "server".into();
                    }
                    i += 1;
                }
                _ => i += 1,
            }
        }

        let data_dir = data_dir.unwrap_or_else(default_data_dir);
        Self {
            mode: RunMode::parse(&mode),
            port,
            data_dir,
            web_dir: std::env::var("MINDMATE_WEB_DIR").ok().map(PathBuf::from),
            lan_enabled,
        }
    }

    pub fn db_path(&self) -> PathBuf {
        self.data_dir.join("data.db")
    }
    pub fn log_dir(&self) -> PathBuf {
        self.data_dir.join("logs")
    }
    /// 实际绑定地址
    pub fn bind_addr(&self) -> &'static str {
        if self.mode.binds_all() || self.lan_enabled {
            "0.0.0.0"
        } else {
            "127.0.0.1"
        }
    }
    /// 是否需要登录（局域网/服务器模式需要）
    pub fn requires_login(&self) -> bool {
        self.mode.binds_all()
    }
}

fn default_data_dir() -> PathBuf {
    if let Some(d) = dirs::data_dir() {
        d.join("Mindmate")
    } else {
        Path::new(".").join("mindmate-data")
    }
}
