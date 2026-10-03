//! Bounded deterministic force simulation shared by browser and native verification.
#![forbid(unsafe_code)]

use wasm_bindgen::prelude::*;

const EPSILON: f32 = 0.000_1;
const MAX_NODES: usize = 512;
const MAX_EDGES: usize = 8192;
const MAX_CANVAS: f32 = 100_000.0;

/// Deterministic force-field simulation used by the interactive GitHub Pages view.
///
/// The native generator owns the canonical graph and layout. This WebAssembly
/// module only adds small, bounded motion around those anchors so the browser
/// view stays faithful to the generated SVG.
#[wasm_bindgen]
pub struct Simulator {
    positions: Vec<f32>,
    velocities: Vec<f32>,
    forces: Vec<f32>,
    anchors: Vec<f32>,
    edge_pairs: Vec<u32>,
    edge_weights: Vec<f32>,
    seed: u32,
    elapsed: f32,
    width: f32,
    height: f32,
}

#[wasm_bindgen]
impl Simulator {
    /// Creates a field bounded by the canonical canvas dimensions.
    ///
    /// # Errors
    /// Returns an error for non-finite coordinates, malformed edges, more than 512 nodes
    /// or 8192 edges, or canvas dimensions outside (0, 100000].
    #[wasm_bindgen(constructor)]
    pub fn new(
        positions: Vec<f32>,
        anchors: Vec<f32>,
        edge_pairs: Vec<u32>,
        edge_weights: Vec<f32>,
        seed: u32,
        width: f32,
        height: f32,
    ) -> Result<Simulator, JsValue> {
        validate_input(
            &positions,
            &anchors,
            &edge_pairs,
            &edge_weights,
            width,
            height,
        )
        .map_err(JsValue::from_str)?;

        Ok(Self {
            velocities: vec![0.0; positions.len()],
            forces: vec![0.0; positions.len()],
            positions,
            anchors,
            edge_pairs,
            edge_weights,
            seed: seed.max(1),
            elapsed: 0.0,
            width,
            height,
        })
    }

    /// Advances the simulation and returns an owned coordinate snapshot.
    /// Use `advance` and scalar coordinate access for allocation-free animation.
    pub fn tick(&mut self, delta_seconds: f32) -> Vec<f32> {
        self.advance(delta_seconds);

        self.positions.clone()
    }

    /// Advances the simulation without allocating or exposing WASM memory views.
    pub fn advance(&mut self, delta_seconds: f32) {
        if !delta_seconds.is_finite() {
            return;
        }

        let dt = delta_seconds.clamp(0.0, 0.05);
        if dt <= 0.0 {
            return;
        }

        self.elapsed += dt;

        let count = self.positions.len() / 2;
        // Reuse scratch storage because every animation frame follows the same graph shape.
        self.forces.fill(0.0);
        let forces = &mut self.forces;

        // Keep every node close to the deterministic native layout.
        for index in 0..count {
            let offset = index * 2;
            let anchor_strength = 7.5 + (index % 5) as f32 * 0.24;
            forces[offset] += (self.anchors[offset] - self.positions[offset]) * anchor_strength;
            forces[offset + 1] +=
                (self.anchors[offset + 1] - self.positions[offset + 1]) * anchor_strength;

            // Tiny seeded phase offsets prevent a mechanically uniform motion.
            let phase = seeded_fraction(self.seed, index as u32) * core::f32::consts::TAU;
            forces[offset] += (self.elapsed * 0.37 + phase).sin() * 0.78;
            forces[offset + 1] += (self.elapsed * 0.29 + phase * 1.37).cos() * 0.62;
        }

        // Mild pairwise repulsion. The graph is intentionally small, so O(n^2)
        // keeps the implementation dependency-free and deterministic.
        for left in 0..count {
            for right in (left + 1)..count {
                let lx = self.positions[left * 2];
                let ly = self.positions[left * 2 + 1];
                let rx = self.positions[right * 2];
                let ry = self.positions[right * 2 + 1];
                let dx = lx - rx;
                let dy = ly - ry;
                let distance_squared = (dx * dx + dy * dy).max(36.0);
                let distance = distance_squared.sqrt();
                let force = 34.0 / distance_squared;
                let fx = dx / distance * force;
                let fy = dy / distance * force;
                forces[left * 2] += fx;
                forces[left * 2 + 1] += fy;
                forces[right * 2] -= fx;
                forces[right * 2 + 1] -= fy;
            }
        }

        // Edges pull related nodes toward their original anchor distance rather
        // than toward an arbitrary fixed length.
        for edge_index in 0..self.edge_weights.len() {
            let left = self.edge_pairs[edge_index * 2] as usize;
            let right = self.edge_pairs[edge_index * 2 + 1] as usize;
            if left >= count || right >= count || left == right {
                continue;
            }

            let lx = self.positions[left * 2];
            let ly = self.positions[left * 2 + 1];
            let rx = self.positions[right * 2];
            let ry = self.positions[right * 2 + 1];
            let dx = rx - lx;
            let dy = ry - ly;
            let distance = (dx * dx + dy * dy).sqrt().max(EPSILON);

            let adx = self.anchors[right * 2] - self.anchors[left * 2];
            let ady = self.anchors[right * 2 + 1] - self.anchors[left * 2 + 1];
            let target = (adx * adx + ady * ady).sqrt().max(28.0);
            let strength = 0.42 + self.edge_weights[edge_index].clamp(0.0, 1.0) * 1.15;
            let displacement = (distance - target) * strength;
            let fx = dx / distance * displacement;
            let fy = dy / distance * displacement;

            forces[left * 2] += fx;
            forces[left * 2 + 1] += fy;
            forces[right * 2] -= fx;
            forces[right * 2 + 1] -= fy;
        }

        let damping = 0.88_f32.powf(dt * 60.0);
        for index in 0..count {
            let offset = index * 2;
            self.velocities[offset] = (self.velocities[offset] + forces[offset] * dt) * damping;
            self.velocities[offset + 1] =
                (self.velocities[offset + 1] + forces[offset + 1] * dt) * damping;

            self.positions[offset] += self.velocities[offset] * dt;
            self.positions[offset + 1] += self.velocities[offset + 1] * dt;

            // Canonical anchors use the declared canvas; fixed legacy dimensions clipped V3 nodes.
            self.positions[offset] = self.positions[offset].clamp(0.0, self.width);
            self.positions[offset + 1] = self.positions[offset + 1].clamp(0.0, self.height);
        }
    }

    /// Reads one coordinate without borrowing a view into growable WASM memory.
    /// Returns NaN for an invalid index so callers cannot read outside the buffer.
    pub fn coordinate(&self, index: usize) -> f32 {
        self.positions.get(index).copied().unwrap_or(f32::NAN)
    }

    /// Returns a copy of flattened x/y coordinates.
    pub fn positions(&self) -> Vec<f32> {
        self.positions.clone()
    }

    /// Restores anchors and clears velocity and elapsed animation time.
    pub fn reset(&mut self) -> Vec<f32> {
        self.positions.clone_from(&self.anchors);
        self.velocities.fill(0.0);
        self.elapsed = 0.0;
        self.positions.clone()
    }

    /// Returns the number of nodes.
    pub fn len(&self) -> usize {
        self.positions.len() / 2
    }

    /// Reports whether the field contains no nodes.
    pub fn is_empty(&self) -> bool {
        self.positions.is_empty()
    }
}

/// Keep validation pure so malformed browser inputs can be exercised on native targets.
fn validate_input(
    positions: &[f32],
    anchors: &[f32],
    edge_pairs: &[u32],
    edge_weights: &[f32],
    width: f32,
    height: f32,
) -> Result<(), &'static str> {
    if !width.is_finite()
        || !height.is_finite()
        || width <= 0.0
        || height <= 0.0
        || width > MAX_CANVAS
        || height > MAX_CANVAS
    {
        return Err("canvas dimensions must be finite and within 0..=100000");
    }

    if positions.len() / 2 > MAX_NODES || edge_weights.len() > MAX_EDGES {
        return Err("simulation exceeds bounded node or edge capacity");
    }

    if positions.is_empty()
        || !positions.len().is_multiple_of(2)
        || positions.len() != anchors.len()
    {
        return Err("positions and anchors must contain matching x/y pairs");
    }

    if positions
        .iter()
        .chain(anchors)
        .enumerate()
        .any(|(i, value)| {
            !value.is_finite() || *value < 0.0 || *value > if i % 2 == 0 { width } else { height }
        })
    {
        return Err("coordinates must be finite and inside the canvas");
    }

    if !edge_pairs.len().is_multiple_of(2) || edge_weights.len() != edge_pairs.len() / 2 {
        return Err("edges and weights must contain matching pairs");
    }

    if edge_pairs
        .iter()
        .any(|index| *index as usize >= positions.len() / 2)
        || edge_weights
            .iter()
            .any(|weight| !weight.is_finite() || !(0.0..=1.0).contains(weight))
    {
        return Err("edge indices and weights must be valid");
    }

    Ok(())
}

/// Derives repeatable phase offsets without an external random-number source.
fn seeded_fraction(seed: u32, index: u32) -> f32 {
    let mut value = seed ^ index.wrapping_mul(0x9E37_79B9);
    value ^= value << 13;
    value ^= value >> 17;
    value ^= value << 5;
    (value % 10_000) as f32 / 10_000.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simulation_is_bounded_and_deterministic() {
        let positions = vec![100.0, 100.0, 200.0, 100.0];
        let anchors = positions.clone();
        let edges = vec![0, 1];
        let weights = vec![0.8];
        let mut left = Simulator::new(
            positions.clone(),
            anchors.clone(),
            edges.clone(),
            weights.clone(),
            42,
            1800.0,
            1680.0,
        )
        .unwrap();

        let mut right =
            Simulator::new(positions, anchors, edges, weights, 42, 1800.0, 1680.0).unwrap();

        let left_positions = left.tick(0.016);
        let right_positions = right.tick(0.016);

        assert_eq!(left_positions, right_positions);
        assert_eq!(left.len(), 2);
    }

    #[test]
    fn valid_coordinates_are_accepted_before_crossing_the_wasm_boundary() {
        let positions = [100.0, 100.0];

        let result = validate_input(&positions, &positions, &[], &[], 1800.0, 1680.0);

        assert!(result.is_ok());
    }

    #[test]
    fn nonfinite_coordinates_are_rejected_before_crossing_the_wasm_boundary() {
        let positions = [f32::NAN, 0.0];
        let anchors = [100.0, 100.0];

        let result = validate_input(&positions, &anchors, &[], &[], 1800.0, 1680.0);

        assert!(result.is_err());
    }

    #[test]
    fn invalid_edge_indices_are_rejected_before_crossing_the_wasm_boundary() {
        let positions = [100.0, 100.0];
        let edges = [0, 1];
        let weights = [0.8];

        let result = validate_input(&positions, &positions, &edges, &weights, 1800.0, 1680.0);

        assert!(result.is_err());
    }

    #[test]
    fn invalid_canvas_is_rejected_before_crossing_the_wasm_boundary() {
        let positions = [100.0, 100.0];
        let width = -1.0;

        let result = validate_input(&positions, &positions, &[], &[], width, 1680.0);

        assert!(result.is_err());
    }

    #[test]
    fn long_running_motion_respects_custom_bounds() {
        let anchors = vec![0.0, 0.0, 2200.0, 1900.0];
        let mut simulator = Simulator::new(
            anchors.clone(),
            anchors.clone(),
            vec![0, 1],
            vec![1.0],
            17,
            2200.0,
            1900.0,
        )
        .unwrap();

        let mut bounded = true;

        for _ in 0..10_000 {
            simulator.advance(0.05);
            bounded &=
                simulator.positions.as_chunks::<2>().0.iter().all(|pair| {
                    (0.0..=2200.0).contains(&pair[0]) && (0.0..=1900.0).contains(&pair[1])
                });
        }

        assert!(bounded);
    }

    #[test]
    fn advance_retains_storage_and_matches_snapshot_api() {
        let anchors = vec![100.0, 100.0, 200.0, 100.0];
        let make = || {
            Simulator::new(
                anchors.clone(),
                anchors.clone(),
                vec![0, 1],
                vec![0.8],
                42,
                1800.0,
                1680.0,
            )
            .unwrap()
        };

        let mut buffered = make();
        let mut snapshots = make();
        let positions_pointer = buffered.positions.as_ptr();
        let forces_pointer = buffered.forces.as_ptr();
        let positions_capacity = buffered.positions.capacity();
        let forces_capacity = buffered.forces.capacity();

        for _ in 0..1000 {
            buffered.advance(0.016);
            snapshots.tick(0.016);
        }

        assert_eq!(buffered.positions, snapshots.positions);
        assert_eq!(buffered.positions.as_ptr(), positions_pointer);
        assert_eq!(buffered.forces.as_ptr(), forces_pointer);
        assert_eq!(buffered.positions.capacity(), positions_capacity);
        assert_eq!(buffered.forces.capacity(), forces_capacity);
        assert!(buffered.coordinate(usize::MAX).is_nan());
    }

    #[test]
    fn node_capacity_rejects_excessive_quadratic_work() {
        let coordinates = vec![0.0; (MAX_NODES + 1) * 2];

        let result = validate_input(&coordinates, &coordinates, &[], &[], 1800.0, 1680.0);

        assert_eq!(
            result,
            Err("simulation exceeds bounded node or edge capacity")
        );
    }

    #[test]
    fn edge_capacity_rejects_excessive_storage() {
        let coordinates = [10.0, 10.0];
        let pairs = vec![0; (MAX_EDGES + 1) * 2];
        let weights = vec![0.5; MAX_EDGES + 1];

        let result = validate_input(&coordinates, &coordinates, &pairs, &weights, 1800.0, 1680.0);

        assert_eq!(
            result,
            Err("simulation exceeds bounded node or edge capacity")
        );
    }

    #[test]
    fn invalid_deltas_do_not_mutate_reused_buffers() {
        let anchors = vec![10.0, 20.0];
        let mut simulator = Simulator::new(
            anchors.clone(),
            anchors.clone(),
            vec![],
            vec![],
            1,
            1800.0,
            1680.0,
        )
        .unwrap();

        for delta in [f32::NAN, f32::INFINITY, -1.0, 0.0] {
            simulator.advance(delta);
        }

        assert_eq!(simulator.positions, anchors);
        assert_eq!(simulator.elapsed, 0.0);
        assert_eq!(simulator.velocities, vec![0.0, 0.0]);
    }

    #[test]
    fn canvas_limit_prevents_force_arithmetic_overflow() {
        let coordinates = [10.0, 20.0];

        let result = validate_input(&coordinates, &coordinates, &[], &[], f32::MAX, f32::MAX);

        assert!(result.is_err());
    }

    #[test]
    fn reset_restores_anchors_and_clears_motion() {
        let anchors = vec![10.0, 20.0];
        let mut simulator = Simulator::new(
            anchors.clone(),
            anchors.clone(),
            vec![],
            vec![],
            1,
            1800.0,
            1680.0,
        )
        .unwrap();

        simulator.advance(0.05);

        let reset = simulator.reset();

        assert_eq!(reset, anchors);
        assert_eq!(simulator.elapsed, 0.0);
        assert_eq!(simulator.velocities, vec![0.0, 0.0]);
    }
}
