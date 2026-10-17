use eew_parser::parse_auto;
use std::{hint::black_box, time::Instant};

fn measure(name: &str, bytes: &[u8], json: bool) {
    let samples = if cfg!(debug_assertions) { 100 } else { 10000 };
    let warmup = if cfg!(debug_assertions) { 5 } else { 500 };
    for _ in 0..warmup {
        black_box(parse_auto(black_box(bytes)).unwrap());
    }
    let mut ns = Vec::with_capacity(samples);
    for _ in 0..samples {
        let start = Instant::now();
        let result = parse_auto(black_box(bytes)).unwrap();
        if json {
            black_box(serde_json::to_vec(&result).unwrap());
        }
        black_box(result);
        ns.push(start.elapsed().as_nanos());
    }
    ns.sort_unstable();
    println!(
        "{name},{},{},{},{}",
        bytes.len(),
        ns[samples / 2],
        ns[samples * 95 / 100],
        ns[samples * 99 / 100]
    );
}
fn main() {
    println!("case,bytes,p50_ns,p95_ns,p99_ns");
    measure(
        "short_telegram",
        include_bytes!("../tests/fixtures/expected/official_noto_first.telegram"),
        false,
    );
    measure("cancel_telegram", b"39 03 10 261016124850 C11 261016123810 ND20261016123810 NCN037 JD////////////// JN/// /// //// ///// /// // // RK///// RT///// RC///// 9999=", false);
    measure(
        "small_xml",
        include_bytes!("../tests/fixtures/official_100gal_first.xml"),
        false,
    );
    measure(
        "large_xml",
        include_bytes!("../tests/fixtures/official_noto_final.xml"),
        false,
    );
    measure(
        "105_regions_xml",
        include_bytes!("../tests/fixtures/synthetic/20261016123810/17-report.xml"),
        false,
    );
    measure(
        "105_regions_xml_json",
        include_bytes!("../tests/fixtures/synthetic/20261016123810/17-report.xml"),
        true,
    );
    measure(
        "large_xml_json",
        include_bytes!("../tests/fixtures/official_noto_final.xml"),
        true,
    );
}
