// Resource limits and error handling.
//
// Every resource an input can drive is bounded by `markkit::Limits`
// (input/output bytes, time, nesting depth, node count, archive and
// spreadsheet sizes, PDF streams, redirects). Defaults are generous; a breach
// returns `Error::ResourceLimit` and `err.limit_kind()` names the limit, so a
// service can map it to "413 Payload Too Large" or a retry with a bigger cap.

use std::time::Duration;

use markkit::{Error, LimitKind, Limits, Markkit};

fn main() {
    // Tight limits for a multi-tenant service.
    let limits = Limits::default()
        .max_input_size(64 * 1024)
        .max_output_size(256)
        .max_nodes(1_000)
        .timeout(Duration::from_secs(5));
    let markkit = Markkit::builder().limits(limits).build();

    let small = "<h1>Fits</h1><p>Well under every limit.</p>";
    let big = format!(
        "<h1>Report</h1>{}",
        "<p>A paragraph of findings.</p>".repeat(50)
    );
    let noisy = format!("<ul>{}</ul>", "<li><b>x</b></li>".repeat(2_000));

    for (name, html) in [("small", small.to_string()), ("big", big), ("noisy", noisy)] {
        match markkit.extract_html_sync(&html) {
            Ok(md) => println!("{name}: ok, {} bytes of markdown", md.len()),
            Err(err @ Error::ResourceLimit(_)) => {
                let kind = err.limit_kind().expect("resource limit has a kind");
                let hint = match kind {
                    LimitKind::MaxInputSize => "reject upload (413)",
                    LimitKind::MaxOutputSize => "raise max_output_size or chunk the input",
                    LimitKind::MaxNodes | LimitKind::MaxNestingDepth => "likely hostile input",
                    LimitKind::Timeout => "retry off the hot path",
                    _ => "see Limits docs",
                };
                println!("{name}: limit {} hit ({err}); {hint}", kind.as_str());
            }
            Err(err) => println!("{name}: other error: {err}"),
        }
    }

    // Deep HTML does not fail by default: content past max_nesting_depth is
    // flattened, so the text survives without risking a stack overflow.
    let deep = format!(
        "{}still here{}",
        "<div>".repeat(5_000),
        "</div>".repeat(5_000)
    );
    let md = Markkit::builder()
        .build()
        .extract_html_sync(&deep)
        .expect("deep HTML is flattened, not rejected");
    println!("deep (default limits): {:?}", md.trim());
}
