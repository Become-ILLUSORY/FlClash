//! 极简回环 HTTP 客户端：handle 层通过它向进程内 meow-api 查询
//! proxies / connections / traffic 等 mihomo 风格快照。
//!
//! 所有调用都发生在 `state::get_runtime()` 的 `block_on` 内，因此用 async reqwest
//! （避免 blocking client 与 tokio 复用线程池导致的死锁）。

use std::time::Duration;

/// GET `{base}{path}`，带 `Authorization: Bearer <secret>`（secret 为空则不带）。
pub async fn api_get(base: &str, secret: &Option<String>, path: &str) -> Result<String, String> {
    let url = format!("{base}{path}");
    request("GET", &url, secret, None).await
}

/// 方法版（支持 POST/PUT/DELETE）。
pub async fn request(
    method: &str,
    url: &str,
    secret: &Option<String>,
    body: Option<&str>,
) -> Result<String, String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| e.to_string())?;
    let mut req = client
        .request(
            method.parse::<reqwest::Method>().map_err(|e| e.to_string())?,
            url,
        );
    if let Some(s) = secret {
        if !s.is_empty() {
            req = req.bearer_auth(s);
        }
    }
    if let Some(b) = body {
        req = req.header("Content-Type", "application/json").body(b.to_string());
    }
    let resp = req.send().await.map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        let text = resp.text().await.unwrap_or_default();
        return Err(format!("http {}: {url}: {text}", url));
    }
    resp.text().await.map_err(|e| e.to_string())
}