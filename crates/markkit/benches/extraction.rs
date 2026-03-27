use criterion::{Criterion, criterion_group, criterion_main};

fn bench_format_detection(c: &mut Criterion) {
    c.bench_function("format_from_content_type", |b| {
        b.iter(|| {
            markkit::format::Format::from_content_type("text/html; charset=utf-8");
        });
    });

    c.bench_function("format_from_extension", |b| {
        b.iter(|| {
            markkit::format::Format::from_extension("html");
        });
    });
}

criterion_group!(benches, bench_format_detection);
criterion_main!(benches);
