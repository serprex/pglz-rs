use std::hint::black_box;
use std::time::{Duration, Instant};

use pglz::{Strategy, compress, decompress, decompress_into, decompress_with_capacity};

const TARGET_BYTES: usize = 128 * 1024 * 1024;
const SAMPLES: usize = 7;

struct Case {
    name: &'static str,
    compressed: Vec<u8>,
    raw_len: usize,
}

struct Measurement {
    name: &'static str,
    ns_per_byte: f64,
    mib_per_sec: f64,
}

fn main() {
    let cases = [
        all_literal_case(),
        repeated_byte_case(),
        repeated_word_case(),
        repeated_text_case(),
    ];

    println!("case,raw,compressed,method,ns_per_byte,mib_per_sec");
    for case in cases {
        let iterations = (TARGET_BYTES / case.raw_len).max(16);
        let measurements = [
            bench_owned(&case, iterations, "owned", decompress),
            bench_owned(&case, iterations, "capacity", |source| {
                decompress_with_capacity(source, case.raw_len)
            }),
            bench_into(&case, iterations, "into", |source, dest| {
                decompress_into(source, dest, true)
            }),
        ];

        for measurement in measurements {
            println!(
                "{},{},{},{},{:.3},{:.1}",
                case.name,
                case.raw_len,
                case.compressed.len(),
                measurement.name,
                measurement.ns_per_byte,
                measurement.mib_per_sec
            );
        }
    }
}

fn all_literal_case() -> Case {
    let raw_len: usize = 1 << 20;
    let mut compressed = Vec::with_capacity(raw_len + raw_len.div_ceil(8));
    for chunk in (0..raw_len).step_by(8) {
        compressed.push(0);
        for i in chunk..(chunk + 8).min(raw_len) {
            compressed.push((i as u8).wrapping_mul(31).wrapping_add(17));
        }
    }
    Case {
        name: "literal_1m",
        compressed,
        raw_len,
    }
}

fn repeated_byte_case() -> Case {
    compressed_case("repeat_byte_1m", vec![b'a'; 1 << 20])
}

fn repeated_word_case() -> Case {
    compressed_case("repeat_word_1m", b"abcd".repeat(1 << 18))
}

fn repeated_text_case() -> Case {
    let text = b"Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor incididunt ut labore et dolore magna aliqua. ";
    compressed_case("repeat_text_1m", text.repeat((1 << 20) / text.len() + 1))
}

fn compressed_case(name: &'static str, mut raw: Vec<u8>) -> Case {
    raw.truncate(1 << 20);
    let compressed = compress(&raw, &Strategy::ALWAYS).expect("compress");
    Case {
        name,
        compressed,
        raw_len: raw.len(),
    }
}

fn bench_owned<F>(case: &Case, iterations: usize, name: &'static str, f: F) -> Measurement
where
    F: Fn(&[u8]) -> Option<Vec<u8>>,
{
    let mut samples = Vec::with_capacity(SAMPLES);
    for _ in 0..SAMPLES {
        let elapsed = time(|| {
            for _ in 0..iterations {
                let out = f(black_box(&case.compressed)).expect("decompress");
                assert_eq!(out.len(), case.raw_len);
                black_box(out);
            }
        });
        samples.push(elapsed);
    }
    measurement(name, median(samples), iterations, case.raw_len)
}

fn bench_into<F>(case: &Case, iterations: usize, name: &'static str, f: F) -> Measurement
where
    F: Fn(&[u8], &mut [u8]) -> Option<usize>,
{
    let mut samples = Vec::with_capacity(SAMPLES);
    for _ in 0..SAMPLES {
        let mut out = vec![0u8; case.raw_len];
        let elapsed = time(|| {
            for _ in 0..iterations {
                let n = f(black_box(&case.compressed), black_box(&mut out)).expect("decompress");
                assert_eq!(n, case.raw_len);
                black_box(&out);
            }
        });
        samples.push(elapsed);
    }
    measurement(name, median(samples), iterations, case.raw_len)
}

fn time(f: impl FnOnce()) -> Duration {
    let start = Instant::now();
    f();
    start.elapsed()
}

fn median(mut samples: Vec<Duration>) -> Duration {
    samples.sort_unstable();
    samples[samples.len() / 2]
}

fn measurement(
    name: &'static str,
    elapsed: Duration,
    iterations: usize,
    raw_len: usize,
) -> Measurement {
    let bytes = (iterations * raw_len) as f64;
    let secs = elapsed.as_secs_f64();
    Measurement {
        name,
        ns_per_byte: secs * 1_000_000_000.0 / bytes,
        mib_per_sec: bytes / secs / 1024.0 / 1024.0,
    }
}
