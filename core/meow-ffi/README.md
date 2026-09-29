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
- `src/callback.rs`     —— 供 core.cpp JNI_OnLoad 注入的函数指针

## 依赖
- `core/meow-rs` git 子模块（meow-rs 工作区 crate，path 依赖，固定 commit）
- cargo features：`full-proxy`（ss/trojan/vless/vmess/hysteria2/anytls/dns/监听/http/socks5/mixed/tun）

## 构建（Android）
见根目录 `scripts/build-meow-core.sh`；CI 见 `.github/workflows/meow-core.yml`。

## 状态
这是迁移的基础脚手架：协议、C ABI、TTunnel/DNS/API 装配、绝大部分 ActionMethod 都有实现，
但**未经编译验证**，且以下点依赖 meow-rs 上游能力（详见 `docs/CORE_MEOW_MAPPING.md`）：
- 外置 fd TUN（VpnService）—— meow-rs 上游需支持 `TunListenerConfig.device_fd`
- sideLoadExternalProvider / updateGeoData —— meow 侧尚无与 mihomo 一致的运行时手动更新入口
- resetTraffic / updateDns / suspend —— meow 无同类语义，暂为调用占位