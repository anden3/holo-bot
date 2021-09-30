use std::io::Read;

use commands::pekofy_text;
use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};

pub fn criterion_benchmark(c: &mut Criterion) {
    let mut file = std::fs::File::open("/home/andre/downloads/wiki_plain.txt").unwrap();
    let mut buffer = [0; 1024 * 1024];

    file.read_exact(&mut buffer).unwrap();
    let contents = std::str::from_utf8(&buffer).unwrap();

    c.bench_with_input(
        BenchmarkId::new("pekofy", "1 MB Wikipedia"),
        &contents,
        |b, s| b.iter(|| pekofy_text(s)),
    );
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
