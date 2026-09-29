//! countrycode：用 MaxMind GeoIP2 Country 数据库查 IP → 国家码。
//! 对应 Go 内核 `handleGetCountryCode`。

use std::net::IpAddr;
use std::path::PathBuf;

/// 打开 meow 默认 geoip 数据库并查国家码；查不到返回 None。
/// `geoip_path` 优先传 FlClash home 下已有的 mmdb 路径，缺省用 meow 默认路径。
pub fn lookup_country_code(ip: IpAddr, geoip_path: Option<PathBuf>) -> Option<String> {
    let path = geoip_path.or_else(|| Some(meow_config::default_geoip_path()))?;
    if !path.is_file() {
        return None;
    }
    let reader = maxminddb::Reader::open_readers(&[path]).ok()?;
    let country: maxminddb::geoip2::Country = reader.lookup(ip).ok()?;
    country
        .country
        .and_then(|c| c.iso_code)
        .map(|s| s.to_string())
}