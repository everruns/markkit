//! Raw PDF bytes: pre-scan, hayro object parsing, text-layer extraction.
#![no_main]

use libfuzzer_sys::fuzz_target;
use markkit::DocumentHint;
use markkit_fuzz::{MAX_INPUT, extract_and_check};

fuzz_target!(|data: &[u8]| {
    if data.len() > MAX_INPUT {
        return;
    }
    extract_and_check(data, DocumentHint::file_name("fuzz.pdf"));
});
