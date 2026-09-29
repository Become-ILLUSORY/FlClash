//! meow-flclash-ffi —— FlClash 与 meow-rs 的 C ABI 桥（libclash.so）。
//!
//! 导出与 cgo 自动生成的 `libclash.h` 完全一致的符号，使
//! `android/core/src/main/cpp/core.cpp`（JNI）和 `Core.kt` 无需改动。
//!
//! 运行模型：
//! - `invokeAction(cb, json)` / `quickSetup(...)` 会各自起一个 OS 线程，
//!   在该线程里 `block_on` 我们的 tokio runtime，避免嵌套 runtime。
//! - `getTraffic/getTotalTraffic` 同步读 `Tunnel::statistics()`，不阻塞。

mod action;
mod callback;
mod geo;
mod handlers;
mod http;
mod logging;
mod orchestrator;
#[cfg(target_os = "android")]
mod protect;
mod state;
mod tun;

use std::os::raw::{c_char, c_int, c_void};
use std::sync::OnceLock;

use parking_lot::Mutex;
use serde_json::json;

use action::{Action, ActionResult};

// ---------------------------------------------------------------- 文本缓冲

/// 给同步返回 C 字符串（getTraffic/getTotalTraffic）用的固定静态缓冲。
type Buf = [u8; 4096];
static TRAFFIC_BUF: OnceLock<Mutex<Buf>> = OnceLock::new();

fn fill_c_buf(s: &str) -> *mut c_char {
    let buf: &Mutex<Buf> = TRAFFIC_BUF.get_or_init(|| Mutex::new([0u8; 4096]));
    let mut g = buf.lock();
    let bytes = s.as_bytes();
    let n = bytes.len().min(g.len() - 1);
    g[..n].copy_from_slice(&bytes[..n]);
    g[n] = 0;
    g.as_mut_ptr() as *mut c_char
}

// ---------------------------------------------------------------- 回调注入桩

/// 一个 OS 线程里跑 block_on；用于异步派发，避免嵌套 runtime。
fn spawn_async(f: impl FnOnce() + Send + 'static) {
    std::thread::Builder::new()
        .name("meow-act".into())
        .spawn(f)
        .ok();
}

fn run_result(cb: *mut c_void, res: &ActionResult) {
    callback::invoke_result(cb, &res.to_json());
}

// ---------------------------------------------------------------- 导出函数

/// 启动 TUN（Android）。同时把 TunInterface 接成 meow-rs 的 SocketProtector
/// （出站 socket 走 VpnService.protect，避免回流进 VPN）。
/// 注：meow-rs 的 TunListener 目前不接受外部 fd（见 tun.rs），此处先装 protect + 暂存 fd。
#[no_mangle]
pub extern "C" fn startTUN(
    callback: *mut c_void,
    fd: c_int,
    stack: *const c_char,
    address: *const c_char,
    dns: *const c_char,
) {
    logging::init();
    #[cfg(target_os = "android")]
    protect::install(callback);
    let stack = cstr(stack);
    let address = cstr(address);
    let dns = cstr(dns);
    tun::start_tun_with_fd(fd, &stack, &address, &dns);
}

#[no_mangle]
pub extern "C" fn stopTun() {
    #[cfg(target_os = "android")]
    protect::uninstall();
    tun::stop_tun_fd();
    if let Some(k) = state::kernel_mut().as_mut() {
        state::get_runtime().block_on(orchestrator::teardown(k));
    }
}

#[no_mangle]
pub extern "C" fn forceGC() {
    // meow-rs 无显式 GC 接口；作为占位保留。
}

#[no_mangle]
pub extern "C" fn updateDns(s: *const c_char) {
    let s = cstr(s);
    let _ = handlers::update_dns(&s);
}

#[no_mangle]
pub extern "C" fn invokeAction(callback: *mut c_void, params: *const c_char) {
    logging::init();
    let params = cstr(params);
    spawn_async(move || {
        let action: Option<Action> = serde_json::from_str(&params).ok();
        let res = match action {
            Some(a) => handlers::dispatch(&a),
            None => ActionResult::error(String::new(), action::ActionMethod::GetIsInit, "bad json"),
        };
        run_result(callback, &res);
    });
}

#[no_mangle]
pub extern "C" fn setEventListener(listener: *mut c_void) {
    let cell = state::EVENT_LISTENER.get_or_init(|| {
        std::sync::Mutex::new(None)
    });
    let mut g = cell.lock().unwrap();
    if listener.is_null() {
        *g = None;
    } else {
        *g = Some(listener);
    }
}

/// 同步获取本秒流量 {up, down}。
#[no_mangle]
pub extern "C" fn getTotalTraffic(_only_statistics_proxy: c_int) -> *mut c_char {
    fill_c_buf(&handlers::total_traffic().to_string())
}

#[no_mangle]
pub extern "C" fn getTraffic(_only_statistics_proxy: c_int) -> *mut c_char {
    fill_c_buf(&handlers::traffic().to_string())
}

#[no_mangle]
pub extern "C" fn suspend(suspended: c_int) {
    let _ = handlers::suspend(suspended != 0);
}

/// init + setup 一次性通道（对应 Go 的 quickSetup）。
#[no_mangle]
pub extern "C" fn quickSetup(
    callback: *mut c_void,
    init_params: *const c_char,
    setup_params: *const c_char,
) {
    let init = cstr(init_params);
    let setup = cstr(setup_params);
    let cb = Cb(callback as usize);
    spawn_async(move || {
        // 1) init
        let init_action = acc(init_params_json(&init), action::ActionMethod::InitClash);
        let mut holder = handlers::dispatch(&init_action);
        // 2) setup
        let setup_val: serde_json::Value =
            serde_json::from_str(&setup).unwrap_or(serialize_setup_from_json(&setup));
        let setup_action = acc(setup_val, action::ActionMethod::SetupConfig);
        let res = handlers::dispatch(&setup_action);
        holder = if holder.code == 0 { res } else { holder };
        run_result(cb.0 as *mut c_void, &holder);
    });
}

// ---------------------------------------------------------------- helpers

fn cstr(p: *const c_char) -> String {
    if p.is_null() {
        return String::new();
    }
    unsafe { std::ffi::CStr::from_ptr(p) }
        .to_string_lossy()
        .into_owned()
}

fn init_params_json(s: &str) -> serde_json::Value {
    serde_json::from_str(s).unwrap_or_else(|_| json!({ "home-dir": s, "version": 0 }))
}

/// quickSetup 收到的是 JSON 字符串，as-is 包成 data。
fn serialize_setup_from_json(s: &str) -> serde_json::Value {
    serde_json::from_str(s).unwrap_or(json!({}))
}

fn acc(data: serde_json::Value, method: action::ActionMethod) -> Action {
    Action {
        id: format!("quick#{:?}", std::thread::current().id()),
        method,
        data,
    }
}

/// 把 C 回调指针包成 Send 类型（usize 视图），便于跨线程捕获。
#[derive(Clone, Copy)]
struct Cb(usize);
unsafe impl Send for Cb {}
unsafe impl Sync for Cb {}