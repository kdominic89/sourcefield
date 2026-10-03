import assert from 'node:assert/strict';

/** Compare real WASM and fallback motion, then measure the bounded worst-case field. */
export async function runSimulationChecks({ browser, base }) {
    const page = await browser.newPage();

    try {
        await page.goto(base);
        const result = await page.evaluate(async () => {
            const wasm = await import('./pkg/sourcefield_wasm.js');
            const fallback = await import('./simulation-fallback.js');
            await wasm.default();
            const anchors = [100,100,200,100,150,200];
            const pairs = [0,1,1,2];
            const weights = [.8,.4];
            const native = new wasm.Simulator(anchors,anchors,pairs,weights,42,1800,1680);
            const script = new fallback.Simulator(anchors,anchors,pairs,weights,42,1800,1680);
            let maximumDifference = 0;
            const measurements = [];

            try {
                for (let frame = 0; frame < 1000; frame++) {
                    native.advance(.016);
                    script.advance(.016);

                    for (let index = 0; index < anchors.length; index++) {
                        maximumDifference = Math.max(maximumDifference, Math.abs(native.coordinate(index) - script.coordinate(index)));
                    }
                }

                for (const count of [64,512]) {
                    const coordinates = Array.from({length:count * 2}, (_,index) =>
                        index % 2 ? 20 + Math.floor(index / 2 / 32) * 40 : 20 + (index / 2 % 32) * 40);
                    const simulator = new wasm.Simulator(coordinates,coordinates,[],[],42,1800,1680);
                    const output = new Float32Array(coordinates.length);
                    const frames = 240;
                    const start = performance.now();

                    for (let frame = 0; frame < frames; frame++) {
                        simulator.advance(.016);

                        for (let index = 0; index < output.length; index++) {
                            output[index] = simulator.coordinate(index);
                        }
                    }

                    measurements.push({nodes:count,frames,milliseconds:performance.now() - start,finite:output.every(Number.isFinite)});
                    simulator.free();
                }
            } finally {
                native.free();
            }

            return {maximumDifference,measurements};
        });

        // Rust evaluates f32 while JS evaluates arithmetic as f64; compare geometry, not bits.
        assert.ok(result.maximumDifference < .05, JSON.stringify(result));
        assert.ok(result.measurements.every(sample => sample.finite));

        return result;
    } finally {
        await page.close();
    }
}
