//! 每个 ActionMethod 的实现 + 分发。
//!
//! 大原则：
//! - 需要查询 mihomo 风格快照（proxies/connections/memory）的，走进程内 meow-api 回环 REST。
//! - 配置生命周期（init/validate/setup/shutdown）直接操作 meow-config / orchestrator。
//! - 流量累计走 Tunnel::statistics（避免 HTTP 轮询开销）。

use std::sync::Mutex;

use once_cell::sync::OnceCell;
use serde_json::json;

use crate::action::{
    Action, ActionResult, ActionMethod, ChangeProxyParams, InitParams, SetupParams,
    TestDelayParams, UpdateParams,
};
use crate::orchestrator::{assemble, teardown};
use crate::state;

/// initClash 记住的 home 目录。
pub static HOME_DIR: OnceCell<Mutex<String>> = OnceCell::new();

fn home_dir() -> String {
    HOME_DIR
        .get_or_init(|| Mutex::new(String::new()))
        .lock()
        .unwrap()
        .clone()
}

/// 默认测速 URL（跟 mihomo 一致）。
pub static TEST_URL: Mutex<&'static str> = Mutex::new("https://www.gstatic.com/generate_204");

// ---------------------------------------------------------------- 生命周期

fn init_clash(params: &InitParams) -> bool {
    meow_common::set_home_dir(std::path::PathBuf::from(&params.home_dir));
    *HOME_DIR.get_or_init(|| Mutex::new(params.home_dir.clone())).lock().unwrap() =
        params.home_dir.clone();
    state::set_initialized(true);
    true
}

fn shutdown() -> bool {
    state::get_runtime().block_on(async {
        if let Some(guard) = state::kernel_mut().as_mut() {
            teardown(guard).await;
        }
    });
    state::set_initialized(false);
    true
}

fn validate_config(path: &str) -> Result<serde_json::Value, String> {
    let content = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    meow_config::set_offline_validate(true);
    state::get_runtime().block_on(async {
        meow_config::load_config_from_str(&content)
            .await
            .map(|_| json!(true))
            .map_err(|e| e.to_string())
    })
}

fn setup_config(data: &serde_json::Value) -> Result<serde_json::Value, String> {
    if !state::is_initialized() {
        return Err("not initialized".into());
    }
    let params: SetupParams = serde_json::from_value(data.clone())
        .map_err(|e| format!("bad setup params: {e}"))?;
    if !params.test_url.is_empty() {
        *TEST_URL.lock().unwrap() = params.test_url;
    }

    let config_path = format!("{}/config.yaml", home_dir());
    let content = std::fs::read_to_string(&config_path).map_err(|e| e.to_string())?;

    state::get_runtime().block_on(async move {
        let config = meow_config::load_config_from_str(&content)
            .await
            .map_err(|e| format!("load config: {e}"))?;
        // 回环 API 端口给 0（随机），避免固定端口冲突
        let kernel = assemble(config, &home_dir(), 0)
            .await
            .map_err(|e| format!("assemble: {e}"))?;
        // 应用 selected-map：对每个组切换选中代理
        for (group, proxy) in &params.selected_map {
            let _ = change_proxy_now(
                &kernel.tunnel,
                &ChangeProxyParams {
                    group_name: group.clone(),
                    proxy_name: proxy.clone(),
                },
            );
        }
        *state::kernel_mut() = Some(kernel);
        Ok::<_, String>(json!(true))
    })
}

fn get_config(path: &str) -> Result<serde_json::Value, String> {
    let content = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    state::get_runtime().block_on(async {
        let config = meow_config::load_config_from_str(&content)
            .await
            .map_err(|e| e.to_string())?;
        // 用 raw 快照序列化成 FlClash 期望的 Map；并补 `rules` 别名（Dart 端读了再改名为 rule）。
        let mut v = serde_json::to_value(&config.raw).map_err(|e| e.to_string())?;
        if let Some(rules) = v.get("rules").cloned() {
            v["rule"] = rules;
        }
        Ok(v)
    })
}

// ---------------------------------------------------------------- 运行时配置

fn update_config(data: &serde_json::Value) -> Result<serde_json::Value, String> {
    let params: UpdateParams = serde_json::from_value(data.clone()).map_err(|e| e.to_string())?;
    let guard = state::kernel();
    let kernel = guard.as_ref().ok_or("not running")?;
    // 仅支持 mihomo 同名的字段；其它 meow 暂未实现的标记 no-op。
    if let Some(mode) = &params.mode {
        if let Ok(m) = mode.parse() {
            kernel.tunnel.set_mode(m);
        }
    }
    if let Some(level) = &params.log_level {
        let _ = crate::logging::set_level(level);
    }
    Ok(json!(true))
}

// ---------------------------------------------------------------- 代理/组/延迟

/// 直接操作 Tunnel 切换组内代理（不入 HTTP）。失败返回错误串。
fn change_proxy_now(
    tunnel: &meow_tunnel::Tunnel,
    p: &ChangeProxyParams,
) -> Result<(), String> {
    let snapshot = tunnel.route_snapshot();
    let Some(group) = snapshot.proxies.get(p.group_name.as_str()) else {
        return Err("Not found group".into());
    };
    let Some(sel) = group.selection() else {
        return Err("Group is not selectable".into());
    };
    if p.proxy_name.is_empty() {
        sel.force_set(None);
    } else {
        state::get_runtime().block_on(async {
            sel.set(&p.proxy_name).await.map_err(|e| e.to_string())
        })?;
    }
    Ok(())
}

fn change_proxy(data: &serde_json::Value) -> Result<serde_json::Value, String> {
    let p: ChangeProxyParams = serde_json::from_value(data.clone()).map_err(|e| e.to_string())?;
    let guard = state::kernel();
    let kernel = guard.as_ref().ok_or("not running")?;
    change_proxy_now(&kernel.tunnel, &p)?;
    Ok(json!(""))
}

async fn get_proxies_async() -> Result<serde_json::Value, String> {
    let base = api_base()?;
    let secret = api_creds().1;
    // meow-api：GET /proxies（全部）、GET /group（仅组）——均返回 mihomo 风格 {"proxies":{...}}
    let proxies_raw = crate::http::api_get(&base, &secret, "/proxies").await?;
    let group_raw = crate::http::api_get(&base, &secret, "/group").await?;
    let proxies: serde_json::Value =
        serde_json::from_str(&proxies_raw).map_err(|e| e.to_string())?;
    let groups: serde_json::Value = serde_json::from_str(&group_raw).map_err(|e| e.to_string())?;

    let proxies_map = proxies
        .get("proxies")
        .cloned()
        .unwrap_or(serde_json::Value::Object(Default::default()));
    // FlClash 期望 "all" = 组名列表（meow 的 /group 只返回组）
    let all: Vec<serde_json::Value> = groups
        .get("proxies")
        .and_then(|p| p.as_object())
        .map(|m| m.keys().cloned().map(serde_json::Value::String).collect())
        .unwrap_or_default();
    Ok(json!({ "proxies": proxies_map, "all": all }))
}

fn get_proxies() -> Result<serde_json::Value, String> {
    state::get_runtime().block_on(get_proxies_async())
}

async fn async_test_delay(data: &serde_json::Value) -> Result<serde_json::Value, String> {
    let p: TestDelayParams = serde_json::from_value(data.clone()).map_err(|e| e.to_string())?;
    let base = api_base()?;
    let secret = api_creds().1;
    let test_url = if p.test_url.is_empty() {
        TEST_URL.lock().unwrap().clone()
    } else {
        p.test_url.clone()
    };
    let url = format!(
        "/proxies/{}/delay?url={}&timeout={}",
        urlencode(&p.proxy_name),
        urlencode(&test_url),
        p.timeout
    );
    let body = crate::http::api_get(&base, &secret, &url).await?;
    let v: serde_json::Value = serde_json::from_str(&body).map_err(|e| e.to_string())?;
    let value = v.get("delay").and_then(|d| d.as_i64()).unwrap_or(-1);
    Ok(json!({
        "name": p.proxy_name,
        "url": p.test_url,
        "value": value as i32,
    }))
}

fn test_delay(data: &serde_json::Value) -> Result<serde_json::Value, String> {
    state::get_runtime().block_on(async_test_delay(data))
}

// ---------------------------------------------------------------- 流量/连接/内存

pub fn traffic() -> serde_json::Value {
    let guard = state::kernel();
    match guard.as_ref() {
        Some(k) => {
            let (ur, dr, _, _) = k.tunnel.statistics().traffic_snapshot();
            json!({ "up": ur as i64, "down": dr as i64 })
        }
        None => json!({ "up": 0, "down": 0 }),
    }
}

pub fn total_traffic() -> serde_json::Value {
    let guard = state::kernel();
    match guard.as_ref() {
        Some(k) => {
            let (up, down) = k.tunnel.statistics().snapshot();
            json!({ "up": up as i64, "down": down as i64 })
        }
        None => json!({ "up": 0, "down": 0 }),
    }
}

fn reset_traffic() -> bool {
    // meow-rs Statistics 未提供重置累计接口；按成功返回（后续如上游加入再接线）。
    true
}

async fn get_connections_async() -> Result<serde_json::Value, String> {
    let base = api_base()?;
    let secret = api_creds().1;
    let body = crate::http::api_get(&base, &secret, "/connections").await?;
    serde_json::from_str(&body).map_err(|e| e.to_string())
}

fn get_connections() -> Result<serde_json::Value, String> {
    state::get_runtime().block_on(get_connections_async())
}

async fn close_connections_async() -> Result<serde_json::Value, String> {
    // 直接走统计层（无需 HTTP）：关闭全部 API 跟踪的连接
    if let Some(k) = state::kernel().as_ref() {
        k.tunnel.statistics().close_all_connections_counted();
    }
    Ok(json!(true))
}

async fn close_connection_async(id: &str) -> Result<serde_json::Value, String> {
    let uuid = uuid::Uuid::parse_str(id).map_err(|e| format!("bad connection id: {e}"))?;
    if let Some(k) = state::kernel().as_ref() {
        k.tunnel.statistics().close_connection(uuid);
    }
    Ok(json!(true))
}

fn reset_connections() -> bool {
    // mihomo 的 resetConnections 等价于重置连接计数；meow 无同类语义，返回 true。
    true
}

fn get_memory() -> Result<serde_json::Value, String> {
    let base = api_base()?;
    let secret = api_creds().1;
    let body = state::get_runtime().block_on(crate::http::api_get(&base, &secret, "/memory"))?;
    let v: serde_json::Value = serde_json::from_str(&body).map_err(|e| e.to_string())?;
    // mihomo 的 /memory 返回 { "inuse": N }；FlClash 期望字符串数字
    let n = v.get("inuse").and_then(|x| x.as_u64()).unwrap_or(0);
    Ok(json!(n.to_string()))
}

// ---------------------------------------------------------------- provider / geo

fn external_providers() -> Result<serde_json::Value, String> {
    let base = api_base()?;
    let secret = api_creds().1;
    let body =
        state::get_runtime().block_on(crate::http::api_get(&base, &secret, "/providers/proxies"))?;
    let v: serde_json::Value = serde_json::from_str(&body).map_err(|e| e.to_string())?;
    // meow-api 返回 {"providers": {name: {...}}}，转成 FlClash 的数组
    let providers = v
        .get("providers")
        .and_then(|p| p.as_object())
        .map(|m| m.values().cloned().collect::<Vec<_>>())
        .unwrap_or_default();
    Ok(json!(providers))
}

fn external_provider(name: &str) -> Result<serde_json::Value, String> {
    let base = api_base()?;
    let secret = api_creds().1;
    let body = state::get_runtime().block_on(crate::http::api_get(
        &base,
        &secret,
        &format!("/providers/proxies/{}", urlencode(name)),
    ))?;
    serde_json::from_str(&body).map_err(|e| e.to_string())
}

async fn update_external_provider(name: &str) -> Result<serde_json::Value, String> {
    let base = api_base()?;
    let secret = api_creds().1;
    crate::http::request(
        "PUT",
        &format!("{base}/providers/proxies/{}", urlencode(name)),
        &secret,
        None,
    )
    .await?;
    Ok(json!(""))
}

fn country_code(ip: &str) -> Option<String> {
    let ip: std::net::IpAddr = ip.parse().ok()?;
    crate::geo::lookup_country_code(ip, None)
}

fn update_geo_data(_params: &serde_json::Value) -> Result<serde_json::Value, String> {
    // TODO(meow): meow-rs 内置 geodata_fetch（启动自动拉取），暂不提供运行时手动更新
    Err("updateGeoData: not implemented for meow-rs (auto-update on startup)".into())
}

// ---------------------------------------------------------------- 日志/监听/杂项

fn start_log() -> bool {
    crate::logging::start_forwarding()
}

fn stop_log() -> bool {
    crate::logging::stop_forwarding()
}

fn start_listener() -> bool {
    // 监听器在 assemble 时已随隧道启动；此处按 mihomo 语义返回 true。
    true
}

fn stop_listener() -> bool {
    true
}

pub fn update_dns(_s: &str) -> bool {
    // meow-rs 的 DNS 由配置文件管理，无外部运行时改 DNS 接口，按成功返回。
    true
}

pub fn suspend(_s: bool) -> bool {
    // meow-rs 无 suspend 语义；保留接口占位。
    true
}

fn crash() {
    panic!("handle invoke crash (meow)");
}

fn delete_file(path: &str) -> Result<serde_json::Value, String> {
    let meta = std::fs::metadata(path).map_err(|e| e.to_string())?;
    if meta.is_dir() {
        std::fs::remove_dir_all(path).map_err(|e| e.to_string())?;
    } else {
        std::fs::remove_file(path).map_err(|e| e.to_string())?;
    }
    Ok(json!(""))
}

// ---------------------------------------------------------------- 分发

pub fn dispatch(action: &Action) -> ActionResult {
    let method = action.method;
    let id = action.id.clone();
    let result = match method {
        ActionMethod::InitClash => do_init(action.data.clone()),
        ActionMethod::GetIsInit => Ok(json!(state::is_initialized())),
        ActionMethod::ForceGc => Ok(json!(true)), // meow 无显式 GC
        ActionMethod::Shutdown => Ok(json!(shutdown())),
        ActionMethod::ValidateConfig => {
            let path = as_str(&action.data).into_owned();
            validate_config(&path)
        }
        ActionMethod::SetupConfig => setup_config(&action.data),
        ActionMethod::GetConfig => {
            let path = as_str(&action.data).into_owned();
            get_config(&path)
        }
        ActionMethod::UpdateConfig => update_config(&action.data),
        ActionMethod::GetProxies => get_proxies(),
        ActionMethod::ChangeProxy => change_proxy(&action.data),
        ActionMethod::AsyncTestDelay => test_delay(&action.data),
        ActionMethod::GetTraffic => Ok(traffic()),
        ActionMethod::GetTotalTraffic => Ok(total_traffic()),
        ActionMethod::ResetTraffic => Ok(json!(reset_traffic())),
        ActionMethod::GetConnections => get_connections(),
        ActionMethod::CloseConnections => {
            state::get_runtime().block_on(close_connections_async())
        }
        ActionMethod::CloseConnection => {
            let id = as_str(&action.data).into_owned();
            state::get_runtime().block_on(close_connection_async(&id))
        }
        ActionMethod::ResetConnections => Ok(json!(reset_connections())),
        ActionMethod::GetExternalProviders => external_providers(),
        ActionMethod::GetExternalProvider => {
            let name = as_str(&action.data).into_owned();
            external_provider(&name)
        }
        ActionMethod::UpdateExternalProvider => {
            let name = as_str(&action.data).into_owned();
            state::get_runtime().block_on(update_external_provider(&name))
        }
        ActionMethod::SideLoadExternalProvider => {
            Err("sideLoadExternalProvider: not implemented for meow-rs".into())
        }
        ActionMethod::UpdateGeoData => update_geo_data(&action.data),
        ActionMethod::GetCountryCode => {
            let ip = as_str(&action.data);
            Ok(json!(country_code(&ip).unwrap_or_default()))
        }
        ActionMethod::StartLog => Ok(json!(start_log())),
        ActionMethod::StopLog => Ok(json!(stop_log())),
        ActionMethod::StartListener => Ok(json!(start_listener())),
        ActionMethod::StopListener => Ok(json!(stop_listener())),
        ActionMethod::UpdateDns => {
            let s = as_str(&action.data).into_owned();
            Ok(json!(update_dns(&s)))
        }
        ActionMethod::GetMemory => get_memory(),
        ActionMethod::Crash => {
            crash();
            Ok(json!(true))
        }
    