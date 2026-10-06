import assert from 'node:assert/strict';
import { join } from 'node:path';

/** Read actual SVG transforms and pulse opacity at deterministic animation times. */
async function checkMotion(page, state) {
    return page.evaluate(state => {
        const svg = document.querySelector('#profile-view svg');
        const animations = svg.getAnimations({ subtree: true });
        const rings = [...svg.querySelectorAll('[data-ring], [data-project-ring], [data-package-row]')];
        const originalTimes = animations.map(animation => animation.currentTime);

        /** Map the approved ring identity to clockwise (1) or counterclockwise (-1). */
        function expectedDirection(element) {
            if (element.dataset.ring) {
                return {
                    'personal-outer': 1,
                    'personal-inner': -1,
                    'organization-outer': -1,
                    'organization-inner': 1,
                }[element.dataset.ring];
            }

            if (element.dataset.projectRing) {
                return element.dataset.projectRing === 'middle' ? -1 : 1;
            }

            return Number(element.dataset.packageRow) % 2 === 1 ? -1 : 1;
        }

        /** Include ancestor transforms, so nested rotation cancellation is observable. */
        function angle(element) {
            const matrix = element.getCTM();
            return Math.atan2(matrix.b, matrix.a);
        }

        try {
            for (const animation of animations) {
                animation.currentTime = 1000;
            }

            const angles = rings.map(angle);
            for (const animation of animations) {
                animation.currentTime = 2000;
            }

            const directions = rings.map((element, index) => {
                const difference = angle(element) - angles[index];
                const direction = Math.sign(Math.atan2(Math.sin(difference), Math.cos(difference)));

                if (direction !== expectedDirection(element)) {
                    throw new Error(`Wrong direction: ${JSON.stringify(element.dataset)}`);
                }

                return { marker: { ...element.dataset }, direction };
            });
            const signals = animations.filter(animation => animation.animationName === 'signal');

            const visible = state.nodes.filter(node => node.show_in_readme);
            const packages = visible.filter(node => node.kind === 'package');
            const domains = visible.filter(node => node.kind === 'domain');
            const projects = visible.filter(node => node.kind === 'project');
            const personalDomains = domains.filter(node => node.scope !== 'organization');
            const personalProjects = projects.filter(project => personalDomains.some(domain => domain.domain === project.domain));
            const expectedRings = domains.length * 2 + projects.length * 2 + packages.length;
            const expectedSignals = personalProjects.length
                + packages.length;

            if (directions.length !== expectedRings || signals.length !== expectedSignals) {
                throw new Error(`Missing animation markers: ${directions.length} rings, ${signals.length} signals`);
            }

            const timing = signals.map(animation => {
                const effectTiming = animation.effect.getTiming();
                const declared = Number(animation.effect.target.dataset.signalDelay) * 1000;

                if (effectTiming.duration !== 3600 || Math.abs(effectTiming.delay - declared) > 0.001) {
                    throw new Error(`Lost signal timing: ${JSON.stringify({ timing: effectTiming, declared })}`);
                }

                // Negative stagger delays can exceed a cycle as packages accumulate.
                // Choose a later cycle so both probes have nonnegative timeline times.
                const cycle = Math.max(1, Math.ceil(-effectTiming.delay / 3600) + 1);
                const sampleStart = effectTiming.delay + cycle * 3600;
                animation.currentTime = sampleStart;
                const lo = Number(getComputedStyle(animation.effect.target).opacity);
                animation.currentTime = sampleStart + 1620;
                const hi = Number(getComputedStyle(animation.effect.target).opacity);

                if (hi - lo < 0.7) {
                    throw new Error(`No signal pulse: ${JSON.stringify({ lo, hi, delay: effectTiming.delay })}`);
                }

                return { delay: effectTiming.delay, duration: effectTiming.duration, lo, hi };
            });
            const scans = animations.filter(animation => animation.effect.target.classList.contains('scan'));
            const expectedScans = projects.filter(project => project.visual === 'trace' && !project.icon).length;
            const scanDurations = scans.map(animation => animation.effect.getTiming().duration);

            if (scans.length !== expectedScans || scanDurations.some(duration => duration !== 24000)) {
                throw new Error(`Wrong scan count or duration: expected ${expectedScans}, got ${scanDurations}`);
            }

            return {
                directions,
                timing,
                scanDuration: scanDurations[0] ?? null,
                scanDurations,
                inlineStyles: svg.querySelectorAll('[style]').length,
            };
        } finally {
            animations.forEach((animation, index) => { animation.currentTime = originalTimes[index]; });
        }
    }, state);
}

/** Read anchored SVG geometry independently of transient CSS animation transforms. */
async function geometry(page) {
    return page.evaluate(() => [...document.querySelectorAll(
        '#profile-view [data-node-decoration], #profile-view [data-edge-id] path, #profile-view [data-connection] path',
    )].map(element => [element.getAttribute('transform'), element.getAttribute('d')]));
}

/** Wait for paint frames rather than an arbitrary delay before comparing layout. */
async function paint(page) {
    await page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))));
}

/** Check browser path geometry, radial tangency, evenly spaced ports, and package trunks. */
async function checkConnections(page, state) {
    const results = await page.evaluate(state => {
        const svg = document.querySelector('#profile-view svg');

        /** Measure SVG coordinates without depending on the viewport scale. */
        function distance(left, right) {
            return Math.hypot(left.x - right.x, left.y - right.y);
        }

        /** Read the visible outer ring as the attachment boundary. */
        function node(id) {
            const element = svg.querySelector(`[data-node-id="${id}"]`);
            return {
                x: Number(element.dataset.x),
                y: Number(element.dataset.y),
                r: Math.max(...[...element.querySelectorAll('[data-node-decoration] circle')]
                    .map(circle => Number(circle.getAttribute('r')))),
            };
        }

        let maxEndpointError = 0;
        let minAlignment = 1;
        const seen = new Set();
        const personalAngles = [];

        for (const path of svg.querySelectorAll('[data-edge-id] path, [data-connection="domain-bridge"] path')) {
            const parent = path.parentElement;
            const id = parent.dataset.edgeId || `${parent.dataset.connection}:${parent.dataset.from}:${parent.dataset.to}`;

            if (seen.has(id)) {
                continue;
            }

            seen.add(id);
            const source = node(parent.dataset.from);
            const target = node(parent.dataset.to);
            const length = path.getTotalLength();
            const start = path.getPointAtLength(0);
            const end = path.getPointAtLength(length);

            if (state.nodes.some(node => node.id === parent.dataset.from && node.kind === 'domain' && node.scope !== 'organization')) {
                personalAngles.push(Math.atan2(start.y - source.y, start.x - source.x) * 180 / Math.PI);
            }

            maxEndpointError = Math.max(
                maxEndpointError,
                Math.abs(distance(start, source) - source.r),
                Math.abs(distance(end, target) - target.r),
            );

            for (const [tip, near, center] of [
                [start, path.getPointAtLength(0.1), source],
                [end, path.getPointAtLength(length - 0.1), target],
            ]) {
                const tx = near.x - tip.x;
                const ty = near.y - tip.y;
                const rx = tip.x - center.x;
                const ry = tip.y - center.y;
                minAlignment = Math.min(minAlignment, (tx * rx + ty * ry) / (Math.hypot(tx, ty) * Math.hypot(rx, ry)));
            }

            for (let index = 0; index <= 500; index++) {
                const point = path.getPointAtLength(length * index / 500);

                if (distance(point, source) < source.r - 0.005 || distance(point, target) < target.r - 0.005) {
                    throw new Error(`Curve inside circle: ${id}`);
                }
            }
        }

        personalAngles.sort((left, right) => left - right);
        const personalGaps = personalAngles.map((angle, index) =>
            (personalAngles[(index + 1) % personalAngles.length] - angle + 360) % 360);

        if (personalGaps.some(gap => Math.abs(gap - 360 / personalAngles.length) > 0.001)) {
            throw new Error('Uneven private connection spacing');
        }

        const columns = [];

        const groups = state.nodes.filter(node => node.kind === 'publication' && node.show_in_readme)
            .sort((left, right) => left.x - right.x);
        const scale = state.canvas.width / 1800;

        for (const group of groups) {
            const childIds = state.edges.filter(edge => edge.from === group.id && edge.kind === 'publishes')
                .map(edge => edge.to);
            const children = state.nodes.filter(node => node.kind === 'package' && node.show_in_readme && childIds.includes(node.id))
                .sort((left, right) => left.y - right.y);
            const count = children.length;
            if (count === 0) {
                columns.push(null);
                continue;
            }

            const x = children[0].x / scale;
            const glyphs = children.map(child => svg.querySelector(
                `[data-node-id="${CSS.escape(child.id)}"] g[transform]`,
            ));
            const candidates = [...svg.querySelectorAll('.package-connector')];
            // Distinct package rows can reuse the same column; identify each actual endpoint.
            const paths = glyphs.map(glyph => candidates.find(path => {
                const values = path.getAttribute('d').match(/-?\d+(?:\.\d+)?/g).map(Number);
                const cy = glyph.transform.baseVal.getItem(0).matrix.f;
                const radius = Number(glyph.querySelector('circle').getAttribute('r'));

                return values[0] === x && values[2] === cy - radius;
            }));

            if (glyphs.length !== count || paths.length !== count || paths.some(path => !path)) {
                throw new Error(`Missing package column: ${x}`);
            }

            for (let index = 0; index < count; index++) {
                const values = paths[index].getAttribute('d').match(/-?\d+(?:\.\d+)?/g).map(Number);
                const cy = glyphs[index].transform.baseVal.getItem(0).matrix.f;
                const radius = Number(glyphs[index].querySelector('circle').getAttribute('r'));

                if (values[0] !== x || values[2] !== cy - radius) {
                    throw new Error(`Wrong package segment end: ${x}/${index}`);
                }

                if (index > 0 && values[1] !== glyphs[index - 1].transform.baseVal.getItem(0).matrix.f + radius) {
                    throw new Error(`Wrong package segment start: ${x}/${index}`);
                }
            }

            columns.push(x);
        }

        return {
            curveCount: seen.size,
            personalPortAngles: personalAngles,
            personalPortGaps: personalGaps,
            maxEndpointError,
            minRadialAlignment: minAlignment,
            packageColumnCenters: columns,
            allCurvesOutsideCircles: true,
        };
    }, state);

    const visibleProjects = state.nodes.filter(node => node.kind === 'project' && node.show_in_readme);
    const connections = state.edges.filter(edge => edge.kind === 'contains' && edge.show_in_readme
        && visibleProjects.some(node => node.id === edge.to));

    assert.equal(results.curveCount, connections.length + Math.max(0, state.nodes.filter(node => node.kind === 'domain' && node.show_in_readme).length - 1));
    assert.equal(results.packageColumnCenters.length, state.nodes.filter(node => node.kind === 'publication' && node.show_in_readme).length);
    assert.ok(results.maxEndpointError <= 0.005, JSON.stringify(results));
    assert.ok(results.minRadialAlignment >= 0.999, JSON.stringify(results));
    return results;
}

/** Probe preprocessing and DOM guards through the actual profile fetch and attachment path. */
async function checkProfileSanitization(context, base) {
    const wrap = body => `<svg xmlns="http://www.w3.org/2000/svg">${body}</svg>`;
    const geometry = '<rect id="svg-probe-shape" width="4" height="3" />';
    const stylesheet = '@import url("https://example.invalid/svg-probe.css"); rect { fill: red; }';
    const fixtures = [
        { name: 'ordinary geometry', source: wrap(geometry), accepted: true, parses: 1 },
        { name: 'generated CSS', source: wrap(`<style>${stylesheet}</style>${geometry}`), accepted: true, parses: 1 },
        { name: 'multiple CSS blocks', source: wrap(`<style type="text/css">${stylesheet}</style>`
            + `<STYLE>${stylesheet}</STYLE>${geometry}`), accepted: true, parses: 1 },
        { name: 'event attributes', source: wrap('<g onload="globalThis.svgProbeExecuted = true">'
            + `${geometry}</g>`), accepted: true, parses: 1 },
        { name: 'quoted metadata and text', source: wrap('<g data-note=" > style=example">'
            + '<text> style=example </text>' + geometry + '</g>'), accepted: true, parses: 1,
            metadata: { note: ' > style=example', text: ' style=example ' } },
        { name: 'inline style attribute', source: wrap('<g style="fill: red">' + geometry + '</g>'),
            accepted: false, parses: 0 },
        { name: 'inline style after quoted angle', source: wrap('<g data-note=">" style="fill: red">'
            + geometry + '</g>'), accepted: false, parses: 0 },
        { name: 'safe links and resources', source: wrap('<defs><linearGradient id="safe-gradient" /></defs>'
            + '<a href="https://example.invalid/svg-probe-link"><rect id="svg-probe-shape" width="4"'
            + ' fill="url(#safe-gradient)" /></a>'), accepted: true, parses: 1 },
        { name: 'recreated style tag', source: wrap(`<sty<style>discard</style>le>${stylesheet}`
            + '</sty<style>discard</style>le>' + geometry), accepted: false, parses: 0 },
        { name: 'nested style tag', source: wrap(`<style><style>discard</style>${stylesheet}</style>`),
            accepted: false, parses: 0 },
        { name: 'unclosed style tag', source: wrap(`<style>${stylesheet}`), accepted: false, parses: 0 },
        { name: 'self-closing style tag', source: wrap('<style />'), accepted: false, parses: 0 },
        { name: 'unpaired closing style tag', source: wrap('</style>'), accepted: false, parses: 0 },
        { name: 'non-style XML names', source: wrap('<stylesheet /><styleable />' + geometry),
            accepted: true, parses: 1 },
        { name: 'qualified style tag', source: wrap(`<s:style xmlns:s="http://www.w3.org/2000/svg">`
            + `${stylesheet}</s:style>`), accepted: false, parses: 0 },
        { name: 'entity declarations', source: '<!DOCTYPE svg [<!ENTITY css "&#60;style&#62;rect {}&#60;/style&#62;">]>'
            + wrap('&css;'), accepted: false, parses: 0 },
        { name: 'stylesheet instruction', source: '<?xml-stylesheet type="text/css"'
            + ' href="https://example.invalid/svg-probe.css"?>' + wrap(geometry), accepted: false, parses: 0 },
        { name: 'invalid root', source: '<html xmlns="http://www.w3.org/1999/xhtml" />',
            accepted: false, parses: 1 },
        { name: 'malformed XML', source: '<svg xmlns="http://www.w3.org/2000/svg"><rect></svg>',
            accepted: false, parses: 1, parserError: true },
        { name: 'script', source: wrap('<script>globalThis.svgProbeExecuted = true</script>'),
            accepted: false, parses: 1 },
        { name: 'recreated script tag', source: wrap('<scr<style>discard</style>ipt>'
            + 'globalThis.svgProbeExecuted = true</scr<style>discard</style>ipt>'), accepted: false, parses: 1 },
        { name: 'foreign content', source: wrap('<foreignObject><iframe src="https://example.invalid/svg-probe" />'
            + '</foreignObject>'), accepted: false, parses: 1 },
        { name: 'external image', source: wrap('<image href="https://example.invalid/svg-probe.png" />'),
            accepted: false, parses: 1 },
        { name: 'external use', source: wrap('<use href="https://example.invalid/svg-probe.svg#shape" />'),
            accepted: false, parses: 1 },
        { name: 'animated mutation', source: wrap('<set attributeName="onload"'
            + ' to="globalThis.svgProbeExecuted = true" />'), accepted: false, parses: 1 },
        { name: 'executable link', source: wrap('<a href="javascript:globalThis.svgProbeExecuted = true">'
            + geometry + '</a>'), accepted: false, parses: 1 },
        { name: 'external resource', source: wrap('<rect fill="url(https://example.invalid/svg-probe.svg)" />'),
            accepted: false, parses: 1 },
    ];
    const results = [];

    for (const fixture of fixtures) {
        const page = await context.newPage();
        const errors = [];
        const requests = [];
        page.on('pageerror', error => errors.push(error.message));
        page.on('console', message => {
            if (message.type() === 'error') errors.push(message.text());
        });
        page.on('request', request => {
            const url = new URL(request.url());
            if (url.origin !== new URL(base).origin || url.pathname.includes('svg-probe')) {
                requests.push(request.url());
            }
        });
        await page.route('**/*svg-probe*', route => route.abort());
        await page.route('**/sourcefield.*.svg', route => route.fulfill({
            contentType: 'image/svg+xml', body: fixture.source,
        }));
        await page.addInitScript(() => {
            globalThis.svgProbeExecuted = false;
            globalThis.svgProbeParses = 0;
            globalThis.svgProbePolicyViolations = [];
            globalThis.svgProbeParserErrors = [];
            const parse = DOMParser.prototype.parseFromString;
            DOMParser.prototype.parseFromString = function (...args) {
                globalThis.svgProbeParses++;
                const parsed = parse.apply(this, args);

                if (parsed.querySelector('parsererror')) {
                    globalThis.svgProbeParserErrors.push({
                        styleElements: parsed.querySelectorAll('style').length,
                        styledElements: parsed.querySelectorAll('[style]').length,
                    });
                }

                return parsed;
            };
            document.addEventListener('securitypolicyviolation', event => {
                globalThis.svgProbePolicyViolations.push(event.violatedDirective);
            });
        });

        try {
            await page.goto(base);
            await page.waitForFunction(() => document.querySelector('#profile-view svg')
                || document.querySelector('#profile-view').textContent.includes('The field image is unavailable.'));
            await paint(page);
            const result = await page.evaluate(() => {
                const svg = document.querySelector('#profile-view svg');
                return {
                    accepted: Boolean(svg), parses: globalThis.svgProbeParses,
                    executed: globalThis.svgProbeExecuted, policyViolations: globalThis.svgProbePolicyViolations,
                    parserErrors: globalThis.svgProbeParserErrors,
                    styles: svg?.querySelectorAll('style, [style]').length ?? 0,
                    handlers: svg ? [...svg.querySelectorAll('*')].flatMap(element => [...element.attributes])
                        .filter(attribute => attribute.localName.toLowerCase().startsWith('on')).length : 0,
                    width: svg?.querySelector('#svg-probe-shape')?.getAttribute('width') ?? null,
                    metadata: {
                        note: svg?.querySelector('[data-note]')?.getAttribute('data-note') ?? null,
                        text: svg?.querySelector('text')?.textContent ?? null,
                    },
                };
            });

            assert.equal(result.accepted, fixture.accepted, fixture.name);
            assert.equal(result.parses, fixture.parses, fixture.name);
            assert.equal(result.executed, false, fixture.name);
            if (fixture.parserError) {
                assert.equal(result.parserErrors.length, 1, fixture.name);
                // Chromium's XML diagnostic document carries built-in styles; the host still rejects it.
                assert.ok(result.parserErrors[0].styleElements + result.parserErrors[0].styledElements > 0);
                assert.ok(result.policyViolations.every(value => ['style-src-elem', 'style-src-attr'].includes(value)));
                assert.ok(errors.every(value => value.includes('Applying inline style violates')));
            } else {
                assert.deepEqual(result.parserErrors, [], fixture.name);
                assert.deepEqual(result.policyViolations, [], fixture.name);
                assert.deepEqual(errors, [], fixture.name);
            }
            assert.deepEqual(requests, [], fixture.name);
            assert.equal(result.styles, 0, fixture.name);
            assert.equal(result.handlers, 0, fixture.name);
            assert.deepEqual(result.metadata, fixture.metadata ?? { note: null, text: null }, fixture.name);
            if (fixture.accepted) assert.equal(result.width, '4', fixture.name);
            results.push({ name: fixture.name, ...result, errors, requests });
        } finally {
            await page.close();
        }
    }

    return results;
}

/**
 * Verify approved field geometry and animation behavior through theme and motion transitions.
 * @param {{browser: import('playwright').Browser, base: string, output: string}} options Runtime and artifact location.
 * @returns {Promise<object>} Motion, lifecycle, geometry, and browser error evidence.
 */
export async function runFieldChecks({ browser, base, output }) {
    const context = await browser.newContext({ viewport: { width: 1500, height: 1100 }, colorScheme: 'dark' });

    try {
        const page = await context.newPage();
        const errors = [];
        page.on('pageerror', error => errors.push(error.message));
        page.on('console', message => {
            if (message.type() === 'error') {
                errors.push(message.text());
            }
        });

        await page.goto(base);
        await page.waitForSelector('#profile-view svg');

        const stateResponse = await page.request.get(new URL('profile-state.json', base).href);
        assert.equal(stateResponse.ok(), true);
        const state = await stateResponse.json();
        const result = {
            dark: await checkMotion(page, state), connections: await checkConnections(page, state),
            sanitization: await checkProfileSanitization(context, base),
        };
        const before = await geometry(page);

        await paint(page);

        assert.deepEqual(await geometry(page), before);
        await page.locator('[data-layer="systems"]').click();
        await paint(page);
        await page.locator('#stage').hover();
        await page.mouse.move(800, 450);
        await page.mouse.down();
        await page.mouse.move(850, 480);
        await page.mouse.up();
        await page.locator('[data-layer="overview"]').click();

        assert.deepEqual(await geometry(page), before);
        result.anchoredOverview = true;
        result.returnFromSystems = await checkMotion(page, state);
        await page.locator('#motion-button').click();

        assert.equal(await page.evaluate(() => document.querySelector('#profile-view svg')
            .getAnimations({ subtree: true }).every(animation => animation.playState === 'paused')), true);
        result.pause = true;
        await page.screenshot({ path: join(output, 'field-desktop-dark.png'), fullPage: true });

        const darkSvg = await page.locator('#profile-view svg').elementHandle();

        await page.emulateMedia({ colorScheme: 'light' });
        await darkSvg.waitForElementState('hidden');
        await darkSvg.dispose();
        await page.waitForSelector('#profile-view svg');
        await paint(page);

        assert.equal(await page.evaluate(() => document.querySelector('#profile-view svg')
            .getAnimations({ subtree: true }).every(animation => animation.playState === 'paused')), true);
        await page.screenshot({ path: join(output, 'field-desktop-light.png'), fullPage: true });
        await page.locator('#motion-button').click();
        result.light = await checkMotion(page, state);

        await page.emulateMedia({ reducedMotion: 'reduce' });
        await page.waitForFunction(() => document.querySelector('#motion-button').disabled);

        assert.equal(await page.locator('#motion-button').isDisabled(), true);
        assert.equal(await page.evaluate(() => document.querySelector('#profile-view svg')
            .getAnimations({ subtree: true }).filter(animation => animation.playState === 'running').length), 0);

        await page.reload();
        await page.waitForSelector('#profile-view svg');
        await page.emulateMedia({ reducedMotion: 'no-preference' });
        await page.waitForFunction(() => !document.querySelector('#motion-button').disabled);

        result.initialReducedRestoration = await checkMotion(page, state);
        await page.setViewportSize({ width: 390, height: 844 });
        await page.screenshot({ path: join(output, 'field-mobile.png'), fullPage: true });

        result.errors = errors;
        assert.deepEqual(errors, []);
        return result;
    } finally {
        await context.close();
    }
}
