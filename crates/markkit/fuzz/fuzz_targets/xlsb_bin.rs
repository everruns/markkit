//! Structure-aware XLSB: fuzzed BIFF12 worksheet stream inside a valid workbook.
#![no_main]

use libfuzzer_sys::fuzz_target;
use markkit::DocumentHint;
use markkit_fuzz::common::{Sheet, rezip, xlsb};
use markkit_fuzz::{MAX_INPUT, extract_and_check};

fuzz_target!(|sheet_bin: &[u8]| {
    if sheet_bin.len() > MAX_INPUT {
        return;
    }
    let base = xlsb(&[Sheet::new("S", vec![])]);
    let path = "xl/worksheets/sheet1.bin";
    let bytes = rezip(&base, &[path], &[(path, sheet_bin)]);
    extract_and_check(&bytes, DocumentHint::file_name("fuzz.xlsb"));
});
