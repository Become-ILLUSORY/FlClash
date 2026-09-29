//! 全局内核状态（等价 `core/hub.go` 里的 isInit / eventListener / runtime 等）。
//!
//! 我们复用 meow-rs 的 `Tunnel` + `DnsServer` + `MixedListener` + `ApiServer`
//! 作为运行时；跨方法共享的句柄都放在这里。

use std::sync::Arc;

use once_cell::sync::OnceCell;
use parking_lot::RwLock;
use tokio::runtime::Runtime;

use crate::orchestrator::Kernel;

/// 进程内 tokio 运行时（多线程，等价 meow main.rs 的 Builder::new_multi_thread）。
pub static RUNTIME: OnceCell<Runtime> = OnceCell::new();

/// 运行中的内核句柄（Tunnel/DNS/listeners/api）。`setupConfig` 后写入。
pub static KERNEL: OnceCell<Arc<RwLock<Option<Kernel>>>> = OnceCell::new();

/// 是否已执行过 initClash（订阅/配置可写）。
pub static INITIALIZED: AtomicBool = AtomicBool::new(false);

/// 事件订阅回调（setEventListener 注入的 InvokeInterface）。
pub static EVENT_LISTENER: OnceCell<Mutex<Option<usize>>> = OnceCell::new();

use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

pub fn get_runtime() -> &'static Runtime {
    RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .thread_name("meow")
            .build()
            .expect("build tokio runtime")
    })
}

pub fn kernel() -> parking_lot::RwLockReadGuard<'static, Option<Kernel>> {
    let cell = KERNEL.get_or_init(|| Arc::new(RwLock::new(None)));
    cell.read()
}

pub fn kernel_mut() -> parking_lot::RwLockWriteGuard<'static, Option<Kernel>> {
    let cell = KERNEL.get_or_init(|| Arc::new(RwLock::new(None)));
    cell.write()
}

/// 读取当前 event listener（无则 None）。
pub fn event_listener() -> Option<*mut c_void> {
    EVENT_LISTENER
        .get()
        .and_then(|m| m.lock().unwrap().clone())
        .map(|x| x as *mut c_void)
}

/// 把一个 JSON 字符串推送给 event listener（等价 Go 的 sendMessage）。
pub fn notify_listener(json: &str) {
    if let Some(listener) = event_listener() {
        crate::callback::invoke_result(listener, json);
    }
}

/// initClash 置位 / 查询。
pub fn set_initialized(v: bool) {
    INITIALIZED.store(v, Ordering::SeqCst);
}

pub fn is_initialized() -> bool {
    INITIALIZED.load(Ordering::SeqCst)
}