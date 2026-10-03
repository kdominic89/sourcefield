//! Isolated renderer allocation and wall-clock measurement from an already built state.
//!
//! Run `cargo run --release -p sourcefield-render --example render_measure -- STATE.json 1000 triplet`.
//! Timings include allocator instrumentation; requested heap bytes are not process RSS.

use std::{
    alloc::{GlobalAlloc, Layout, System},
    error::Error,
    hint::black_box,
    sync::atomic::{AtomicUsize, Ordering},
    time::Instant,
};

use sourcefield_core::ProfileState;
use sourcefield_render::{PreparedPresentation, Theme, render_svg};

struct MeasuredAllocator;

static LIVE_BYTES: AtomicUsize = AtomicUsize::new(0);
static PEAK_BYTES: AtomicUsize = AtomicUsize::new(0);
static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);

#[global_allocator]
static ALLOCATOR: MeasuredAllocator = MeasuredAllocator;

fn record_allocation(bytes: usize) {
    let live = LIVE_BYTES.fetch_add(bytes, Ordering::Relaxed) + bytes;
    PEAK_BYTES.fetch_max(live, Ordering::Relaxed);
    ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
}

// SAFETY: Every allocation operation delegates the identical layout and pointer to System.
// Counters never access allocation contents and use atomics without allocating recursively.
unsafe impl GlobalAlloc for MeasuredAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: The caller supplies a valid allocator layout; it is forwarded unchanged.
        let pointer = unsafe { System.alloc(layout) };

        if !pointer.is_null() {
            record_allocation(layout.size());
        }

        pointer
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        LIVE_BYTES.fetch_sub(layout.size(), Ordering::Relaxed);
        // SAFETY: The caller's System allocation pointer and original layout are preserved.
        unsafe { System.dealloc(pointer, layout) };
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        // SAFETY: Forwarding preserves the caller's realloc contract and original layout.
        let replacement = unsafe { System.realloc(pointer, layout, size) };

        if !replacement.is_null() {
            LIVE_BYTES.fetch_sub(layout.size(), Ordering::Relaxed);
            record_allocation(size);
        }

        replacement
    }
}

/// Measure only SVG production; parsing and warmup remain outside the sample.
fn main() -> Result<(), Box<dyn Error>> {
    let path = std::env::args().nth(1).ok_or("state path required")?;
    let iterations: usize = std::env::args()
        .nth(2)
        .unwrap_or_else(|| "1000".into())
        .parse()?;
    let mode = std::env::args().nth(3).unwrap_or_else(|| "triplet".into());
    if iterations == 0 || !matches!(mode.as_str(), "single" | "triplet") {
        return Err("positive iterations and single/triplet mode required".into());
    }

    let state: ProfileState = serde_json::from_slice(&std::fs::read(path)?)?;
    for _ in 0..20 {
        black_box(render(&state, &mode));
    }

    let live_before = LIVE_BYTES.load(Ordering::Relaxed);
    PEAK_BYTES.store(live_before, Ordering::Relaxed);
    ALLOCATIONS.store(0, Ordering::Relaxed);
    let start = Instant::now();
    let mut output_bytes = 0;

    for _ in 0..iterations {
        output_bytes += black_box(render(black_box(&state), &mode));
    }

    let elapsed = start.elapsed().as_nanos();
    let allocations = ALLOCATIONS.load(Ordering::Relaxed);
    let peak = PEAK_BYTES
        .load(Ordering::Relaxed)
        .saturating_sub(live_before);
    println!(
        "mode={mode} iterations={iterations} elapsed_ns={elapsed} allocations={allocations} peak_live_bytes={peak} output_bytes={output_bytes}"
    );

    Ok(())
}

/// Match artifact production's three flavor calls without measuring filesystem I/O.
fn render(state: &ProfileState, mode: &str) -> usize {
    if mode == "single" {
        return render_svg(state, Theme::Dark, true).len();
    }

    let prepared = PreparedPresentation::new(state);
    let mut bytes = 0;
    for (theme, motion) in [
        (Theme::Dark, true),
        (Theme::Light, true),
        (Theme::Dark, false),
    ] {
        bytes += black_box(prepared.render(theme, motion)).len();
    }

    bytes
}
