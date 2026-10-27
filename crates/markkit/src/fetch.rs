//! Hardened HTTP fetch behind [`Markkit::extract_url`](crate::Markkit::extract_url)
//! (threat model T3).
//!
//! Decisions:
//! - Default-deny non-public destinations. Three layers: (1) URL check before
//!   any I/O (scheme, allowlist, IP literal, `localhost` names), (2) the same
//!   check on every redirect hop, (3) a DNS resolver that drops non-public
//!   addresses, so a public name resolving to 127.0.0.1 / 169.254.169.254
//!   (DNS rebinding) never connects. Layer 3 pins the addresses reqwest
//!   connects to, so there is no check-then-resolve gap.
//! - With an HTTP(S) proxy from the environment, the proxy resolves names;
//!   layers 1-2 still apply, layer 3 cannot. Documented in the threat model.
//! - Body bounded by `max_input_size` while streaming (declared
//!   `Content-Length` rejected up front); whole fetch bounded by
//!   `limits.timeout`.
//! - Policy and resolver run inside reqwest and can only return opaque
//!   errors, so they record the block reason in a per-call slot that is
//!   turned into [`Error::UrlBlocked`] afterwards.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use reqwest::Url;
use reqwest::dns::{Addrs, Name, Resolve, Resolving};

use crate::config::MarkkitBuilder;
use crate::error::Error;
use crate::limits::{LimitExceeded, LimitKind};

/// Fetched body plus its declared content type.
pub(crate) struct Fetched {
    pub bytes: Vec<u8>,
    pub content_type: Option<String>,
    /// Final URL after redirects.
    pub url: String,
}

/// Per-call slot for the first block reason seen inside reqwest.
type Blocked = Arc<Mutex<Option<String>>>;

fn record(slot: &Blocked, reason: String) -> String {
    let mut s = slot.lock().unwrap_or_else(|e| e.into_inner());
    s.get_or_insert(reason).clone()
}

pub(crate) async fn fetch(url: &str, cfg: &MarkkitBuilder) -> crate::Result<Fetched> {
    let parsed = Url::parse(url).map_err(|e| Error::UrlBlocked(format!("invalid URL: {e}")))?;
    check_url(&parsed, cfg).map_err(Error::UrlBlocked)?;

    let blocked: Blocked = Arc::default();
    let client = client(cfg, &blocked)?;
    let limit = cfg.limits.max_input_size;

    let run = async {
        let mut response = client.get(parsed).send().await?;
        if let Some(len) = response.content_length()
            && len > limit as u64
        {
            return Ok(Err(LimitExceeded::new(
                LimitKind::MaxInputSize,
                format!("response Content-Length {len} exceeds max_input_size {limit}"),
            )
            .into()));
        }
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(str::to_string);
        let final_url = response.url().to_string();
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await? {
            if bytes.len() + chunk.len() > limit {
                return Ok(Err(LimitExceeded::new(
                    LimitKind::MaxInputSize,
                    format!("response body exceeds max_input_size {limit}"),
                )
                .into()));
            }
            bytes.extend_from_slice(&chunk);
        }
        Ok(Ok(Fetched {
            bytes,
            content_type,
            url: final_url,
        }))
    };

    let result: Result<crate::Result<Fetched>, reqwest::Error> = run.await;
    match result {
        Ok(r) => r,
        Err(e) => {
            if let Some(reason) = blocked.lock().unwrap_or_else(|p| p.into_inner()).take() {
                return Err(Error::UrlBlocked(reason));
            }
            if e.is_timeout() {
                return Err(LimitExceeded::new(
                    LimitKind::Timeout,
                    format!("fetch exceeded timeout of {:?}", cfg.limits.timeout),
                )
                .into());
            }
            Err(Error::Http(e.to_string()))
        }
    }
}

fn client(cfg: &MarkkitBuilder, blocked: &Blocked) -> crate::Result<reqwest::Client> {
    let max_redirects = cfg.limits.max_redirects;
    let policy_cfg = cfg.clone();
    let policy_slot = blocked.clone();
    let policy = reqwest::redirect::Policy::custom(move |attempt| {
        if attempt.previous().len() > max_redirects {
            let reason = format!("too many redirects (max {max_redirects})");
            return attempt.error(record(&policy_slot, reason));
        }
        match check_url(attempt.url(), &policy_cfg) {
            Ok(()) => attempt.follow(),
            Err(reason) => {
                let reason = record(&policy_slot, format!("redirect: {reason}"));
                attempt.error(reason)
            }
        }
    });

    let timeout = cfg.limits.timeout;
    let mut builder = reqwest::Client::builder()
        .redirect(policy)
        .timeout(timeout)
        .connect_timeout(timeout.min(Duration::from_secs(10)));
    if !cfg.allow_private_networks {
        builder = builder.dns_resolver(Arc::new(PublicOnlyResolver {
            blocked: blocked.clone(),
        }));
    }
    builder
        .build()
        .map_err(|e| Error::Http(format!("client setup: {e}")))
}

/// Check scheme, allowlist and (unless private networks are allowed) IP
/// literals / `localhost` names. Pure: no DNS, no I/O.
fn check_url(url: &Url, cfg: &MarkkitBuilder) -> Result<(), String> {
    match url.scheme() {
        "http" | "https" => {}
        s => return Err(format!("scheme '{s}' not allowed (http/https only)")),
    }
    let host = url
        .host_str()
        .ok_or_else(|| "URL has no host".to_string())?;
    let host = normalize_host(host);

    if !cfg.allowed_hosts.is_empty() && !cfg.allowed_hosts.iter().any(|h| normalize_host(h) == host)
    {
        return Err(format!("host '{host}' not in allowed hosts"));
    }

    if cfg.allow_private_networks {
        return Ok(());
    }
    if host == "localhost" || host.ends_with(".localhost") {
        return Err(format!("host '{host}' is a loopback name"));
    }
    if let Ok(ip) = host.parse::<IpAddr>()
        && is_forbidden_ip(ip)
    {
        return Err(format!("address {ip} is not public"));
    }
    Ok(())
}

/// Lowercase, strip IPv6 brackets and a trailing root dot.
fn normalize_host(host: &str) -> String {
    let h = host.trim_start_matches('[').trim_end_matches(']');
    h.strip_suffix('.').unwrap_or(h).to_ascii_lowercase()
}

/// True for any address that is not globally routable unicast.
pub(crate) fn is_forbidden_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => is_forbidden_v4(v4),
        IpAddr::V6(v6) => is_forbidden_v6(v6),
    }
}

fn is_forbidden_v4(ip: Ipv4Addr) -> bool {
    let [a, b, c, _] = ip.octets();
    ip.is_unspecified()
        || ip.is_loopback()
        || ip.is_private()
        || ip.is_link_local()
        || ip.is_broadcast()
        || ip.is_documentation()
        || ip.is_multicast()
        || a == 0 // 0.0.0.0/8 "this network"
        || (a == 100 && (64..128).contains(&b)) // 100.64.0.0/10 CGNAT
        || (a == 192 && b == 0 && c == 0) // 192.0.0.0/24 IETF protocol
        || (a == 198 && (b == 18 || b == 19)) // 198.18.0.0/15 benchmarking
        || a >= 240 // 240.0.0.0/4 reserved
}

fn is_forbidden_v6(ip: Ipv6Addr) -> bool {
    let seg = ip.segments();
    // Addresses that embed an IPv4 address are judged by it.
    if let Some(v4) = ip.to_ipv4_mapped() {
        return is_forbidden_v4(v4);
    }
    let embedded = |hi: u16, lo: u16| Ipv4Addr::from(((hi as u32) << 16) | lo as u32);
    if seg[..6] == [0; 6] && !(ip.is_loopback() || ip.is_unspecified()) {
        return is_forbidden_v4(embedded(seg[6], seg[7])); // ::a.b.c.d (deprecated compat)
    }
    if seg[..6] == [0x64, 0xff9b, 0, 0, 0, 0] {
        return is_forbidden_v4(embedded(seg[6], seg[7])); // NAT64 64:ff9b::/96
    }
    if seg[0] == 0x2002 {
        return is_forbidden_v4(embedded(seg[1], seg[2])); // 6to4 2002::/16
    }
    ip.is_loopback()
        || ip.is_unspecified()
        || ip.is_multicast()
        || (seg[0] & 0xfe00) == 0xfc00 // fc00::/7 unique local
        || (seg[0] & 0xffc0) == 0xfe80 // fe80::/10 link-local
        || (seg[0] & 0xffc0) == 0xfec0 // fec0::/10 site-local (deprecated)
        || seg[0] == 0x2001 && seg[1] == 0x0db8 // 2001:db8::/32 documentation
}

/// Resolver that only hands reqwest public addresses.
struct PublicOnlyResolver {
    blocked: Blocked,
}

impl Resolve for PublicOnlyResolver {
    fn resolve(&self, name: Name) -> Resolving {
        let blocked = self.blocked.clone();
        let host = name.as_str().to_string();
        Box::pin(async move {
            let addrs: Vec<SocketAddr> =
                tokio::net::lookup_host((host.as_str(), 0)).await?.collect();
            let public: Vec<SocketAddr> = addrs
                .iter()
                .copied()
                .filter(|a| !is_forbidden_ip(a.ip()))
                .collect();
            if public.is_empty() {
                let reason = if addrs.is_empty() {
                    format!("host '{host}' did not resolve")
                } else {
                    format!("host '{host}' resolves only to non-public addresses")
                };
                return Err(record(&blocked, reason).into());
            }
            let addrs: Addrs = Box::new(public.into_iter());
            Ok(addrs)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn forbidden(s: &str) -> bool {
        is_forbidden_ip(s.parse().unwrap())
    }

    #[test]
    fn non_public_addresses_forbidden() {
        for ip in [
            "127.0.0.1",
            "10.1.2.3",
            "172.31.255.255",
            "192.168.0.1",
            "169.254.169.254",
            "100.64.0.1",
            "0.1.2.3",
            "192.0.0.1",
            "198.18.0.1",
            "224.0.0.1",
            "255.255.255.255",
            "240.0.0.1",
            "::1",
            "::",
            "::ffff:10.0.0.1",
            "::127.0.0.1",
            "64:ff9b::a9fe:a9fe", // NAT64 of 169.254.169.254
            "2002:7f00:1::",      // 6to4 of 127.0.0.1
            "fd12::1",
            "fe80::1",
            "ff02::1",
            "2001:db8::1",
        ] {
            assert!(forbidden(ip), "{ip} should be forbidden");
        }
    }

    #[test]
    fn public_addresses_allowed() {
        for ip in [
            "93.184.216.34",
            "8.8.8.8",
            "100.128.0.1",
            "172.32.0.1",
            "2606:4700::1111",
            "::ffff:8.8.8.8",
            "64:ff9b::808:808",
            "2002:808:808::",
        ] {
            assert!(!forbidden(ip), "{ip} should be allowed");
        }
    }

    fn check(url: &str, cfg: &MarkkitBuilder) -> Result<(), String> {
        check_url(&Url::parse(url).unwrap(), cfg)
    }

    #[test]
    fn check_url_default_policy() {
        let cfg = MarkkitBuilder::default();
        assert!(check("https://example.com/a", &cfg).is_ok());
        assert!(check("http://8.8.8.8/", &cfg).is_ok());
        assert!(check("http://localhost/", &cfg).is_err());
        assert!(check("http://a.localhost./", &cfg).is_err());
        assert!(check("http://[::1]:8080/", &cfg).is_err());
        assert!(check("data:text/html,hi", &cfg).is_err());
    }

    #[test]
    fn check_url_allowlist() {
        let cfg = MarkkitBuilder::default().allow_host("Example.COM.");
        assert!(check("https://example.com/", &cfg).is_ok());
        assert!(check("https://EXAMPLE.com./x", &cfg).is_ok());
        assert!(check("https://sub.example.com/", &cfg).is_err());
        assert!(check("https://example.com.evil.net/", &cfg).is_err());
    }

    #[test]
    fn check_url_private_opt_in_keeps_scheme_and_allowlist() {
        let cfg = MarkkitBuilder::default()
            .allow_private_networks(true)
            .allow_host("127.0.0.1");
        assert!(check("http://127.0.0.1:9/", &cfg).is_ok());
        assert!(check("http://10.0.0.1/", &cfg).is_err());
        assert!(check("file:///etc/passwd", &cfg).is_err());
    }
}
