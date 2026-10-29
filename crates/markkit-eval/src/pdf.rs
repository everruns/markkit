//! PDF text, layout and table eval with generated, inspectable content streams.
//!
//! Decision: expected coordinates and text are authored in the dataset, not
//! computed by the parser. Both modes measure ideal text fidelity; `layout`
//! additionally measures geometry. Column/table cases check spatial reconstruction
//! and table structure. Missing/failed conversions score zero, never N/A.

use std::time::Instant;

use markkit::{DocumentHint, Markkit};
use mira::subject::subject_fn;
use mira::{Eval, Sample, Score, Transcript};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Deserialize, Serialize)]
struct Fixture {
    id: String,
    category: String,
    pages: Vec<Page>,
    markdown: String,
    layout: Value,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
struct Page {
    attrs: String,
    content: String,
}

const DATA: &str = include_str!("../data/pdf-layout.json");

fn fixtures() -> Vec<Fixture> {
    serde_json::from_str(DATA).expect("tested embedded PDF dataset")
}

// Minimal valid PDF writer, independent of hayro/markkit internals. Helvetica
// omits widths intentionally: its documented 0.5 em fallback makes scenes easy
// to inspect. Each page may override inherited MediaBox / Rotate / CropBox.
fn pdf(f: &Fixture) -> Vec<u8> {
    let kids: Vec<_> = (0..f.pages.len())
        .map(|i| format!("{} 0 R", 4 + i * 2))
        .collect();
    let mut objects = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        format!(
            "<< /Type /Pages /Kids [{}] /Count {} /MediaBox [0 0 612 792] >>",
            kids.join(" "),
            f.pages.len()
        ),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string(),
    ];
    for (i, p) in f.pages.iter().enumerate() {
        objects.push(format!("<< /Type /Page /Parent 2 0 R {} /Resources << /Font << /F1 3 0 R >> >> /Contents {} 0 R >>", p.attrs, 5 + i * 2));
        objects.push(format!(
            "<< /Length {} >>\nstream\n{}\nendstream",
            p.content.len(),
            p.content
        ));
    }
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = vec![0];
    for (i, o) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend(format!("{} 0 obj\n{o}\nendobj\n", i + 1).as_bytes());
    }
    let xref = out.len();
    out.extend(format!("xref\n0 {}\n0000000000 65535 f \n", offsets.len()).as_bytes());
    for offset in &offsets[1..] {
        out.extend(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend(
        format!(
            "trailer << /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            offsets.len()
        )
        .as_bytes(),
    );
    out
}

/// Match expected geometry fields with a 0.001 point tolerance. Array lengths
/// must match, so extra/missing pages or text runs cannot improve the score.
fn matches(expected: &Value, actual: &Value) -> bool {
    match (expected, actual) {
        (Value::Object(e), Value::Object(a)) => e
            .iter()
            .all(|(k, v)| a.get(k).is_some_and(|a| matches(v, a))),
        (Value::Array(e), Value::Array(a)) => {
            e.len() == a.len() && e.iter().zip(a).all(|(e, a)| matches(e, a))
        }
        (Value::Number(e), Value::Number(a)) => e
            .as_f64()
            .zip(a.as_f64())
            .is_some_and(|(e, a)| (e - a).abs() < 0.001),
        _ => expected == actual,
    }
}

fn transcript(f: &Fixture, layout: bool) -> Transcript {
    let bytes = pdf(f);
    let start = Instant::now();
    let result = Markkit::builder()
        .pdf_layout(layout)
        .build()
        .extract_document_sync(&bytes, DocumentHint::default());
    let elapsed = start.elapsed();
    let e = match result {
        Ok(e) => e,
        Err(e) => return Transcript::failed(format!("PDF extraction failed: {e}")),
    };
    let mut t = Transcript::response(serde_json::to_string(&e).expect("serializable extraction"));
    t.record_metric("extract_us", elapsed.as_micros() as f64);
    t.record_metric("input_bytes", bytes.len() as f64);
    t.record_metric("output_bytes", t.final_response.len() as f64);
    t.record_metric("layout_requested", if layout { 1.0 } else { 0.0 });
    t
}

fn score(name: &str, sample: &Sample, t: &Transcript) -> Score {
    if t.errored_infra() {
        return Score::na(name, "subject infra error");
    }
    let fixture = fixtures()
        .into_iter()
        .find(|f| f.id == sample.id)
        .expect("sample in embedded dataset");
    let actual: Value = serde_json::from_str(&t.final_response).unwrap_or(Value::Null);
    if name == "layout_geometry"
        && t.error.is_none()
        && actual["markdown"].is_string()
        && t.metrics.get("layout_requested") == Some(&0.0)
    {
        return Score::na(name, "layout capture disabled in default mode");
    }
    let passed = match name {
        "text_fidelity" => actual["markdown"].as_str() == Some(fixture.markdown.as_str()),
        "layout_geometry" => matches(&fixture.layout, &actual["layout"]),
        "markdown_parity" => {
            let plain = transcript(&fixture, false);
            let plain: Value = serde_json::from_str(&plain.final_response).unwrap_or(Value::Null);
            actual["markdown"].is_string() && actual["markdown"] == plain["markdown"]
        }
        _ => false,
    };
    Score {
        scorer: name.into(),
        value: if passed { 1.0 } else { 0.0 },
        pass: passed,
        na: false,
        reason: if passed {
            "matched authored expectation".into()
        } else {
            format!("{name} expectation failed for {}", fixture.id)
        },
    }
}

/// Offline Mira study: compare default extraction with optional layout capture.
pub fn pdf_eval() -> Eval {
    let mut b = Eval::new("markkit_pdf")
        .describe("PDF text fidelity, positioned geometry, table structure and Markdown parity")
        .subject(subject_fn(|sample: Sample, cx| async move {
            let mode = cx.param("mode").unwrap_or("default");
            if !["default", "layout"].contains(&mode) {
                return Transcript::infra_error("unknown PDF mode");
            }
            let Some(f) = fixtures().into_iter().find(|f| f.id == sample.id) else {
                return Transcript::infra_error("missing PDF fixture");
            };
            transcript(&f, mode == "layout")
        }))
        .axis("mode", ["default", "layout"])
        .target(crate::mira_study::markkit_target())
        .max_turns(1);
    for name in ["text_fidelity", "layout_geometry", "markdown_parity"] {
        b = b.scorer(mira::scorer::scorer(
            name,
            move |s: &Sample, t: &Transcript| score(name, s, t),
        ));
    }
    for f in fixtures() {
        b = b.add_sample(
            Sample::new(&f.id, format!("PDF {} scene", f.category))
                .tag("pdf")
                .tag(&f.category)
                .file("scene.json", serde_json::to_string_pretty(&f).unwrap()),
        );
    }
    b.build()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fixtures_are_valid_and_cover_geometry_and_known_gaps() {
        let fs = fixtures();
        assert!(fs.len() >= 10);
        let ids: std::collections::HashSet<_> = fs.iter().map(|f| &f.id).collect();
        assert_eq!(ids.len(), fs.len());
        for category in ["rotation", "crop", "spacing", "columns", "table", "empty"] {
            assert!(
                fs.iter().any(|f| f.category == category),
                "missing {category}"
            );
        }
        let _ = pdf_eval();
        for f in fs {
            let t = transcript(&f, true);
            assert!(t.error.is_none(), "{}: {:?}", f.id, t.error);
            let actual: Value = serde_json::from_str(&t.final_response).unwrap();
            assert!(
                matches(&f.layout, &actual["layout"]),
                "{} geometry mismatch: {}",
                f.id,
                actual["layout"]
            );
        }
    }
    #[test]
    fn geometry_scorer_rejects_missing_extra_wrong_and_non_numeric_values() {
        let expected =
            serde_json::json!({"pages":[{"width":612.0,"spans":[{"text":"Hi","bbox":[1,2,3,4]}]}]});
        assert!(matches(&expected, &expected));
        assert!(!matches(&expected, &Value::Null));
        for patch in [
            serde_json::json!(null),
            serde_json::json!("612"),
            serde_json::json!(613),
        ] {
            let mut actual = expected.clone();
            actual["pages"][0]["width"] = patch;
            assert!(!matches(&expected, &actual));
        }
        let mut actual = expected.clone();
        actual["pages"][0]["spans"] = serde_json::json!([]);
        assert!(!matches(&expected, &actual));
        let mut actual = expected.clone();
        actual["pages"].as_array_mut().unwrap().push(Value::Null);
        assert!(!matches(&expected, &actual));
    }
    #[test]
    fn extraction_failure_is_scored_zero() {
        let sample = Sample::new(&fixtures()[0].id, "failure");
        for name in ["text_fidelity", "layout_geometry", "markdown_parity"] {
            let s = score(name, &sample, &Transcript::failed("malformed"));
            assert!(!s.na && !s.pass && s.value == 0.0);
        }
    }

    #[test]
    fn disabled_layout_is_na_but_missing_requested_geometry_scores_zero() {
        let f = &fixtures()[0];
        let sample = Sample::new(&f.id, "optional geometry");
        assert!(score("layout_geometry", &sample, &transcript(f, false)).na);
        let mut requested = transcript(f, true);
        let mut actual: Value = serde_json::from_str(&requested.final_response).unwrap();
        actual.as_object_mut().unwrap().remove("layout");
        requested.final_response = actual.to_string();
        let s = score("layout_geometry", &sample, &requested);
        assert!(!s.na && !s.pass && s.value == 0.0);
    }
}
