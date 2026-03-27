// Basic markkit usage: extract markdown from HTML with default settings.

use markkit::Markkit;

fn main() {
    let markkit = Markkit::builder().build();

    let html = r#"
        <html>
        <body>
            <h1>Welcome to Markkit</h1>
            <p>This is a <strong>simple</strong> example of HTML to markdown conversion.</p>
            <ul>
                <li>Fast extraction</li>
                <li>Clean output</li>
                <li>Configurable</li>
            </ul>
            <p>Visit <a href="https://example.com">our site</a> for more info.</p>
        </body>
        </html>
    "#;

    let markdown = markkit.extract_html_sync(html).expect("extraction failed");

    println!("--- Extracted Markdown ---");
    println!("{markdown}");
}
