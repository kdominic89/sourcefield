//! Repeatable native simulation timing and retained-storage measurements.

use sourcefield_wasm::Simulator;
use std::{hint::black_box, time::Instant};

/// Measure both snapshot and retained-buffer APIs against identical seeded inputs.
fn main() {
    for count in [64, 512] {
        let anchors: Vec<f32> = (0..count)
            .flat_map(|index| {
                [
                    20.0 + (index % 32) as f32 * 40.0,
                    20.0 + (index / 32) as f32 * 40.0,
                ]
            })
            .collect();

        let make = || {
            Simulator::new(
                anchors.clone(),
                anchors.clone(),
                vec![],
                vec![],
                42,
                1800.0,
                1680.0,
            )
            .unwrap()
        };

        let frames = 1000;
        let mut retained = make();
        let mut snapshots = make();
        let started = Instant::now();

        for _ in 0..frames {
            retained.advance(black_box(0.016));
            black_box(retained.coordinate(0));
        }

        let retained_ms = started.elapsed().as_secs_f64() * 1000.0;
        let started = Instant::now();

        for _ in 0..frames {
            black_box(snapshots.tick(black_box(0.016)));
        }

        let snapshot_ms = started.elapsed().as_secs_f64() * 1000.0;
        assert_eq!(retained.positions(), snapshots.positions());
        println!(
            "nodes={count} frames={frames} retained_ms={retained_ms:.3} snapshot_ms={snapshot_ms:.3} coordinate_storage_bytes={} snapshot_coordinate_copies_avoided={}",
            count * 2 * 4 * 4,
            count * 2 * 4 * frames
        );
    }
}
