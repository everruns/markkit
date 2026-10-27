// Transforms: rewrite the IR between parsing and rendering.

use markkit::Markkit;
use markkit::ir::Node;
use markkit::transform::{self, NormalizeHeadings};

fn main() {
    let html = r#"
        <h3>Release notes</h3>
        <p>See <a href="https://example.com/changes">the changes</a>.</p>
        <h4>Fixes</h4>
        <ul><li>Tables keep row headers</li></ul>
    "#;

    let markkit = Markkit::builder()
        // Built-in: shift headings so the shallowest is `#`.
        .transform(NormalizeHeadings)
        // Custom: replace links with their text.
        .transform(transform::from_fn("unlink", |doc| {
            doc.visit_mut(&mut |node| {
                if let Node::Link { children, .. } = node {
                    *node = Node::Italic {
                        children: std::mem::take(children),
                    };
                }
            });
            Ok(())
        }))
        .build();

    println!("Passes: {:?}\n", markkit.config().transforms);
    let md = markkit.extract_html_sync(html).expect("extract failed");
    println!("{md}");
}
