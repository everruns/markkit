//! Security tests for `extract_url` (threat model T3 SSRF, T2/T4 via remote bodies).
//!
//! A raw TCP server on loopback stands in for the remote host. Loopback is
//! itself a forbidden destination by default, which the first tests assert;
//! the rest opt in with `allow_private_networks(true)` to exercise limits.

#![cfg(feature = "http_client")]

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use markkit::Markkit;
use markkit::error::Error;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// Serve `respond(path)` to every connection; returns base URL and a hit counter.
async fn serve<F>(respond: F) -> (String, Arc<AtomicUsize>)
where
    F: Fn(&str) -> Vec<u8> + Send + Sync + 'static,
{
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let hits = Arc::new(AtomicUsize::new(0));
    let counter = hits.clone();
    let respond = Arc::new(respond);
    tokio::spawn(async move {
        loop {
            let Ok((mut sock, _)) = listener.accept().await else {
                return;
            };
            counter.fetch_add(1, Ordering::SeqCst);
            let respond = respond.clone();
            tokio::spawn(async move {
                let mut buf = vec![0u8; 4096];
                let n = sock.read(&mut buf).await.unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]);
                let path = req.split_whitespace().nth(1).unwrap_or("/").to_string();
                let _ = sock.write_all(&respond(&path)).await;
                let _ = sock.shutdown().await;
            });
        }
    });
    (format!("http://{addr}"), hits)
}

fn html(body: &str) -> Vec<u8> {
    format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
    .into_bytes()
}

fn redirect(to: &str) -> Vec<u8> {
    format!(
        "HTTP/1.1 302 Found\r\nLocation: {to}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
    )
    .into_bytes()
}

fn private_ok() -> Markkit {
    Markkit::builder().allow_private_networks(true).build()
}

fn assert_blocked(r: markkit::Result<String>) {
    match r {
        Err(Error::UrlBlocked(_)) => {}
        other => panic!("expected UrlBlocked, got {other:?}"),
    }
}

// ─── Destination checks (default policy) ─────────────────────────────

#[tokio::test]
async fn loopback_ip_blocked_by_default_without_connecting() {
    let (base, hits) = serve(|_| html("<p>secret</p>")).await;
    assert_blocked(Markkit::builder().build().extract_url(&base).await);
    assert_eq!(
        hits.load(Ordering::SeqCst),
        0,
        "no request may reach the host"
    );
}

#[tokio::test]
async fn localhost_name_blocked_by_default_without_connecting() {
    let (base, hits) = serve(|_| html("<p>secret</p>")).await;
    let url = base.replace("127.0.0.1", "localhost");
    assert_blocked(Markkit::builder().build().extract_url(&url).await);
    assert_eq!(hits.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn forbidden_ip_literals_blocked() {
    let m = Markkit::builder().build();
    for url in [
        "http://169.254.169.254/latest/meta-data/",
        "http://10.0.0.1/",
        "http://192.168.1.1/",
        "http://172.16.0.1/",
        "http://100.64.0.1/",
        "http://0.0.0.0/",
        "http://[::1]/",
        "http://[::ffff:127.0.0.1]/",
        "http://[fd00::1]/",
        "http://[fe80::1]/",
        "http://2130706433/", // 127.0.0.1 as a decimal integer
        "http://0x7f.0.0.1/",
    ] {
        assert_blocked(m.extract_url(url).await);
    }
}

#[tokio::test]
async fn non_http_schemes_blocked() {
    let m = private_ok();
    for url in [
        "file:///etc/passwd",
        "ftp://example.com/",
        "gopher://127.0.0.1/",
    ] {
        assert_blocked(m.extract_url(url).await);
    }
}

#[tokio::test]
async fn invalid_url_is_error_not_panic() {
    assert!(
        Markkit::builder()
            .build()
            .extract_url("not a url")
            .await
            .is_err()
    );
}

// ─── Opt-in private access ──────────────────────────────────────────

#[tokio::test]
async fn private_networks_reachable_when_allowed() {
    let (base, _) = serve(|_| html("<h1>Hi</h1>")).await;
    assert_eq!(private_ok().extract_url(&base).await.unwrap(), "# Hi\n");
}

// ─── Host allowlist ─────────────────────────────────────────────────

#[tokio::test]
async fn host_outside_allowlist_blocked() {
    let (base, hits) = serve(|_| html("<p>x</p>")).await;
    let m = Markkit::builder()
        .allow_private_networks(true)
        .allow_host("example.com")
        .build();
    assert_blocked(m.extract_url(&base).await);
    assert_eq!(hits.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn redirect_to_host_outside_allowlist_blocked() {
    let (base, _) = serve(|path| {
        if path == "/" {
            redirect("http://localhost:9/") // different host than 127.0.0.1
        } else {
            html("<p>x</p>")
        }
    })
    .await;
    let m = Markkit::builder()
        .allow_private_networks(true)
        .allow_host("127.0.0.1")
        .build();
    assert_blocked(m.extract_url(&base).await);
}

#[tokio::test]
async fn allowlisted_host_matches_case_insensitively() {
    let (base, _) = serve(|_| html("<p>ok</p>")).await;
    let url = base.replace("127.0.0.1", "LOCALHOST");
    let m = Markkit::builder()
        .allow_private_networks(true)
        .allow_host("localhost")
        .build();
    assert_eq!(m.extract_url(&url).await.unwrap(), "ok\n");
}

// ─── Redirects ──────────────────────────────────────────────────────

#[tokio::test]
async fn redirect_loop_stops_at_limit() {
    let (base, hits) = serve(|_| redirect("/again")).await;
    let m = Markkit::builder()
        .allow_private_networks(true)
        .max_redirects(2)
        .build();
    assert!(m.extract_url(&base).await.is_err());
    assert!(hits.load(Ordering::SeqCst) <= 3, "followed past the limit");
}

#[tokio::test]
async fn redirect_to_non_http_scheme_blocked() {
    let (base, _) = serve(|_| redirect("file:///etc/passwd")).await;
    assert!(private_ok().extract_url(&base).await.is_err());
}

// ─── Body size and time limits ──────────────────────────────────────

#[tokio::test]
async fn declared_oversized_body_rejected() {
    let (base, _) = serve(|_| {
        b"HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: 100000000\r\nConnection: close\r\n\r\n<p>".to_vec()
    })
    .await;
    let m = Markkit::builder()
        .allow_private_networks(true)
        .max_input_size(1024)
        .build();
    match m.extract_url(&base).await {
        Err(Error::ResourceLimit(_)) => {}
        other => panic!("expected ResourceLimit, got {other:?}"),
    }
}

#[tokio::test]
async fn undeclared_oversized_body_rejected_while_streaming() {
    let (base, _) = serve(|_| {
        let mut r =
            b"HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nConnection: close\r\n\r\n".to_vec();
        r.extend(std::iter::repeat_n(b'a', 64 * 1024));
        r
    })
    .await;
    let m = Markkit::builder()
        .allow_private_networks(true)
        .max_input_size(1024)
        .build();
    match m.extract_url(&base).await {
        Err(Error::ResourceLimit(_)) => {}
        other => panic!("expected ResourceLimit, got {other:?}"),
    }
}

#[tokio::test]
async fn stalled_server_times_out() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let (_sock, _) = listener.accept().await.unwrap();
        tokio::time::sleep(Duration::from_secs(30)).await; // never answer
    });
    let m = Markkit::builder()
        .allow_private_networks(true)
        .max_processing_secs(1)
        .build();
    let start = Instant::now();
    assert!(m.extract_url(&format!("http://{addr}/")).await.is_err());
    assert!(
        start.elapsed() < Duration::from_secs(10),
        "timeout not enforced"
    );
}
