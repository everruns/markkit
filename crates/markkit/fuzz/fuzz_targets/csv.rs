//! Raw CSV / TSV input: dialect handling, row/column caps, table render.
#![no_main]

use libfuzzer_sys::fuzz_target;
use markkit::DocumentHint;
use markkit_fuzz::{MAX_INPUT, extract_and_check};

fuzz_target!(|data: &[u8]| {
    if data.len() > MAX_INPUT {
        return;
    }
    // First byte picks the dialect so both delimiters get coverage.
    let name = if data.first().is_some_and(|b| b & 1 == 1) {
        "fuzz.tsv"
    } else {
        "fuzz.csv"
    };
    extract_and_check(data, DocumentHint::file_name(name));
});
