// markkit-eval: mira eval study host.
//
// Advertises markkit's evals to the `mira` CLI over stdio:
//
//     mira list --study-bin markkit-eval
//     mira run --study-bin markkit-eval markkit_extract
//     mira run --study-bin markkit-eval markkit_extract --tag news --format html --out report.html
//
// The `#[eval]` wrappers live in the bin crate so their inventory registrations
// are guaranteed to link into this binary; the work is in
// `markkit_eval::mira_study`. See knowledge/operations/eval.md.

use markkit_eval::mira_study;
use mira::{Eval, eval};

#[eval]
fn markkit_extract() -> Eval {
    mira_study::extract_eval()
}

#[eval]
fn markkit_pdf() -> Eval {
    markkit_eval::pdf::pdf_eval()
}

#[eval]
fn markkit_qa() -> Eval {
    markkit_eval::qa::qa_eval()
}

fn main() -> std::io::Result<()> {
    mira::Study::registered().serve_blocking()
}
