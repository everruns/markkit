// Configuration: demonstrate how various options affect extraction output.

use markkit::Markkit;

fn main() {
    let html = r#"
        <h1>Config Demo</h1>
        <p>Visit <a href="https://example.com">Example</a> for details.</p>
        <p>Here is an image: <img src="logo.png" alt="Logo" /></p>
        <p>Final paragraph.</p>
    "#;

    // Default: links and images preserved
    let default_mk = Markkit::builder().build();
    let out = default_mk.extract_html_sync(html).expect("failed");
    println!("--- Default (links + images ON) ---");
    println!("{out}");

    // Disable links
    let no_links = Markkit::builder().preserve_links(false).build();
    let out = no_links.extract_html_sync(html).expect("failed");
    println!("--- preserve_links(false) ---");
    println!("{out}");

    // Disable images
    let no_images = Markkit::builder().preserve_images(false).build();
    let out = no_images.extract_html_sync(html).expect("failed");
    println!("--- preserve_images(false) ---");
    println!("{out}");

    // Small max_input_size to trigger resource limit
    let tiny = Markkit::builder().max_input_size(10).build();
    match tiny.extract_html_sync(html) {
        Ok(_) => println!("Unexpected success"),
        Err(e) => println!("--- max_input_size(10) ---\nError: {e}"),
    }
}
