// Custom selectors: include only specific elements and exclude unwanted ones.

use markkit::Markkit;

fn main() {
    let markkit = Markkit::builder()
        .include_selector("article")
        .exclude_selector(".sidebar")
        .build();

    let html = r#"
        <html>
        <body>
            <nav><a href="/">Home</a> | <a href="/about">About</a></nav>
            <article>
                <h1>Main Article</h1>
                <p>This is the primary content that should be extracted.</p>
                <div class="sidebar">
                    <p>Sidebar content that should be excluded.</p>
                </div>
                <p>More article content after the sidebar.</p>
            </article>
            <footer><p>Copyright 2026</p></footer>
        </body>
        </html>
    "#;

    let markdown = markkit.extract_html_sync(html).expect("extraction failed");

    println!("--- With include_selector('article') and exclude_selector('.sidebar') ---");
    println!("{markdown}");
    println!("Note: nav, footer, and sidebar content are excluded.");
}
