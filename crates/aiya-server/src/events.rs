// 事件总线：SSE 的生产者侧。桌面壳的 app.emit 在 server 壳的等价物——
// 命令与后台线程往这里发，SSE handler 订阅后按通道转发给浏览器。

use serde_json::Value;
use tokio::sync::broadcast;

#[derive(Clone)]
pub struct EventBus {
    tx: broadcast::Sender<(String, String)>,
}

impl EventBus {
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(256);
        Self { tx }
    }

    /// 发一条事件（无订阅者时静默丢弃——broadcast 的正常路径）。
    pub fn emit(&self, channel: &'static str, payload: &Value) {
        let _ = self.tx.send((channel.to_string(), payload.to_string()));
    }

    pub fn subscribe(&self) -> broadcast::Receiver<(String, String)> {
        self.tx.subscribe()
    }
}
