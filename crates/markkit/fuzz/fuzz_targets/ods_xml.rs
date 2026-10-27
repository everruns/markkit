//! Structure-aware ODS: fuzzed `<office:spreadsheet>` content (row/column
//! repeats drive the grid pre-scan, T6b).
#![no_main]

use libfuzzer_sys::fuzz_target;
use markkit::DocumentHint;
use markkit_fuzz::common::ods;
use markkit_fuzz::{MAX_INPUT, extract_and_check};

fuzz_target!(|inner: &str| {
    if inner.len() > MAX_INPUT {
        return;
    }
    extract_and_check(&ods(inner), DocumentHint::file_name("fuzz.ods"));
});
