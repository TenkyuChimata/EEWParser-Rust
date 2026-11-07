//! This integration-test binary deliberately has one test: allocation counters
//! cover only one parser operation at a time, independent of wall-clock timing.
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

struct CountingAllocator;
static TOTAL_REQUESTED: AtomicUsize = AtomicUsize::new(0);
static MAX_REQUESTED: AtomicUsize = AtomicUsize::new(0);

// SAFETY: all allocations and deallocations are forwarded unchanged to System.
// The counters never inspect or alter pointers and do not allocate themselves.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        record(layout.size());
        unsafe { System.alloc(layout) }
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        record(layout.size());
        unsafe { System.alloc_zeroed(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        record(size);
        unsafe { System.realloc(ptr, layout, size) }
    }
}
fn record(size: usize) {
    TOTAL_REQUESTED.fetch_add(size, Ordering::Relaxed);
    MAX_REQUESTED.fetch_max(size, Ordering::Relaxed);
}
#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

fn measure<T>(operation: impl FnOnce() -> T) -> (T, usize, usize) {
    TOTAL_REQUESTED.store(0, Ordering::Relaxed);
    MAX_REQUESTED.store(0, Ordering::Relaxed);
    let result = operation();
    let total = TOTAL_REQUESTED.load(Ordering::Relaxed);
    let maximum = MAX_REQUESTED.load(Ordering::Relaxed);
    (result, total, maximum)
}

#[test]
fn xml_memory_budget_and_adjacent_text_growth_are_bounded() {
    let base = include_str!("fixtures/official_noto_first.xml");
    let fragmented = |count| {
        base.replace(
            "</Body>",
            &format!("<Text>{}</Text></Body>", "<![CDATA[x]]>".repeat(count)),
        )
    };
    let small = fragmented(5_000);
    let large = fragmented(10_000);
    let (small_result, small_total, _) = measure(|| eew_parser::parse_xml(small.as_bytes()));
    let (large_result, large_total, _) = measure(|| eew_parser::parse_xml(large.as_bytes()));
    assert_eq!(small_result.unwrap().body.text, Some("x".repeat(5_000)));
    assert_eq!(large_result.unwrap().body.text, Some("x".repeat(10_000)));
    // roxmltree 0.20 requested 4x as much allocation for 2x as many CDATA
    // sections. Allow ample overhead while guarding against that regression.
    assert!(
        large_total < small_total * 3,
        "allocation growth: {small_total} -> {large_total}"
    );

    let oversized_estimate = base.replace(
        "</Body>",
        &format!("<Text>{}</Text></Body>", "=".repeat(250_001)),
    );
    let (result, total, maximum) = measure(|| eew_parser::parse_xml(oversized_estimate.as_bytes()));
    assert!(
        matches!(result, Err(eew_parser::EewError::InvalidXml(message)) if message.contains("allocation markers"))
    );
    // Without the lexical budget this small XML asked the DOM to reserve
    // approximately 18 MB before its actual node limit could be checked.
    assert!(maximum < 1_000_000, "oversized DOM reserve: {maximum}");
    assert!(
        total < 1_000_000,
        "unexpected allocations before rejection: {total}"
    );
}
