//! Provider-decoded mail: show a concise preview while keeping quoted context.
use markkit::{
    Limits,
    email::{EmailBody, extract_body},
};

fn main() -> markkit::Result<()> {
    let view = extract_body(
        EmailBody {
            text: Some("Yes, please.\n\nOn Monday Alice wrote:\n> Should we reschedule?\n"),
            html: None,
        },
        Limits::default().max_input_size(64 * 1024),
    )?;
    assert_eq!(view.clean, "Yes, please.");
    println!("Clean: {}", view.clean);
    for omitted in &view.omitted {
        println!(
            "Preserved {}: {}",
            omitted.reason,
            &view.full[omitted.start..omitted.end]
        );
    }
    Ok(())
}
