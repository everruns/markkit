//! Structure-aware XLS: fuzzed BIFF8 workbook stream inside a valid OLE2
//! container (DIMENSIONS / MULRK / BOUNDSHEET pre-scan, T6b-c).
#![no_main]

use libfuzzer_sys::fuzz_target;
use markkit::DocumentHint;
use markkit_fuzz::common::xls_from_stream;
use markkit_fuzz::{MAX_INPUT, extract_and_check};

fuzz_target!(|stream: &[u8]| {
    if stream.len() > MAX_INPUT {
        return;
    }
    extract_and_check(
        &xls_from_stream(stream),
        DocumentHint::file_name("fuzz.xls"),
    );
});
