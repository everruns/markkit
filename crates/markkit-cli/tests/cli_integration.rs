use std::io::Write;
use std::process::Command;

fn markkit_bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_markkit"))
}

#[test]
fn help_flag_works() {
    let output = markkit_bin()
        .arg("--help")
        .output()
        .expect("failed to run markkit");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Extract clean markdown"));
}

#[test]
fn extract_help_works() {
    let output = markkit_bin()
        .args(["extract", "--help"])
        .output()
        .expect("failed to run markkit");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("--stdin"));
}

#[test]
fn extract_from_stdin_flag() {
    let mut child = markkit_bin()
        .args(["extract", "--stdin"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("failed to spawn markkit");

    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(b"<h1>Hello World</h1><p>Some text here.</p>")
        .unwrap();

    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Hello World"), "output: {stdout}");
    assert!(stdout.contains("Some text here"), "output: {stdout}");
}

#[test]
fn extract_from_stdin_dash() {
    let mut child = markkit_bin()
        .args(["extract", "-"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("failed to spawn markkit");

    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(b"<h1>Dash Test</h1>")
        .unwrap();

    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Dash Test"), "output: {stdout}");
}

#[test]
fn extract_from_file() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("test.html");
    std::fs::write(&file_path, "<h1>File Test</h1><p>Paragraph.</p>").unwrap();

    let output = markkit_bin()
        .args(["extract", "--file", file_path.to_str().unwrap()])
        .output()
        .expect("failed to run markkit");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("File Test"), "output: {stdout}");
    assert!(stdout.contains("Paragraph"), "output: {stdout}");
}

#[test]
fn extract_text_and_markdown_files() {
    let dir = tempfile::tempdir().unwrap();
    let cases = [
        (
            "notes.txt",
            "line one\nline two\n",
            "line one  \nline two\n",
        ),
        (
            "doc.md",
            "Title\n=====\n\n* a\n\n<script>x()</script>\n",
            "# Title\n\n- a\n",
        ),
        (
            "data.csv",
            "a;b\n1;2\n",
            "| a | b |\n| --- | --- |\n| 1 | 2 |\n",
        ),
    ];
    for (name, input, expected) in cases {
        let file_path = dir.path().join(name);
        std::fs::write(&file_path, input).unwrap();
        let output = markkit_bin()
            .args(["extract", "--file", file_path.to_str().unwrap()])
            .output()
            .expect("failed to run markkit");
        assert!(
            output.status.success(),
            "{name} stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(String::from_utf8_lossy(&output.stdout), expected, "{name}");
    }
}

#[test]
fn extract_json_output() {
    let mut child = markkit_bin()
        .args(["extract", "--stdin", "--json"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("failed to spawn markkit");

    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(b"<h1>JSON Test</h1>")
        .unwrap();

    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(&stdout).expect("invalid JSON output");
    assert_eq!(parsed["source"], "<stdin>");
    assert!(parsed["markdown"].as_str().unwrap().contains("JSON Test"));
}

#[test]
fn extract_with_include_selector() {
    let mut child = markkit_bin()
        .args(["extract", "--stdin", "-i", "article"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("failed to spawn markkit");

    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(b"<div>Ignore this</div><article><p>Keep this</p></article>")
        .unwrap();

    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Keep this"), "output: {stdout}");
}

#[test]
fn extract_with_exclude_selector() {
    let mut child = markkit_bin()
        .args(["extract", "--stdin", "-e", "nav"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("failed to spawn markkit");

    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(b"<nav>Menu</nav><p>Content</p>")
        .unwrap();

    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Content"), "output: {stdout}");
    assert!(
        !stdout.contains("Menu"),
        "nav should be excluded, output: {stdout}"
    );
}

#[test]
fn missing_source_errors() {
    let output = markkit_bin()
        .args(["extract"])
        .output()
        .expect("failed to run markkit");

    assert!(!output.status.success());
}

#[test]
fn limit_flags_enforced() {
    let mut child = markkit_bin()
        .args(["extract", "--stdin", "--max-output-size", "5"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("failed to spawn markkit");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"<p>hello world, this is long</p>")
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("max_output_size"), "stderr: {stderr}");
}

#[test]
fn extract_with_rules_file() {
    let dir = tempfile::tempdir().unwrap();
    let page = dir.path().join("page.html");
    std::fs::write(
        &page,
        r#"<div class="note">Careful</div><div class="ad">Buy</div>"#,
    )
    .unwrap();
    let rules = dir.path().join("rules.json");
    std::fs::write(
        &rules,
        r#"[{"selector": ".note", "action": "block_quote", "title": "Note"},
            {"selector": ".ad", "action": "skip"}]"#,
    )
    .unwrap();

    let output = markkit_bin()
        .args(["extract", "--file", page.to_str().unwrap()])
        .args(["--rules", rules.to_str().unwrap()])
        .output()
        .expect("failed to run markkit");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim_end(),
        "> **Note**\n>\n> Careful"
    );
}

#[test]
fn extract_with_bad_rules_file_fails() {
    let dir = tempfile::tempdir().unwrap();
    let rules = dir.path().join("rules.json");
    std::fs::write(&rules, r#"[{"selector": "p", "action": "exec"}]"#).unwrap();
    let output = markkit_bin()
        .args(["extract", "--stdin", "--rules", rules.to_str().unwrap()])
        .output()
        .expect("failed to run markkit");
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("invalid rules file"));
}
