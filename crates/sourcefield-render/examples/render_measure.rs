//! Isolated renderer allocation and wall-clock measurement from an already built state.
//!
//! Run `cargo run --release -p sourcefield-render --example render_measure -- STATE.json 1000 triplet`.
//! Add `icons` as the mode to compare legacy, built-in, and one shared custom definition.
//! Icon samples hash every output and verify preparation allocations do not depend on catalog content.
//! Timings include allocator instrumentation and, in icon mode, checksums.
//! Requested heap bytes are not process RSS.

use std::{
    alloc::{GlobalAlloc, Layout, System},
    error::Error,
    hint::black_box,
    sync::atomic::{AtomicUsize, Ordering},
    time::Instant,
};

use sourcefield_core::{NodeKind, ProfileState, builtin_icon, validate_state};
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
    if iterations == 0 || !matches!(mode.as_str(), "single" | "triplet" | "icons") {
        return Err("positive iterations and single/triplet/icons mode required".into());
    }

    let state: ProfileState = serde_json::from_slice(&std::fs::read(path)?)?;
    if mode == "icons" {
        return measure_icons(state, iterations);
    }

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

/// Compare catalog cases on identical geometry, with all case construction outside measurements.
fn measure_icons(mut state: ProfileState, iterations: usize) -> Result<(), Box<dyn Error>> {
    let shared = builtin_icon("builtin:sourcefield")
        .ok_or("missing built-in catalog entry")?
        .clone();
    let mut preparation_baseline = None;

    for case in ["legacy", "builtin", "shared-custom"] {
        state.icons.clear();

        if case == "shared-custom" {
            state.icons.insert("shared".into(), shared.clone());
        }

        let mut selected = 0;

        for node in &mut state.nodes {
            node.icon = if node.kind == NodeKind::Project {
                selected += 1;

                match case {
                    "builtin" => Some("builtin:sourcefield".into()),
                    "shared-custom" => Some("shared".into()),
                    _ => None,
                }
            } else {
                None
            };
        }

        if selected == 0 {
            return Err("icon measurement requires at least one project".into());
        }

        validate_state(&state)?;
        let expected = fingerprint(&state);

        for _ in 0..20 {
            assert_eq!(black_box(fingerprint(&state)), expected);
        }

        let preparation = measure_preparation(&state);

        if let Some(baseline) = preparation_baseline {
            assert_eq!(
                preparation, baseline,
                "catalog content must not be cloned during preparation"
            );
        } else {
            preparation_baseline = Some(preparation);
        }

        let live_before = LIVE_BYTES.load(Ordering::Relaxed);
        PEAK_BYTES.store(live_before, Ordering::Relaxed);
        ALLOCATIONS.store(0, Ordering::Relaxed);
        let start = Instant::now();

        for _ in 0..iterations {
            assert_eq!(black_box(fingerprint(black_box(&state))), expected);
        }

        let elapsed = start.elapsed().as_nanos();
        let allocations = ALLOCATIONS.load(Ordering::Relaxed);
        let live_after = LIVE_BYTES.load(Ordering::Relaxed);
        let peak = PEAK_BYTES
            .load(Ordering::Relaxed)
            .saturating_sub(live_before);
        assert_eq!(live_after, live_before, "rendering retained heap bytes");
        println!(
            "case={case} nodes={} projects={selected} definitions={} iterations={iterations} \
             elapsed_ns={elapsed} allocations={allocations} peak_live_bytes={peak} \
             retained_bytes=0 \
             preparation_allocations={} preparation_peak_bytes={} \
             output_bytes={} output_hash={:016x}",
            state.nodes.len(),
            state.icons.len(),
            preparation.0,
            preparation.1,
            expected.0,
            expected.1,
        );
    }

    Ok(())
}

/// Observe borrowed preparation alone, excluding output strings and catalog construction.
fn measure_preparation(state: &ProfileState) -> (usize, usize) {
    let live_before = LIVE_BYTES.load(Ordering::Relaxed);
    PEAK_BYTES.store(live_before, Ordering::Relaxed);
    ALLOCATIONS.store(0, Ordering::Relaxed);
    let prepared = PreparedPresentation::new(black_box(state));
    let allocations = ALLOCATIONS.load(Ordering::Relaxed);
    let peak = PEAK_BYTES
        .load(Ordering::Relaxed)
        .saturating_sub(live_before);
    drop(black_box(prepared));
    assert_eq!(LIVE_BYTES.load(Ordering::Relaxed), live_before);

    (allocations, peak)
}

/// Fingerprint complete triplets without retaining output buffers or allocating checksum state.
fn fingerprint(state: &ProfileState) -> (usize, u64) {
    let prepared = PreparedPresentation::new(state);
    let mut bytes = 0;
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;

    for (theme, motion) in [
        (Theme::Dark, true),
        (Theme::Light, true),
        (Theme::Dark, false),
    ] {
        let svg = prepared.render(theme, motion);
        bytes += svg.len();

        for byte in svg.bytes() {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x0100_0000_01b3);
        }
    }

    (bytes, hash)
}
