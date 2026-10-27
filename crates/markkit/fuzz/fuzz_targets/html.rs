//! HTML → Markdown: no panic, output passes the S17 oracle.
#![no_main]

use libfuzzer_sys::fuzz_target;
use markkit_fuzz::{MAX_INPUT, check_markdown, markkit};

fuzz_target!(|data: &[u8]| {
    if data.len() > MAX_INPUT {
        return;
    }
    let html = String::from_utf8_lossy(data);
    if let Ok(md) = markkit().extract_html_sync(&html) {
        check_markdown(&md);
    }
});
