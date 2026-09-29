//! FlClash Action 协议的类型定义（JSON）。
//!
//! 与 `core/action.go` + `core/constant.go` 一一对应：
//! 请求 `{"id": .., "method": .., "data": ..}` → 响应 `{"id","method","data","code"}`。
//! 事件推送：`method == "message"` 且 `id` 为空字符串。

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Action {
    #[serde(default)]
    pub id: String,
    pub method: ActionMethod,
    #[serde(default)]
    pub data: serde_json::Value,
}

#[derive(Debug, Clone)]
pub struct ActionResult {
    pub id: String,
    pub method: ActionMethod,
    pub data: serde_json::Value,
    pub code: i32,
}

impl ActionResult {
    pub fn success(id: String, method: ActionMethod, data: impl Serialize) -> Self {
        ActionResult {
            id,
            method,
            data: serde_json::to_value(data).unwrap_or(serde_json::Value::Null),
            code: 0,
        }
    }

    pub fn error(id: String, method: ActionMethod, err: impl ToString) -> Self {
        ActionResult {
            id,
            method,
            data: serde_json::Value::String(err.to_string()),
            code: -1,
        }
    }

    pub fn to_json(&self) -> String {
        serde_json::json!({
            "id": self.id,
            "method": self.method,
            "data": self.data,
            "code": self.code,
        })
        .to_string()
    }
}

/// 事件推送（message 通道）。与 `core/constant.go` 的 Message 一致。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    #[serde(rename = "type")]
    pub msg_type: MessageType,
    pub data: serde_json::Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MessageType {
    Log,
    Delay,
    Request,
    Loaded,
}

macro_rules! action_methods {
    ($( $(#[$attr:meta])* $name:ident ),+ $(,)?) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[serde(rename_all = "camelCase")]
        pub enum ActionMethod {
            $( $(#[$attr])* $name ),+
        }

        impl ActionMethod {
            pub const ALL: &'static [ActionMethod] = &[$(ActionMethod::$name),+];
        }
    };
}

action_methods!(
    // 初始化/生命周期
    #[serde(rename = "initClash")]
    InitClash,
    #[serde(rename = "getIsInit")]
    GetIsInit,
    #[serde(rename = "forceGc")]
    ForceGc,
    #[serde(rename = "shutdown")]
    Shutdown,
    #[serde(rename = "setupConfig")]
    SetupConfig,
    #[serde(rename = "quickSetup")]
    QuickSetup,
    // 配置
    #[serde(rename = "validateConfig")]
    ValidateConfig,
    #[serde(rename = "updateConfig")]
    UpdateConfig,
    #[serde(rename = "getConfig")]
    GetConfig,
    // 代理 / 组
    #[serde(rename = "getProxies")]
    GetProxies,
    #[serde(rename = "changeProxy")]
    ChangeProxy,
    #[serde(rename = "asyncTestDelay")]
    AsyncTestDelay,
    // 流量
    #[serde(rename = "getTraffic")]
    GetTraffic,
    #[serde(rename = "getTotalTraffic")]
    GetTotalTraffic,
    #[serde(rename = "resetTraffic")]
    ResetTraffic,
    // 连接
    #[serde(rename = "getConnections")]
    GetConnections,
    #[serde(rename = "closeConnections")]
    CloseConnections,
    #[serde(rename = "resetConnections")]
    ResetConnections,
    #[serde(rename = "closeConnection")]
    CloseConnection,
    // provider / geo / 资源
    #[serde(rename = "getExternalProviders")]
    GetExternalProviders,
    #[serde(rename = "getExternalProvider")]
    GetExternalProvider,
    #[serde(rename = "updateExternalProvider")]
    UpdateExternalProvider,
    #[serde(rename = "sideLoadExternalProvider")]
    SideLoadExternalProvider,
    #[serde(rename = "updateGeoData")]
    UpdateGeoData,
    #[serde(rename = "getCountryCode")]
    GetCountryCode,
    // 日志 / 监听 / 杂项
    #[serde(rename = "startLog")]
    StartLog,
    #[serde(rename = "stopLog")]
    StopLog,
    #[serde(rename = "startListener")]
    StartListener,
    #[serde(rename = "stopListener")]
    StopListener,
    #[serde(rename = "updateDns")]
    UpdateDns,
    #[serde(rename = "getMemory")]
    GetMemory,
    #[serde(rename = "crash")]
    Crash,
    #[serde(rename = "deleteFile")]
    DeleteFile,
);

pub type ProxiesData = HashMap<String, serde_json::Value>;

/// FlClash 用到的参数类型（序列化对齐 core/hub.go）。
pub use model::*;
mod model {
    use super::*;
    use serde::Deserialize;

    #[derive(Debug, Deserialize)]
    pub struct InitParams {
        #[serde(rename = "home-dir")]
        pub home_dir: String,
        pub version: i32,
    }

    #[derive(Debug, Deserialize)]
    pub struct SetupParams {
        #[serde(rename = "selected-map")]
        pub selected_map: HashMap<String, String>,
        #[serde(rename = "test-url")]
        pub test_url: String,
    }

    #[derive(Debug, Deserialize)]
    pub struct ChangeProxyParams {
        #[serde(rename = "group-name")]
        pub group_name: String,
        #[serde(rename = "proxy-name")]
        pub proxy_name: String,
    }

    #[derive(Debug, Deserialize)]
    pub struct TestDelayParams {
        #[serde(rename = "proxy-name")]
        pub proxy_name: String,
        #[serde(rename = "test-url")]
        pub test_url: String,
        pub timeout: i64,
    }

    #[derive(Debug, Deserialize)]
    pub struct UpdateParams {
        pub tun: Option<serde_json::Value>,
        #[serde(rename = "allow-lan")]
        pub allow_lan: Option<bool>,
        #[serde(rename = "mixed-port")]
        pub mixed_port: Option<i32>,
        pub mode: Option<String>,
        #[serde(rename = "log-level")]
        pub log_level: Option<String>,
        pub ipv6: Option<bool>,
        pub sniffing: Option<bool>,
        #[serde(rename = "tcp-concurrent")]
        pub tcp_concurrent: Option<bool>,
        #[serde(rename = "external-controller")]
        pub external_controller: Option<String>,
        #[serde(rename = "interface-name")]
        pub interface: Option<String>,
        #[serde(rename = "unified-delay")]
        pub unified_delay: Option<bool>,
    }

    #[derive(Debug, Deserialize)]
    pub struct UpdateGeoDataParams {
        #[serde(rename = "geo-type")]
        pub geo_type: String,
        #[serde(rename = "geo-name")]
        pub geo_name: String,
    }
}