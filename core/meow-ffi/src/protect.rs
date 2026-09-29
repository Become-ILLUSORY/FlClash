//! Android 出站 socket 保护：把 JNI 的 `protect_func`
//! （`TunInterface.protect(int)` → `VpnService.protect(fd)`）桥进 meow-rs 的
//! `SocketProtector`，让内核自己的出站 socket 不回流进 VPN。
//!
//! meow-rs 已提供钩子：`meow_common::socket_protect::{SocketProtector,
//! set_socket_protector, clear_socket_protector}`（`crates/meow-common/src/
//! socket_protect.rs:208/221/226`，`cfg(any(target_os="android", test+unix))`）。
//! 所有经 `meow_common::{connect_tcp, connect_tcp_host, bind_udp}`
//! 建立的 socket 都会先调用它。

use std::os::unix::io::RawFd;
use std::os::raw::c_void;
use std::sync::Arc;

use meow_common::socket_protect::{clear_socket_protector, set_socket_protector, SocketProtector};

/// 把 `TunInterface` 的回调指针包成 meow 的 SocketProtector。
struct JniSocketProtector {
    /// `TunInterface` 的 Java 对象全局引用（core.cpp 传入的 callback）。
    iface: *mut c_void,
}

// 回调指针只在 VPN 生命周期内有效，跨线程传递由 meow 的 socket 建立路径决定；
// Java 侧 protect() 本身线程安全。
unsafe impl Send for JniSocketProtector {}
unsafe impl Sync for JniSocketProtector {}

impl SocketProtector for JniSocketProtector {
    fn protect(&self, fd: RawFd) -> std::io::Result<()> {
        crate::callback::call_protect(self.iface, fd);
        Ok(())
    }
}

/// startTUN 时安装（TunInterface 在此刻才拿得到）。
pub fn install(iface: *mut c_void) {
    if iface.is_null() {
        return;
    }
    set_socket_protector(Arc::new(JniSocketProtector { iface }));
}

/// stopTun 时卸载。
pub fn uninstall() {
    clear_socket_protector();
}