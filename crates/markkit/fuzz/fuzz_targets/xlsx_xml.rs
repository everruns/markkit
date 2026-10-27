//! Structure-aware XLSX: fuzzed worksheet XML inside a valid workbook.
#![no_main]

use libfuzzer_sys::fuzz_target;
use markkit::DocumentHint;
use markkit_fuzz::common::{Sheet, xlsx_with};
use markkit_fuzz::{MAX_INPUT, extract_and_check};

fuzz_target!(|sheet_xml: &str| {
    if sheet_xml.len() > MAX_INPUT {
        return;
    }
    let bytes = xlsx_with(
        &[Sheet::new("S", vec![])],
        &[Some(sheet_xml.to_string())],
        &[],
    );
    extract_and_check(&bytes, DocumentHint::file_name("fuzz.xlsx"));
});
