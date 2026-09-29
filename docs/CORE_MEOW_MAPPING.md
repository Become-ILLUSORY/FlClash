# CORE_MEOW_MAPPING —— FlClash ActionMethod ↔ meow-rs 实现状态

> 约定：迁移 = 本分支已完成代码实现；verify = 代码已写但需真机/CI 编译验证；
> gap = meow-rs 上游缺能力，需补丁或取舍。

| ActionMethod | FlClash 用途 | 实现 | 验证方式 | 状态 |
|---|---|---|---|---|
| initClash | 设 home dir / 初始化 | handlers::init_clash | CI | ✅迁移 |
| getIsInit | 是否已初始化 | state::is_initialized | — | ✅迁移 |
| forceGc | 触发 GC | no-op | — | ✅迁移(no-op) |
| shutdown | 关内核 | orchestrator::teardown | CI | ✅迁移 |
| setupConfig | 加载 home/config.yaml 并装配内核 | orchestrator::assemble | 真机 | 🔶verify |
| getConfig | 回读配置 | load_config_from_str→raw | 真机 | 🔶verify |
| updateConfig | 热改 mode/log-level 等 | tunnel.set_mode + log reload | 真机 | 🔶verify(部分) |
| validateConfig | 校验配置 | load_config_from_str(offline) | CI | ✅迁移 |
| getProxies | 代理列表 | meow-api /proxies + /group 组装 | 真机 | 🔶verify |
| changeProxy | 组切节点 | Tunnel selection set/force_set | 真机 | ✅迁移 |
| asyncTestDelay | 延迟测试 | meow-api /proxies/:name/delay | 真机 | ✅迁移 |
| getTraffic | 实时速率 | Statistics.traffic_snapshot | 真机 | ✅迁移 |
| getTotalTraffic | 累计流量 | Statistics.snapshot | 真机 | ✅迁移 |
| resetTraffic | 清零累计 | no-op（meow 无重置） | — | ✅迁移(no-op) |
| getConnections | 连接列表 | meow-api /connections | 真机 | ✅迁移 |
| closeConnections | 关全部连接 | DELETE /connections | 真机 | ✅迁移 |
| closeConnection | 关单连接 | DELETE /connections/:id | 真机 | ✅迁移 |
| resetConnections | 重置连接计数 | no-op | — | ✅迁移(no-op) |
| getExternalProviders | 外部 provider 列表 | meow-api /providers/proxies | 真机 | 🔶verify |
| getExternalProvider | 单 provider | /providers/proxies/:name | 真机 | 🔶verify |
| updateExternalProvider | 刷新 provider | PUT /providers/proxies/:name | 真机 | ✅迁移 |
| sideLoadExternalProvider | 本地导入 | **gap** (meow 无运行时 import) | — | ⛔gap |
| updateGeoData | 更新 geo 数据 | **gap**（启动自动拉取，无手动入口） | — | ⛔gap |
| getCountryCode | IP→国家 | geo.rs maxminddb | CI | ✅迁移 |
| startLog | 订阅日志 | Kernel.log_tx → event listener | 真机 | 🔶verify |
| stopLog | 停止日志 | no-op | — | ✅迁移(no-op) |
| startListener/stopListener | 监听启停 | assemble 内已随隧道启动 | — | 🔶verify(简) |
| updateDns | 运行时改 DNS | no-op（DNS 由 config 管） | — | ✅迁移(no-op) |
| getMemory | 内存占用 | meow-api /memory | 真机 | 🔶verify |
| crash | 崩溃测试 | panic | — | ✅迁移 |
| deleteFile | 删文件 | std::fs remove | CI | ✅迁移 |
| quickSetup | init+setup(Android) | C ABI quickSetup | 真机 | 🔶verify |
| startTUN | VpnService fd + protect (Android) | protect.rs 装 SocketProtector；fd 暂存(tun.rs) | 真机 | 🔶verify(protect 可用，TUN 数据面 gap) |
| stopTun | 关 TUN | protect::uninstall + teardown | 真机 | ✅迁移 |
| setEventListener | 事件回调 | callback::result_func | 真机 | ✅迁移 |
| suspend | 挂起 | no-op | — | ✅迁移(no-op) |

## 关键上游 gap（决定真机可用度）
1. **外置 fd TUN（唯一硬缺口）**：meow-rs `TunListener` 用 `tun_rs::DeviceBuilder` 自建设备
   （`crates/meow-listener/src/tun/mod.rs:424`），无 `from_fd`/外部 fd 入口；`tun.stack` 亦被忽略。
   需给上游加 `TunListenerConfig.device_fd`（见 `src/tun.rs`）。补丁前 Android 只能用「应用内代理」。
2. **sideLoadExternalProvider / updateGeoData**：meow-rs 有 `geodata_fetch`（启动时 run_on_startup +
   auto_update_loop）与 provider `refresh()`/后台 supervisor，但**没有 mihomo 那种“手动立刻更新/本地导入”**
   的运行时入口。可经 meow-api `/providers/*` 或 geodata_fetch 的循环触发，尚未全接。
3. **getProxies 的 `all` 组装**：meow-api `/group` 返回 `{proxies:{...}}`（mihomo 组视图）而非 `{groups:[...]}`，
   已改为取 key 作为组名列表，但需真机核对与 FlClash Dart 模型字段完全一致。
4. **无 forceGC / resetTraffic / suspend 对应语义**：均为调用占位（no-op）。

## 已确认可用的 meow-rs 钩子（不需要改上游）
- `SocketProtector`（Android protect 回调）：`meow-common/src/socket_protect.rs:208/221` → 见 `src/protect.rs`
- `HostResolver`：`meow_common::{set_host_resolver, clear_host_resolver}` + `meow_dns::ResolverHostHook`
- 日志：`meow_api::log_stream::{LogBroadcastLayer, install_log_reloader, reload_log_level}` → `src/logging.rs`
- 连接跟踪：`Statistics::{active_connections, close_connection, close_all_connections_counted}`