//! 日志：安装 tracing 层 + `LogBroadcastLayer`，并把日志广播转发到
//! FlClash 的事件回调（对应 Go 内核的 log 事件 / `startLog`）。
//!
//! meow-rs 的 `meow_api::log_stream` 已提供 `LogBroadcastLayer`
//! （`crates/meow-api/src/log_stream.rs`）与 `install_log_reloader`。

use once_cell::sync::OnceCell;
use tokio::sync::broadcast;

use meow_api::log_stream::{install_log_reloader, LogBroadcastLayer, LogMessage};

/// 全进程日志广播发送端（`startLog` 订阅它）。
static LOG_TX: OnceCell<broadcast::Sender<LogMessage>> = OnceCell::new();

/// 幂等安装：tracing fmt + LogBroadcastLayer。
pub fn init() {
    if LOG_TX.get().is_some() {
        return;
    }
    use tracing_subscriber::filter::LevelFilter;
    use tracing_subscriber::prelude::*;

    let (tx, _) = broadcast::channel(256);
    if LOG_TX.set(tx.clone()).is_err() {
        return;
    }

    let env_filter = || tracing_subscriber::EnvFilter::new("info");
    let log_layer = LogBroadcastLayer { tx: tx.clone() }.with_filter(LevelFilter::TRACE);
    let (filter_layer, reload_handle) = tracing_subscriber::reload::Layer::new(env_filter());

    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer().with_filter(filter_layer))
        .with(log_layer)
        .init();

    install_log_reloader(move |level| {
        // mihomo 用 warning/silent，EnvFilter 只认 warn/off
        let normalized = match level.to_ascii_lowercase().as_str() {
            "warning" => "warn".to_string(),
            "silent" => "off".to_string(),
            other => other.to_string(),
        };
        reload_handle
            .reload(tracing_subscriber::EnvFilter::new(&normalized))
            .map_err(|e| e.to_string())
    });
}

/// 订阅日志并转发到 event listener（`startLog`）。
/// 返回一个 abortable task。
pub fn spawn_forwarder() -> Option<tokio::task::JoinHandle<()>> {    let tx = LOG_TX.get()?;
    let mut rx = tx.subscribe();
    Some(tokio::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(msg) => {
                    let json = serde_json::json!({
                        "type": "log",
                        "data": {
                            "level": format!("{:?}", msg.level).to_lowercase(),
                            "payload": msg.payload,
                            "time": msg.time.to_string(),
                        }
                    });
                    crate::state::notify_listener(&json.to_string());
                }
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    }))
}

/// 运行时改日志级别（`updateConfig.log-level`）。
pub fn set_level(level: &str) -> Result<(), String> {
    meow_api::log_stream::reload_log_level(level)
}

/// 当前转发的日志任务句柄（startLog/stopLog 用）。
static LOG_TASK: OnceCell<parking_lot::Mutex<Option<tokio::task::JoinHandle<()>>>> = OnceCell::new();

fn task_slot() -> &'static parking_lot::Mutex<Option<tokio::task::JoinHandle<()>>> {
    LOG_TASK.get_or_init(|| parking_lot::Mutex::new(None))
}

/// 开始转发日志事件（幂等：先停再启）。
pub fn start_forwarding() -> bool {
    init();
    stop_forwarding();
    if let Some(task) = spawn_forwarder() {
        *task_slot().lock() = Some(task);
        true
    } else {
        false
    }
}

/// 停止转发日志事件。
pub fn stop_forwarding() -> bool {
    if let Some(t) = task_slot().lock().take() {
        t.abort();
    }
    true
}