//! `markkit_cli::run` with a custom `CliHost`: the embedding contract.
//!
//! Proves an embedder (e.g. the bashkit `markkit` builtin) gets the same CLI as
//! the binary, with stdio, file reads, URL fetching and refused flags routed
//! through the host. No test here touches the network or the real filesystem
//! through the host.

use markkit::{DocumentHint, Extraction, Markkit};
use markkit_cli::{CliHost, HostFeature, run};
use std::collections::HashMap;
use std::future::Future;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;

#[derive(Default)]
struct TestHost {
    stdout: Mutex<String>,
    stderr: Mutex<String>,
    stdin: Vec<u8>,
    files: HashMap<PathBuf, Vec<u8>>,
    /// url => (final url, content type, body)
    pages: HashMap<String, (String, String, Vec<u8>)>,
    fetched: Mutex<Vec<String>>,
    refused: Vec<HostFeature>,
    broken_pipe: bool,
}

impl TestHost {
    fn out(&self) -> String {
        self.stdout.lock().unwrap().clone()
    }
    fn err(&self) -> String {
        self.stderr.lock().unwrap().clone()
    }
}

impl CliHost for TestHost {
    fn write_stdout(&self, text: &str) -> io::Result<()> {
        if self.broken_pipe {
            return Err(io::Error::from(io::ErrorKind::BrokenPipe));
        }
        self.stdout.lock().unwrap().push_str(text);
        Ok(())
    }
    fn write_stderr(&self, text: &str) {
        self.stderr.lock().unwrap().push_str(text);
    }
    fn read_stdin(&self) -> io::Result<Vec<u8>> {
        Ok(self.stdin.clone())
    }
    fn read_file(&self, path: &Path) -> io::Result<Vec<u8>> {
        self.files
            .get(path)
            .cloned()
            .ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))
    }
    fn extract_url(
        &self,
        markkit: &Markkit,
        url: &str,
    ) -> impl Future<Output = markkit::Result<Extraction>> + Send {
        self.fetched.lock().unwrap().push(url.to_string());
        let page = self.pages.get(url).cloned();
        let mut config = markkit.config().clone();
        async move {
            let Some((final_url, content_type, body)) = page else {
                return Err(markkit::Error::UnsupportedFormat("no such page".into()));
            };
            config.base_url = Some(final_url);
            Markkit::new(config).extract_document_sync(
                &body,
                DocumentHint {
                    content_type: Some(&content_type),
                    file_name: None,
                },
            )
        }
    }
    fn refuse(&self, feature: HostFeature) -> Option<String> {
        self.refused
            .contains(&feature)
            .then(|| "is not available in this environment".to_string())
    }
}

async fn run_with(host: &TestHost, args: &[&str]) -> i32 {
    run(std::iter::once("markkit").chain(args.iter().copied()), host).await
}

fn binary(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_markkit"))
        .args(args)
        .output()
        .expect("run markkit binary")
}

const PAGE: &[u8] = b"<html><head><title>T</title></head><body><h1>Hello</h1><p><a href=\"/next\">next</a></p></body></html>";

#[tokio::test]
async fn help_and_version_match_binary_byte_for_byte() {
    for args in [
        &["--help"][..],
        &["extract", "--help"],
        &["mcp", "--help"],
        &["--version"],
    ] {
        let host = TestHost::default();
        assert_eq!(run_with(&host, args).await, 0, "{args:?}");
        let bin = binary(args);
        assert_eq!(bin.status.code(), Some(0), "{args:?}");
        assert_eq!(host.out(), String::from_utf8_lossy(&bin.stdout), "{args:?}");
        assert!(host.err().is_empty());
    }
}

#[tokio::test]
async fn usage_errors_match_binary_and_exit_2() {
    for args in [
        &["extract", "--no-such-flag", "x"][..],
        &[],
        &["extract", "--layout", "x"],
    ] {
        let host = TestHost::default();
        assert_eq!(run_with(&host, args).await, 2, "{args:?}");
        let bin = binary(args);
        assert_eq!(bin.status.code(), Some(2), "{args:?}");
        assert_eq!(host.err(), String::from_utf8_lossy(&bin.stderr), "{args:?}");
    }
}

#[tokio::test]
async fn runtime_error_matches_binary_and_exits_1() {
    let host = TestHost::default();
    assert_eq!(run_with(&host, &["extract"]).await, 1);
    let bin = binary(&["extract"]);
    assert_eq!(bin.status.code(), Some(1));
    assert_eq!(host.err(), String::from_utf8_lossy(&bin.stderr));
}

#[tokio::test]
async fn stdin_goes_through_host() {
    for args in [&["extract", "--stdin"][..], &["extract", "-"]] {
        let host = TestHost {
            stdin: PAGE.to_vec(),
            ..Default::default()
        };
        assert_eq!(run_with(&host, args).await, 0, "{}", host.err());
        assert!(host.out().contains("# Hello"), "{}", host.out());
    }
}

#[tokio::test]
async fn file_and_rules_read_through_host() {
    let mut files = HashMap::new();
    files.insert(PathBuf::from("/vfs/doc.html"), PAGE.to_vec());
    files.insert(
        PathBuf::from("/vfs/rules.json"),
        br#"[{"selector": "h1", "action": "skip"}]"#.to_vec(),
    );
    let host = TestHost {
        files,
        ..Default::default()
    };
    let code = run_with(
        &host,
        &[
            "extract",
            "--file",
            "/vfs/doc.html",
            "--rules",
            "/vfs/rules.json",
        ],
    )
    .await;
    assert_eq!(code, 0, "{}", host.err());
    assert!(!host.out().contains("Hello"), "{}", host.out());
    assert!(host.out().contains("next"), "{}", host.out());
}

#[tokio::test]
async fn missing_file_is_error_exit_1() {
    let host = TestHost::default();
    assert_eq!(
        run_with(&host, &["extract", "--file", "/nope.html"]).await,
        1
    );
    assert!(host.err().starts_with("Error: "), "{}", host.err());
    assert!(host.out().is_empty());
}

#[tokio::test]
async fn invalid_utf8_rules_is_error() {
    let mut files = HashMap::new();
    files.insert(PathBuf::from("/r.json"), vec![0xff, 0xfe]);
    let host = TestHost {
        files,
        stdin: PAGE.to_vec(),
        ..Default::default()
    };
    assert_eq!(
        run_with(&host, &["extract", "-", "--rules", "/r.json"]).await,
        1
    );
    assert!(host.err().contains("valid UTF-8"), "{}", host.err());
}

#[tokio::test]
async fn url_goes_through_host_and_resolves_against_final_url() {
    let mut pages = HashMap::new();
    pages.insert(
        "https://a.test/start".to_string(),
        (
            "https://b.test/final/".to_string(),
            "text/html".to_string(),
            PAGE.to_vec(),
        ),
    );
    let host = TestHost {
        pages,
        ..Default::default()
    };
    assert_eq!(
        run_with(&host, &["extract", "https://a.test/start", "--json"]).await,
        0,
        "{}",
        host.err()
    );
    let json: serde_json::Value = serde_json::from_str(&host.out()).unwrap();
    assert_eq!(json["source"], "https://a.test/start");
    let md = json["markdown"].as_str().unwrap();
    assert!(md.contains("https://b.test/next"), "{md}");
    assert_eq!(*host.fetched.lock().unwrap(), ["https://a.test/start"]);
}

#[tokio::test]
async fn refused_ocr_fails_before_reading_input() {
    let host = TestHost {
        refused: vec![HostFeature::Ocr],
        ..Default::default()
    };
    assert_eq!(
        run_with(&host, &["extract", "https://a.test/x.pdf", "--ocr"]).await,
        1
    );
    assert_eq!(
        host.err(),
        "Error: --ocr is not available in this environment\n"
    );
    assert!(host.fetched.lock().unwrap().is_empty());
}

#[tokio::test]
async fn mcp_is_refused_by_default_host() {
    let host = TestHost::default();
    assert_eq!(run_with(&host, &["mcp"]).await, 1);
    assert_eq!(
        host.err(),
        "Error: mcp is not available in this environment\n"
    );
}

#[tokio::test]
async fn broken_pipe_on_stdout_exits_0() {
    let host = TestHost {
        stdin: PAGE.to_vec(),
        broken_pipe: true,
        ..Default::default()
    };
    assert_eq!(run_with(&host, &["extract", "-"]).await, 0);
    assert!(host.err().is_empty());
}
