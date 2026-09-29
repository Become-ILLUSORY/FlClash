# FlClash 内核迁移：Clash.Meta(Go) → meow-rs(Rust)

## 目标

在本仓库分支 `feat/meow-rs-core` 上，把 FlClash 当前基于 **mihomo/Clash.Meta（Go, cgo, FFI）** 的内核，
替换为 **meow-rs（Rust 实现的 mihomo 兼容内核）**。

迁移的**接口契约保持 100% 不变**：`libclash.so` 导出的 C 符号、`com.follow.clash.core` 的 JNI 层、
以及 `core/*.go` 实现的 JSON Action 协议（`{id,method,data}` → `{id,method,data,code}`）原样保留。
因此 **Flutter 前端（lib/）与 Android JNI 层（android/core）无需任何改动**，只需替换内核 `.so`。

## 现状（基线）

- 内核源码：`core/`（Go, `go build -buildmode=c-shared` → `libclash.so` + 自动生成 `libclash.h`）
  - 子模块：`core/Clash.Meta`（`myflavor/Clash.Meta` 分支 `FlClash`，Go 内核）
- 构建入口：`setup.dart buildCore()`（`GOOS/GOARCH/CGO_ENABLED/CC` 环境变量 → `go build`）
- 产物落位：`libclash/<target>/<archName>/libclash.{so|dll}` → `android/core/src/main/jniLibs/<abi>/`
- Android JNI：`android/core/src/main/cpp/core.cpp`（`Java_com_follow_clash_core_Core_*`，链接 `libclash`）
- 动作协议：见 `core/action.go` / `core/hub.go` / `core/constant.go`（全部 ActionMethod 清单在 `constant.go`）

## 迁移目标架构（本分支要落成的）

```
core/meow-rs/            ← 新增 git 子模块，固定 meow-rs 仓库 commit
core/meow-ffi/           ← 新增 Rust cdylib crate（产物名 clash → libclash.so）
                            ├─ lib.rs        # extern "C" 导出：与 libclash.so 完全一致的符号
                            ├─ tls.rs        # 对 socks5/tls/binlog 的 socket protect 钩子（Android VPN）
                            ├─ tun.rs        # startTUN(fd) → 用外部 fd 喂给 meow 的 TunListener
                            ├─ action.rs     # Action/ActionResult/message JSON 协议类型
                            ├─ handlers.rs   # 把每个 ActionMethod 映射到 meow-rs API
                            ├─ state.rs      # 全局 kernel 状态（config/tunnel/listeners/dns/api）
                            └─ jni_bridge.rs # bride.c 等价物：result/release/protect/resolve_process 回调
```

关键：`meow-ffi` 以 `path = "../meow-rs/crates/..."` 直接依赖 meow-rs 工作区 crate
（`meow-config` / `meow-tunnel` / `meow-listener` / `meow-dns` / `meow-api` / `meow-proxy` / `meow-common`），
**复用其内核逻辑，而不是进程外调用**，这样 TUN fd、protect 回调都能进程内接管，与现 FlClash 架构等价。

## Action 协议映射（FlClash ↔ meow-rs）

| FlClash ActionMethod | meow-rs 对应实现 | 状态 |
|---|---|---|
| initClash | state::init（写 homeDir） | 迁移 |
| getIsInit | 全局 bool | 迁移 |
| forceGc | `std::alloc` / 空实现提示 | 迁移(no-op) |
| shutdown | 关 Tunnel/listeners/api | 迁移 |
| validateConfig | meow_config::load_config_from_str 试解析 | 迁移 |
| updateConfig | apply UpdateParams → 更新 runtime 配置 | 迁移 |
| setupConfig | 应用 setup → 启动内核（Tunnel+listeners+DNS+api） | 迁移 |
| getConfig | meow_config 序列化回 YAML/JSON | 迁移 |
| getProxies | meow-api 的 proxies 结构（Tunnel 的快照） | 迁移 |
| changeProxy | Tunnel 内 Selector 切换 | 迁移 |
| getTraffic / getTotalTraffic | meow_tunnel::Statistics | 迁移 |
| resetTraffic | Statistics.reset | 迁移 |
| asyncTestDelay | meow_tunnel::health_check | 迁移 |
| getConnections / close* / reset* | meow-api connections 快照 + Tunnel | 迁移 |
| getExternalProviders / getExternalProvider / update* / sideLoad* | meow-config::proxy_provider | 迁移 |
| updateGeoData | meow-app geodata_fetch / 内置 mmdb | 部分迁移 |
| getCountryCode | maxminddb（meow-api 依赖） | 迁移 |
| getMemory | meow-api 的 sysinfo | 迁移 |
| startLog / stopLog | meow-api::log_stream / tracing | 迁移 |
| startListener / stopListener | meow-api / Tunnel 启停 | 迁移 |
| updateDns / suspend / crash / deleteFile | 实现 | 迁移 |
| quickSetup / startTUN / stopTun / setEventListener | JNI/C ABI 直出 | 迁移 |

> 完整字段级对照见后续 `CORE_MEOW_MAPPING.md`（迁移时逐条填写，并在测试后勾选）。

## 与 meow-rs 现有钩子的衔接（已确认）

- **Android VpnService protect（宿主回调）**：meow-rs **已内置** `SocketProtector` 钩子
  （`meow-common/src/socket_protect.rs:208/221`，`cfg(target_os="android")`）：宿主实现
  `SocketProtector::protect(fd)`（内部调 `VpnService.protect`）后 `set_socket_protector(...)`，
  之后 meow 所有经 `connect_tcp*` / `bind_udp` 的出站 socket 都会先被 protect。
  → 我们把它桥接到 JNI 的 `protect_func`（`src/protect.rs`），**无需上游改动**。
- **HostResolver 钩子**：`meow_common::{set_host_resolver, clear_host_resolver}` +
  `meow_dns::ResolverHostHook`（main.rs 同款），代理服务器域名解析走配置 DNS。
- **日志订阅**：`meow_api::log_stream::{LogBroadcastLayer, install_log_reloader, reload_log_level}`，
  我们装 tracing 层并把广播转发到 FlClash 事件回调（`src/logging.rs`）。
- **外部控制面**：meow-api 路由与 mihomo 基本同构（`/proxies` `/group` `/connections`
  `/configs` `/traffic` `/logs` `/memory` `/providers/*`），所以我们只在回环地址起一个
  ApiServer 供 handle 层查询，避免重复实现 mihomo 风格 JSON。

## 唯一决定性的上游缺口：外置 fd TUN

meow-rs 的 `TunListener` 用 `tun_rs::DeviceBuilder` **自建设备**
（`crates/meow-listener/src/tun/mod.rs:424`），**没有 `from_fd` / 外部 fd 入口**，
且 `tun.stack` 字段被 `parse_tun_config` 忽略（只支持 lwIP）。
Android 的设备必须由 `VpnService` 建好、把 fd 传进来 —— 需要给上游加
`TunListenerConfig.device_fd: Option<RawFd>`（见 `core/meow-ffi/src/tun.rs`）。
补丁合入前，Android 只能「应用内代理」：mixed 监听 + `SocketProtector`，无系统 VPN 的 TUN 数据面。

## 里程碑

1. [x] fork `myflavor/FlClash` → `Become-ILLUSORY/FlClash`
2. [x] 建分支 `feat/meow-rs-core`
3. [ ] `core/meow-ffi` cdylib：全部 ActionMethod 映射 + 事件流 + protect/resolve_process 回调
4. [ ] `core/meow-rs` 子模块固定 + `path` 依赖对接
5. [ ] `setup.dart` 内核构建切换到 cargo（保留 go 分支以便回退）
6. [ ] CI 增加 Android arm64 .so 构建流水线（cargo-ndk）
7. [ ] 真机联调：TUN、订阅、GEO、延迟、连接、流量、日志

## 风险与取舍

- meow-rs 内核成熟度/API 稳定性低于 mihomo；`meow-*` crate 是 0.21.2 工作区，非独立发布版本，
  因此用子模块固定 commit 是稳妥做法，升级即改 commit。
- `libclash.h` 由 cgo 自动生成，迁移后改为手工维护的头文件（导出项不变）。
- 迁移阶段 Go 内核 `core/` 保留（可回退）；`feat/meow-rs-core` 是唯一迁移线。