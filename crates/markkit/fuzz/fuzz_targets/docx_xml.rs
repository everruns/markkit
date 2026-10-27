//! Structure-aware DOCX: fuzzed XML inside a valid package, so the input
//! reaches the WordprocessingML walker instead of failing at the ZIP header.
#![no_main]

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;
use markkit::DocumentHint;
use markkit_fuzz::common::{DocxParts, docx_with};
use markkit_fuzz::{MAX_INPUT, extract_and_check};

#[derive(Arbitrary, Debug)]
struct Input<'a> {
    body: &'a str,
    rels: &'a str,
    styles: Option<&'a str>,
    numbering: Option<&'a str>,
}

fuzz_target!(|input: Input<'_>| {
    let size = input.body.len() + input.rels.len();
    if size > MAX_INPUT {
        return;
    }
    let bytes = docx_with(DocxParts {
        body: input.body,
        rels: input.rels,
        styles: input.styles,
        numbering: input.numbering,
        ..Default::default()
    });
    extract_and_check(&bytes, DocumentHint::file_name("fuzz.docx"));
});
