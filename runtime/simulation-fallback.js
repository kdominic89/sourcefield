/** Deterministic JavaScript fallback, explicitly distinct from the Rust/WASM engine. */
export class Simulator {
    /** Copy caller-owned buffers and reject invalid coordinates before animation starts. */
    constructor(positions, anchors, edgePairs, edgeWeights, seed = 1, width = 1800, height = 1680) {
        if (!Number.isFinite(width) || !Number.isFinite(height) || width <= 0 || height <= 0 || width > 100000 || height > 100000 ||
            positions.length > 1024 || edgeWeights.length > 8192 || positions.length === 0 || positions.length % 2 || anchors.length !== positions.length ||
            edgePairs.length % 2 || edgeWeights.length !== edgePairs.length / 2) {
            throw new RangeError('Invalid simulation shape');
        }

        for (const values of [positions, anchors]) {
            if (values.some((value, index) => !Number.isFinite(value) || value < 0 ||
                value > (index % 2 ? height : width))) {
                throw new RangeError('Coordinates must be inside the canvas');
            }
        }

        if (edgePairs.some(index => !Number.isInteger(index) || index < 0 || index >= positions.length / 2) ||
            edgeWeights.some(weight => !Number.isFinite(weight) || weight < 0 || weight > 1)) {
            throw new RangeError('Invalid edge');
        }

        this.positionData = Float32Array.from(positions);
        this.anchorData = Float32Array.from(anchors);
        this.velocityData = new Float32Array(positions.length);
        this.forceData = new Float32Array(positions.length);
        this.edgePairs = Uint32Array.from(edgePairs);
        this.edgeWeights = Float32Array.from(edgeWeights);
        this.seed = seed || 1;
        this.width = width;
        this.height = height;
        this.elapsed = 0;
    }

    /** Advance and return an owned snapshot for callers needing persistent coordinates. */
    tick(deltaSeconds) {
        this.advance(deltaSeconds);

        return this.positions();
    }

    /** Match the Rust force model while reusing all per-frame simulation buffers. */
    advance(deltaSeconds) {
        if (!Number.isFinite(deltaSeconds) || deltaSeconds <= 0) return;

        const dt = Math.min(.05, deltaSeconds);
        this.elapsed += dt;
        const forces = this.forceData;
        const positions = this.positionData;
        const count = positions.length / 2;
        forces.fill(0);

        for (let index = 0; index < count; index++) {
            const offset = index * 2;
            const phase = seeded(this.seed, index) * Math.PI * 2;
            const strength = 7.5 + (index % 5) * .24;
            forces[offset] = (this.anchorData[offset] - positions[offset]) * strength +
                Math.sin(this.elapsed * .37 + phase) * .78;
            forces[offset + 1] = (this.anchorData[offset + 1] - positions[offset + 1]) * strength +
                Math.cos(this.elapsed * .29 + phase * 1.37) * .62;
        }

        for (let left = 0; left < count; left++) {
            for (let right = left + 1; right < count; right++) {
                const dx = positions[left * 2] - positions[right * 2];
                const dy = positions[left * 2 + 1] - positions[right * 2 + 1];
                const squared = Math.max(36, dx * dx + dy * dy);
                const distance = Math.sqrt(squared);
                const force = 34 / squared;
                const fx = dx / distance * force;
                const fy = dy / distance * force;
                forces[left * 2] += fx;
                forces[left * 2 + 1] += fy;
                forces[right * 2] -= fx;
                forces[right * 2 + 1] -= fy;
            }
        }

        for (let index = 0; index < this.edgeWeights.length; index++) {
            const left = this.edgePairs[index * 2] * 2;
            const right = this.edgePairs[index * 2 + 1] * 2;
            if (left === right) continue;

            const dx = positions[right] - positions[left];
            const dy = positions[right + 1] - positions[left + 1];
            const distance = Math.max(.0001, Math.sqrt(dx * dx + dy * dy));
            const ax = this.anchorData[right] - this.anchorData[left];
            const ay = this.anchorData[right + 1] - this.anchorData[left + 1];
            const target = Math.max(28, Math.sqrt(ax * ax + ay * ay));
            const displacement = (distance - target) * (.42 + this.edgeWeights[index] * 1.15);
            const fx = dx / distance * displacement;
            const fy = dy / distance * displacement;
            forces[left] += fx;
            forces[left + 1] += fy;
            forces[right] -= fx;
            forces[right + 1] -= fy;
        }

        const damping = Math.pow(.88, dt * 60);

        for (let index = 0; index < positions.length; index++) {
            this.velocityData[index] = (this.velocityData[index] + forces[index] * dt) * damping;
            positions[index] = Math.max(0, Math.min(index % 2 ? this.height : this.width,
                positions[index] + this.velocityData[index] * dt));
        }
    }

    /** Read one coordinate without exposing mutable simulator-owned storage. */
    coordinate(index) {
        return this.positionData[index] ?? Number.NaN;
    }

    /** Return a defensive coordinate copy. */
    positions() {
        return this.positionData.slice();
    }

    /** Restore the original anchors and clear accumulated motion. */
    reset() {
        this.positionData.set(this.anchorData);
        this.velocityData.fill(0);
        this.elapsed = 0;

        return this.positions();
    }

    /** Return the number of simulated nodes. */
    len() {
        return this.positionData.length / 2;
    }

    /** Report whether the field contains no nodes. */
    is_empty() {
        return this.positionData.length === 0;
    }
}

/** Match the Rust phase seed without relying on random browser state. */
function seeded(seed, index) {
    let value = (seed ^ Math.imul(index, 0x9e3779b9)) >>> 0;
    value ^= value << 13;
    value ^= value >>> 17;
    value ^= value << 5;

    return (value >>> 0) % 10000 / 10000;
}
