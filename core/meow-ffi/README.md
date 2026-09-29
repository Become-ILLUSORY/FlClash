# meow-flclash-ffi

FlClash 与 meow-rs 的 C ABI 桥，产出 `libclash.so`，是原 Go 内核（`core/`）的替代品。
导出符号与 cgo 生成的 `libclash.h` 一致，因此 **Android JNI 层与 Flutter 前端零改动**。

## 目录
- `include/libclash.h`  —— C ABI 定义（自维护，替代 cgo 自动生成的头）
- `src/lib.rs`          —— C 导出 + runtime 调度
- `src/action.rs`       —— Action/ActionResult/message JSON 协议（对应 core/action.go + constant.go）
- `src/state.rs`        —— 全局初始化/运行句柄（对应 core/hub.go 的 isInit/eventListener）
- `src/orchestrator.rs` —— 内核装配（移植 meow main.rs::run：Tunnel/DNS/监听/meow-api）
- `src/handlers.rs`     —— 每个 ActionMethod 的实现（对应 core/hub.go + common.go）
- `src/geo.rs`          —— getCountryCode 的 MaxMind 查表
- `src/http.rs`         —— 进程内 meow-api 回环查询客户端
- `src/tun.rs`          —— Android VpnService 外置 fd 接入点（等待 meow-rs 上游补丁）
- `src/protect.rs`      —— 把 JNI `protect_func` 桥到 meow 的 `SocketProtector`（仅 Android）
- `src/logging.rs`      —— tracing + LogBroadcastLayer 安装与日志事件转发
- `src/callback.rs`     —— 供 core.cpp JNI_OnLoad 注入的函数指针

## 依赖
- `core/meow-rs` git 子模块（meow-rs 工作区 crate，path 依赖，固定 commit）
- cargo features：`full-proxy`（ss/trojan/vless/vmess/hysteria2/anytls/dns/监听/http/socks5/mixed/tun）

## 构建（Android）
见根目录 `scripts/build-meow-core.sh`；CI 见 `.github/workflows/meow-core.yml`。

## 状态
这是迁移的基础脚手架：协议、C ABI、TUN/DNS/API 装配、protect 桥、日志转发、绝大部分 ActionMethod
都有实现，但**未经编译验证**。meow-rs 侧**唯一硬缺口是外置 fd TUN**（详见 `docs/CORE_MEOW_MAPPING.md`）：

- **外置 fd TUN（VpnService）**：meow-rs 的 `TunListener` 无 fd 入口，需上游加 `TunListenerConfig.device_fd`（见 `src/tun.rs`）
- sideLoadExternalProvider / updateGeoData、forceGC / resetTraffic / suspend：meow 无对应语义，暂为占位或经 meow-api 近似

已确认**不需要改上游**：Android `SocketProtector`（`src/protect.rs`）、HostResolver、日志广播（`src/logging.rs`）。