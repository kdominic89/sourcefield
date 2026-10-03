/** Measure the real canvas overlay in isolation, retaining identical labels and viewport. */
import { readFile, writeFile } from 'node:fs/promises';
import { pathToFileURL } from 'node:url';
import { parseArgs } from 'node:util';

const { values } = parseArgs({ options: {
    baseline: { type: 'string' }, current: { type: 'string' },
    'playwright-module': { type: 'string' }, browser: { type: 'string' },
    output: { type: 'string' },
} });

for (const name of ['baseline', 'current', 'playwright-module', 'browser', 'output']) {
    if (!values[name]) throw new Error(`Missing --${name}`);
}

const { chromium } = await import(pathToFileURL(values['playwright-module']).href);
const browser = await chromium.launch({ executablePath: values.browser, headless: true });
const results = [];

try {
    const pages = {};

    for (const name of ['baseline', 'current']) {
        const page = await browser.newPage({ viewport: { width: 1500, height: 1100 } });
        const source = (await readFile(values[name], 'utf8'))
            .replace(/^import .*;\n/m, '').replace(/boot\(\)\.catch\([\s\S]*$/, '');

        await page.setContent('<style>:root{--mono:monospace;--text:#ffffff;--quiet:#888888;--personal:#abcdef;--organization:#fedcba}</style><canvas id="label-canvas" width="1500" height="1100"></canvas>');
        await page.addScriptTag({ content: source });
        pages[name] = page;
    }

    for (const count of [64, 512]) {
        for (let sample = 0; sample < 5; sample++) {
            // Alternate order so thermal drift does not consistently favor either revision.
            for (const name of sample % 2 ? ['current', 'baseline'] : ['baseline', 'current']) {
                const result = await pages[name].evaluate(({ count }) => {
                    app.nodes = Array.from({ length: count }, (_, index) => ({
                        id: `node-${index}`, kind: index < 2 ? 'domain' : 'project',
                        label: `Project ${index}`, domain: index % 2 ? 'organization' : 'personal',
                        x: 100 + index % 16 * 80, y: 100 + Math.floor(index / 16) * 25,
                    }));
                    app.domainNodes = app.nodes.filter(node => node.kind === 'domain');
                    app.organizationDomains = new Set(['organization']);
                    app.layer = 'systems';
                    app.size = { width: 1500, height: 1100, fit: 1, dpr: 1 };
                    const visual = { nodes: app.nodes.map(node => ({
                        node, x: node.x, y: node.y, size: 8, color: '#abcdef', alpha: 1,
                        selected: false, hovered: false,
                    })) };
                    const palette = readPalette();

                    for (let frame = 0; frame < 40; frame++) drawOverlay(visual, palette, frame / 60);

                    let styleReads = 0;
                    let membershipArrays = 0;
                    const originalStyle = window.getComputedStyle;
                    const originalFilter = app.nodes.filter;
                    window.getComputedStyle = (...args) => {
                        styleReads++;
                        return originalStyle(...args);
                    };
                    app.nodes.filter = function(...args) {
                        membershipArrays++;
                        return originalFilter.apply(this, args);
                    };
                    const started = performance.now();

                    for (let frame = 0; frame < 240; frame++) drawOverlay(visual, palette, frame / 60);

                    const elapsedMs = performance.now() - started;
                    window.getComputedStyle = originalStyle;
                    delete app.nodes.filter;

                    return { elapsedMs, frames: 240, styleReads, membershipArrays,
                        retainedDomainReferences: app.domainNodes.length };
                }, { count });
                results.push({ name, count, sample, ...result });
            }
        }
    }

    const summary = [];

    for (const count of [64, 512]) {
        for (const name of ['baseline', 'current']) {
            const samples = results.filter(result => result.count === count && result.name === name);
            const timings = samples.map(result => result.elapsedMs).sort((a, b) => a - b);
            summary.push({ name, count, medianMs: timings[2], frames: 240,
                styleReads: samples[0].styleReads, membershipArrays: samples[0].membershipArrays,
                retainedDomainReferences: samples[0].retainedDomainReferences });
        }
    }

    await writeFile(values.output, `${JSON.stringify({ summary, results }, null, 2)}\n`);
    console.log(JSON.stringify(summary, null, 2));
} finally {
    await browser.close();
}
