//! 事件总线：写操作后广播，经 SSE 推送给所有在线端（桌面 + 浏览器）

use serde::Serialize;
use tokio::sync::broadcast;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Event {
    /// 事件类型：node.created / todo.updated / reminder.triggered …
    pub kind: String,
    /// 附加数据（JSON）
    pub payload: serde_json::Value,
    pub at: String,
}

impl Event {
    pub fn new(kind: &str, payload: serde_json::Value) -> Self {
        Self {
            kind: kind.to_string(),
            payload,
            at: chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
        }
    }
}

#[derive(Clone)]
pub struct EventBus {
    tx: broadcast::Sender<Event>,
}

impl EventBus {
    pub fn new() -> Self {
        let (tx, _rx) = broadcast::channel(1024);
        Self { tx }
    }
    pub fn publish(&self, ev: Event) {
        // 无订阅者时 send 返回 Err，忽略即可
        let _ = self.tx.send(ev);
    }
    pub fn subscribe(&self) -> broadcast::Receiver<Event> {
        self.tx.subscribe()
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}
