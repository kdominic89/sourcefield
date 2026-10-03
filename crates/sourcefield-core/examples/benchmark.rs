//! Reproducible native preparation, graph, and README benchmark with allocation instrumentation.
//!
//! Run `cargo run --release -p sourcefield-core --example benchmark -- 100`.
//! Timings include allocator instrumentation; requested heap bytes are not process RSS.

use std::{
    alloc::{GlobalAlloc, Layout, System},
    error::Error,
    hint::black_box,
    sync::atomic::{AtomicUsize, Ordering},
    time::Instant,
};

use sourcefield_core::{
    Config, Snapshot, build_prepared_state, prepare_profile, render_package_readme_prepared,
    render_project_readme,
};

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

fn main() -> Result<(), Box<dyn Error>> {
    let iterations = std::env::args()
        .nth(1)
        .map_or(Ok(100usize), |value| value.parse())?;

    if iterations == 0 {
        return Err("iteration count must be positive".into());
    }

    let config: Config = toml::from_str(include_str!("../../../config/profile.toml"))?;
    let snapshot: Snapshot =
        serde_json::from_str(include_str!("../../../config/offline-snapshot.json"))?;

    measure("synthetic-profile", &config, &snapshot, iterations)?;
    let capacity = capacity_config(&config);

    measure(
        "512-node-capacity",
        &capacity,
        &Snapshot::default(),
        iterations,
    )?;

    Ok(())
}

fn capacity_config(template: &Config) -> Config {
    let mut config = template.clone();
    let mut project = config.projects[0].clone();
    project.components.clear();
    project.implemented_with.clear();
    project.integrates.clear();
    project.targets.clear();
    project.repository = None;
    project.label_prefix = None;
    project.radius = Some(48.0);
    config.domains.truncate(1);
    config.domains[0].anchor = [900.0, 400.0];
    project.domain = config.domains[0].id.clone();
    config.technologies.clear();
    config.publications.clear();
    config.interests.clear();
    config.learning.clear();
    config.projects = (0..511)
        .map(|index| {
            let mut item = project.clone();
            item.id = format!("project-{index:03}");
            item.label = format!("Project {index:03}");
            item.surface_label = item.label.clone();
            item.anchor = [
                300.0 + (index % 3) as f32 * 600.0,
                1000.0 + (index / 3) as f32 * 240.0,
            ];

            item
        })
        .collect();
    config.render.height = 43000;

    config
}

fn measure(
    name: &str,
    config: &Config,
    snapshot: &Snapshot,
    iterations: usize,
) -> Result<(), Box<dyn Error>> {
    let prepared = prepare_profile(config, snapshot)?;
    let reference = build_prepared_state(&prepared, "benchmark-fixed-time")?;
    let expected_hash = reference.semantic_hash.clone();
    let nodes = reference.nodes.len();
    let edges = reference.edges.len();
    drop(reference);
    drop(prepared);
    let baseline = LIVE_BYTES.load(Ordering::Relaxed);
    PEAK_BYTES.store(baseline, Ordering::Relaxed);
    ALLOCATIONS.store(0, Ordering::Relaxed);
    let started = Instant::now();

    for _ in 0..iterations {
        let prepared = prepare_profile(black_box(config), black_box(snapshot))?;
        let state = build_prepared_state(&prepared, "benchmark-fixed-time")?;
        let projects = render_project_readme(prepared.config());
        let packages = render_package_readme_prepared(prepared.config());
        assert_eq!(
            state.semantic_hash, expected_hash,
            "semantic output changed during repeated generation"
        );
        black_box((state, projects, packages));
    }

    let elapsed = started.elapsed();
    let retained_bytes = LIVE_BYTES.load(Ordering::Relaxed).saturating_sub(baseline);
    let peak_extra_bytes = PEAK_BYTES.load(Ordering::Relaxed).saturating_sub(baseline);
    let allocations = ALLOCATIONS.load(Ordering::Relaxed);
    assert_eq!(
        retained_bytes, 0,
        "generation retained heap allocations after outputs were dropped"
    );
    println!(
        "case={name} nodes={nodes} edges={edges} iterations={iterations} total_ms={:.3} us_per_iteration={:.3} peak_extra_heap_bytes={peak_extra_bytes} retained_heap_bytes={retained_bytes} allocations_per_iteration={:.1} semantic_hash={expected_hash}",
        elapsed.as_secs_f64() * 1000.0,
        elapsed.as_secs_f64() * 1_000_000.0 / iterations as f64,
        allocations as f64 / iterations as f64
    );

    Ok(())
}
