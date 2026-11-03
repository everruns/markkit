// Main content: keep only the article body of a web page.
//
// Navigation, headers, footers, scripts and forms are always stripped.
// `main_content(true)` goes further: it scores the remaining blocks and keeps
// the detected article, dropping sidebars, "related posts", comment threads
// and share bars. `base_url` resolves relative links and images, so the
// Markdown stays useful outside the original page.

use markkit::Markkit;

const PAGE: &str = r#"
<html><body>
  <nav><a href="/">Home</a> <a href="/blog">Blog</a></nav>
  <div class="layout">
    <aside class="sidebar">
      <h3>Popular posts</h3>
      <ul><li><a href="/p/1">Ten tips</a></li><li><a href="/p/2">Why Rust</a></li></ul>
    </aside>
    <article>
      <h1>Parsing untrusted documents</h1>
      <p>Every input is hostile until proven otherwise. Markkit bounds input size,
         output size, nesting depth, node counts and time for each call, and never
         executes scripts, macros or formulas.</p>
      <p>Read the <a href="../security/threat-model">threat model</a> for the full list
         of mitigations, each one backed by a test.</p>
      <img src="/img/pipeline.png" alt="Extraction pipeline">
    </article>
    <section class="comments"><h3>3 comments</h3><p>First!</p></section>
  </div>
  <footer>© Example Inc.</footer>
</body></html>
"#;

fn main() -> markkit::Result<()> {
    let everything = Markkit::builder().build().extract_html_sync(PAGE)?;
    println!("--- Default (boilerplate tags stripped) ---\n{everything}");

    let article = Markkit::builder()
        .main_content(true)
        .base_url("https://example.com/blog/posts/")
        .build()
        .extract_html_sync(PAGE)?;
    println!("--- main_content(true) + base_url ---\n{article}");

    // Explicit selectors win when you know the site's markup.
    let selected = Markkit::builder()
        .include_selector("article")
        .preserve_images(false)
        .build()
        .extract_html_sync(PAGE)?;
    println!("--- include_selector(\"article\"), no images ---\n{selected}");
    Ok(())
}
