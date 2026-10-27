//! Raw Markdown input: re-parse through the IR and render back.
#![no_main]

use libfuzzer_sys::fuzz_target;
use markkit::DocumentHint;
use markkit_fuzz::{MAX_INPUT, extract_and_check};

fuzz_target!(|data: &[u8]| {
    if data.len() > MAX_INPUT {
        return;
    }
    extract_and_check(data, DocumentHint::file_name("fuzz.md"));
});
