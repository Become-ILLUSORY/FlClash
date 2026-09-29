//! 内核装配：把 meow-rs 的 Tunnel/DNS/监听器/API 串起来（移植 `meow-app/src/main.rs::run`）。
//!
//! FlClash 用的是「进程内库」模型：每次 `setupConfig` 就装配一整套运行时，
//! `shutdown`/`stopListener` 时拆掉。为少重复实现 mihomo 风格的 JSON 序列化，
//! 我们同时把 meow-rs 自带的 `meow-api`（REST）起在回环地址上，handle 层通过
//! 轻量 HTTP 客户端问它要 proxies / connections / traffic / memory 等快照。

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use parking_lot::RwLock;
use tokio::sync::broadcast;

use meow_api::log_stream::LogMessage;
use meow_config::proxy_provider::ProxyProvider;
use meow_config::rule_provider::RuleProvider;
use meow_tunnel::Tunnel;

/// 一次性启动的内核（所有运行句柄）。
pub struct Kernel {
    pub tunnel: Tunnel,
    /// meow-api 回环地址，供 handle 层做内部查询。
    pub api_addr: std::net::SocketAddr,
    pub api_secret: Option<String>,
    /// DNS server task（用于 stop 时回收）。
    pub dns_task: Option<tokio::task::JoinHandle<()>>,
    /// 订阅 refresh / geo 等后台任务句柄（stop 时 abort）。
    pub background: Vec<tokio::task::AbortHandle>,
    /// log 广播发送端，供 `startLog` 订阅转发到 event listener。
    pub log_tx: broadcast::Sender<LogMessage>,
    pub log_rx: Option<broadcast::Receiver<LogMessage>>,
}

/// 把一个 config（已经 `load_config_from_str` 得到）装配成可运行内核。
///
/// `home_dir`：FlClash 的 home 目录（geo 数据、配置持久化都在这下面）。
/// `api_port`：给 meow-api 用的回环端口（0 = 随机）。
pub async fn assemble(
    config: meow_config::Config,
    home_dir: &str,
    api_port: u16,
) -> Result<Kernel, String> {
    // 与 meow main.rs 相同的宿主 resolver 钩子：确保代理服务器域名解析
    // 走配置里的 DNS，而不是 libc（Android 上避免绕过 VpnService.protect）。
    const VPN_PLATFORM: bool = cfg!(any(target_os = "android", target_os = "ios"));
    if config.dns.enabled || VPN_PLATFORM {
        meow_common::set_host_resolver(Arc::new(
            meow_dns::ResolverHostHook::new_with_proxy_resolver(
                Arc::clone(&config.dns.resolver),
                config.dns.proxy_resolver.clone(),
            ),
        ));
    } else {
        meow_common::clear_host_resolver();
    }

    // 核心路由引擎
    let tunnel = Tunnel::new_with_slot(config.dns.resolver_slot.clone());
    tunnel.set_dialer_registry(config.provider_dialer_registry.clone());
    tunnel.set_mode(config.general.mode);
    tunnel.update_routing(
        config.proxies,
        config.rules,
        config.dialer_registry,
    );
    tunnel.spawn_background_tasks();
    tunnel.reconcile_health_checks(&meow_config::extract_health_check_specs(
        config.raw.proxy_groups.as_deref().unwrap_or(&[]),
    ));

    // 后台任务收集
    let mut background: Vec<tokio::task::AbortHandle> = Vec::new();

    // DNS server（可选）
    let dns_server_handle: Arc<RwLock<Option<meow_api::routes::DnsServerHandle>>> =
        Arc::new(RwLock::new(None));
    if let Some(listen_addr) = config.dns.listen_addr {
        let dns_server = meow_dns::DnsServer::new(
            Arc::clone(&config.dns.resolver),
            listen_addr,
        );
        let slot = dns_server.resolver_slot();
        let bound = dns_server
            .bind()
            .await
            .map_err(|e| format!("dns.listen {listen_addr}: {e}"))?;
        let task = tokio::spawn(async move {
            if let Err(e) = bound.run().await {
                tracing::error!("DNS server error: {}", e);
            }
        });
        background.push(task.abort_handle());
        *dns_server_handle.write() = Some(meow_api::routes::DnsServerHandle {
            listen: listen_addr,
            task,
            resolver_slot: slot,
        });
    }

    // 监听器（mirror main.rs 的 listeners: 遍历）。这里先只处理 Mixed/HTTP/Socks5。
    let mut named_listeners = config.listeners.named.clone();
    for nl in &mut named_listeners {
        use meow_config::ListenerSpec;
        let addr = match format!("{}:{}", nl.listen, nl.port).parse::<std::net::SocketAddr>() {
            Ok(a) => a,
            Err(_) => continue,
        };
        match &nl.spec {
            ListenerSpec::Mixed | ListenerSpec::Http | ListenerSpec::Socks5 => {
                let socket = match tokio::net::TcpListener::bind(addr).await {
                    Ok(s) => s,
                    Err(e) => {
                        tracing::error!("listener '{}': bind {} failed: {}", nl.name, addr, e);
                        continue;
                    }
                };
                let bound = socket.local_addr().unwrap_or(addr);
                nl.port = bound.port();
                let listener = meow_listener::MixedListener::new(
                    tunnel.clone(),
                    bound,
                    nl.name.clone(),
                )
                .with_max_connections(nl.max_connections);
                background.push(tokio::spawn(async move {
                    if let Err(e) = listener.run_on(socket).await {
                        tracing::error!("Listener error: {}", e);
                    }
                })
                .abort_handle());
            }
            _ => {}
        }
    }

    // meow-api 回环 REST（供 handle 层查询 proxies/connections/traffic/memory）
    let api_addr: std::net::SocketAddr =
        format!("127.0.0.1:{api_port}").parse().unwrap_or_else(|_| {
            std::net::SocketAddr::from((std::net::Ipv4Addr::LOCALHOST, 0))
        });
    let api_secret = config.api.secret.clone();

    if let Some(addr) = Some(api_addr) {
        let proxy_providers: Arc<dashmap::DashMap<String, Arc<ProxyProvider>>> = {
            let m = dashmap::DashMap::new();
            for (name, p) in config.proxy_providers.iter() {
                m.insert(name.clone(), Arc::clone(p));
            }
            Arc::new(m)
        };
        let rule_providers: Arc<RwLock<HashMap<String, Arc<RuleProvider>>>> =
            Arc::new(RwLock::new(config.rule_providers.clone()));

        let (log_tx, _) = broadcast::channel(256);
        let api = meow_api::ApiServer::new(
            tunnel.clone(),
            addr,
            api_secret.clone(),
            format!("{home_dir}/config.yaml"),
            Arc::new(RwLock::new(config.raw.clone())),
            log_tx.clone(),
            proxy_providers,
            rule_providers,
            Arc::new(meow_config::rule_provider_refresh::RefreshSupervisor::default()),
            Arc::new(
                meow_config::proxy_provider_refresh::ProxyProviderRefreshSupervisor::default(),
            ),
            named_listeners.clone(),
            None, // external_ui
            Arc::clone(&dns_server_handle),
            config.provider_dialer_registry.clone(),
        );
        let socket = tokio::net::TcpListener::bind(addr)
            .await
            .map_err(|e| format!("external-controller {addr}: {e}"))?;
        background.push(tokio::spawn(async move {
            if let Err(e) = api.run_on(socket).await {
                tracing::error!("API server error: {}", e);
            }
        })
        .abort_handle());

        return Ok(Kernel {
            tunnel,
            api_addr: addr,
            api_secret,
            dns_task: None,
            background,
            log_tx,
            log_rx: None,
        });
    }

    Err("no api address".into())
}

/// 关闭内核。
pub async fn teardown(kernel: &mut Kernel) {
    kernel.tunnel.stop_tun().await;
    for t in kernel.background.drain(..) {
        t.abort();
    }
    // 宿主 resolver 钩子清掉（重新装配前不残留）
    meow_common::clear_host_resolver();
}