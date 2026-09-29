//! Android TUN：把 `VpnService` 创建的 fd 接进 meow-rs 的数据面。
//!
//! ## 现状与上游缺口
//! meow-rs 当前 `TunListener` 用 `tun-rs::DeviceBuilder` **自建设备**（
//! `crates/meow-listener/src/tun/mod.rs:424`），并**不接受外部 fd**。
//! Android 上设备必须由 `VpnService` 创建后把 fd 传进来，因此需要给 meow-rs
//! 打一个上游补丁（建议加 `TunListenerConfig.device_fd: Option<RawFd>`，构造时
//! 若传入 fd 则跳过 `DeviceBuilder`，用 `tun_rs` 从 fd 建设备）——本文件是那个
//! 接入点：`start_tun_with_fd` 目前先把 fd 存进全局并记日志，待补丁合入后在此接线。
//!
//! 临时方案：若补丁未合入，Android 端退化为「应用内代理」（mixed 监听 + protect），
//! 不启用系统 VPN 的 TUN 数据面，功能可先跑通。

use std::os::raw::c_int;
use std::sync::Mutex;
use std::sync::OnceLock;

/// 最近一次 `startTUN` 传入的 fd（为未来的外部 fd 补丁预留）。
pub static PENDING_TUN_FD: OnceLock<Mutex<Option<(c_int, String, String, String)>>> =
    OnceLock::new();

fn tt() -> &'static Mutex<Option<(c_int, String, String, String)>> {
    PENDING_TUN_FD.get_or_init(|| Mutex::new(None))
}

/// 由 C 层 startTUN 调用：记录 fd/config，并（在补丁可用时）启动 TUN。
pub fn start_tun_with_fd(fd: c_int, stack: &str, address: &str, dns: &str) {
    tracing::info!(
        "startTUN received fd={fd} stack={stack} address={address} dns={dns}"
    );
    *tt().lock().unwrap() = Some((
        fd,
        stack.to_string(),
        address.to_string(),
        dns.to_string(),
    ));

    // TODO(meow-rs 上游补丁): 在 Kernel 已 assemble 的情况下，把 fd 交给
    // TunListener（config.tun.enable + device_fd=Some(fd)），并调用
    // `tunnel.set_tun_handle(...)`。在此之前打印警告，但不崩溃。
    tracing::warn!(
        "meow-rs 尚未支持外部 fd TUN；fd={fd} 已暂存，等待上游补丁后接线"
    );
}

/// 清空外部 fd 状态（stopTun 用）。
pub fn stop_tun_fd() {
    *tt().lock().unwrap() = None;
}